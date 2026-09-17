//! Voice input: dictation into the message box using the speech recognition
//! built into the operating system (Windows only for now).

use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;

pub enum VoiceEvent {
    /// The words heard so far in the current phrase. They may still change.
    Partial(String),
    /// A finished phrase to add to the message.
    Final(String),
    /// Listening ended, with an explanation if something went wrong.
    Stopped(Option<VoiceError>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum VoiceError {
    /// Windows' "Online speech recognition" privacy setting is off.
    SpeechPrivacyOff,
    MicrophoneUnavailable,
    Other(String),
}

impl VoiceError {
    pub fn message(&self) -> String {
        match self {
            Self::SpeechPrivacyOff => {
                "Voice input needs Windows' online speech recognition. Turn it on in Settings → Privacy & security → Speech."
                    .to_owned()
            }
            Self::MicrophoneUnavailable => {
                "No microphone is available, or Windows isn't letting apps use it.".to_owned()
            }
            Self::Other(message) => format!("Voice input stopped: {message}"),
        }
    }
}

/// A dictation in progress. Dropping it stops listening.
pub struct Dictation {
    stop: Sender<()>,
    events: Receiver<VoiceEvent>,
}

impl Dictation {
    pub fn start(ctx: &egui::Context) -> Self {
        let (stop, stop_rx) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let notify = move |event: VoiceEvent| {
                let _ = events_tx.send(event);
                ctx.request_repaint();
            };
            platform::listen(stop_rx, notify);
        });
        Self { stop, events }
    }

    pub fn stop(&self) {
        let _ = self.stop.send(());
    }

    pub fn poll(&self) -> Vec<VoiceEvent> {
        self.events.try_iter().collect()
    }
}

impl Drop for Dictation {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Opens the system page where speech recognition can be turned on.
pub fn open_speech_settings() {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg("ms-settings:privacy-speech").spawn();
}

/// Adds dictated text to a message, with a space between it and what's already there.
pub fn append_phrase(input: &mut String, phrase: &str) {
    let phrase = phrase.trim();
    if phrase.is_empty() {
        return;
    }
    if !input.is_empty() && !input.ends_with(char::is_whitespace) {
        input.push(' ');
    }
    input.push_str(phrase);
}

#[cfg(windows)]
mod platform {
    use std::sync::mpsc::Receiver;
    use std::sync::{Arc, Mutex, PoisonError};

    use windows::Foundation::{TimeSpan, TypedEventHandler};
    use windows::Media::SpeechRecognition::{
        SpeechContinuousRecognitionCompletedEventArgs, SpeechContinuousRecognitionResultGeneratedEventArgs,
        SpeechContinuousRecognitionSession, SpeechRecognitionConfidence, SpeechRecognitionHypothesisGeneratedEventArgs,
        SpeechRecognitionResultStatus, SpeechRecognitionScenario, SpeechRecognitionTopicConstraint, SpeechRecognizer,
    };
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
    use windows::core::HSTRING;

    use super::{VoiceError, VoiceEvent};

    /// "The speech privacy policy was not accepted prior to attempting a speech recognition."
    const PRIVACY_POLICY_NOT_ACCEPTED: i32 = 0x8004_5509_u32 as i32;

    pub fn listen(stop: Receiver<()>, notify: impl Fn(VoiceEvent) + Send + Sync + 'static) {
        let notify: Arc<dyn Fn(VoiceEvent) + Send + Sync> = Arc::new(notify);
        let error = run(&stop, Arc::clone(&notify)).err();
        notify(VoiceEvent::Stopped(error));
    }

    /// Sets up a dictation recognizer. This doesn't use the microphone yet.
    pub fn prepare() -> Result<SpeechRecognizer, VoiceError> {
        // Speech recognition calls back from other threads, so this thread joins the multithreaded apartment.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(to_error)?;

        let recognizer = SpeechRecognizer::new().map_err(to_error)?;
        let dictation =
            SpeechRecognitionTopicConstraint::Create(SpeechRecognitionScenario::Dictation, &HSTRING::from("dictation"))
                .map_err(to_error)?;
        recognizer.Constraints().map_err(to_error)?.Append(&dictation).map_err(to_error)?;
        let compiled = recognizer.CompileConstraintsAsync().map_err(to_error)?.join().map_err(to_error)?;
        let status = compiled.Status().map_err(to_error)?;
        if status != SpeechRecognitionResultStatus::Success {
            return Err(status_error(status));
        }
        Ok(recognizer)
    }

    fn run(stop: &Receiver<()>, notify: Arc<dyn Fn(VoiceEvent) + Send + Sync>) -> Result<(), VoiceError> {
        let recognizer = prepare()?;

        let partial = Arc::clone(&notify);
        recognizer
            .HypothesisGenerated(&TypedEventHandler::<SpeechRecognizer, SpeechRecognitionHypothesisGeneratedEventArgs>::new(
                move |_, args| {
                    if let Some(args) = args.as_ref() {
                        partial(VoiceEvent::Partial(args.Hypothesis()?.Text()?.to_string()));
                    }
                    Ok(())
                },
            ))
            .map_err(to_error)?;

        let session: SpeechContinuousRecognitionSession = recognizer.ContinuousRecognitionSession().map_err(to_error)?;
        // Keep listening through pauses; the user stops it with the button.
        session.SetAutoStopSilenceTimeout(TimeSpan { Duration: 10 * 60 * 10_000_000 }).map_err(to_error)?;

        let finals = Arc::clone(&notify);
        session
            .ResultGenerated(&TypedEventHandler::<
                SpeechContinuousRecognitionSession,
                SpeechContinuousRecognitionResultGeneratedEventArgs,
            >::new(move |_, args| {
                if let Some(args) = args.as_ref() {
                    let result = args.Result()?;
                    if result.Status()? == SpeechRecognitionResultStatus::Success
                        && result.Confidence()? != SpeechRecognitionConfidence::Rejected
                    {
                        finals(VoiceEvent::Final(result.Text()?.to_string()));
                    }
                }
                Ok(())
            }))
            .map_err(to_error)?;

        // The session can end on its own, e.g. if the microphone disappears or the network drops.
        let ended: Arc<Mutex<Option<Result<(), VoiceError>>>> = Arc::new(Mutex::new(None));
        let ended_slot = Arc::clone(&ended);
        session
            .Completed(&TypedEventHandler::<
                SpeechContinuousRecognitionSession,
                SpeechContinuousRecognitionCompletedEventArgs,
            >::new(move |_, args| {
                let status = args.as_ref().map(|args| args.Status()).transpose()?;
                let outcome = match status {
                    Some(status) if status != SpeechRecognitionResultStatus::Success
                        && status != SpeechRecognitionResultStatus::UserCanceled =>
                    {
                        Err(status_error(status))
                    }
                    _ => Ok(()),
                };
                *ended_slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome);
                Ok(())
            }))
            .map_err(to_error)?;

        session.StartAsync().map_err(to_error)?.join().map_err(to_error)?;

        loop {
            if let Some(outcome) = ended.lock().unwrap_or_else(PoisonError::into_inner).take() {
                return outcome;
            }
            match stop.recv_timeout(std::time::Duration::from_millis(100)) {
                // A stop request, or the app dropped the dictation.
                Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        // Stopping lets the recognizer deliver the last phrase before it ends.
        if let Ok(stopping) = session.StopAsync() {
            let _ = stopping.join();
        }
        Ok(())
    }

    fn to_error(error: windows::core::Error) -> VoiceError {
        if error.code().0 == PRIVACY_POLICY_NOT_ACCEPTED {
            VoiceError::SpeechPrivacyOff
        } else {
            VoiceError::Other(error.message().to_string())
        }
    }

    fn status_error(status: SpeechRecognitionResultStatus) -> VoiceError {
        match status {
            SpeechRecognitionResultStatus::MicrophoneUnavailable => VoiceError::MicrophoneUnavailable,
            SpeechRecognitionResultStatus::NetworkFailure => {
                VoiceError::Other("Windows couldn't reach its speech service. Check your internet connection.".into())
            }
            SpeechRecognitionResultStatus::TopicLanguageNotSupported => {
                VoiceError::Other("Dictation isn't available for your Windows speech language.".into())
            }
            SpeechRecognitionResultStatus::TimeoutExceeded => VoiceError::Other("It stopped after a long silence.".into()),
            other => VoiceError::Other(format!("Windows speech recognition reported status {}.", other.0)),
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::sync::mpsc::Receiver;

    use super::{VoiceError, VoiceEvent};

    pub fn listen(_stop: Receiver<()>, notify: impl Fn(VoiceEvent) + Send + Sync + 'static) {
        notify(VoiceEvent::Stopped(Some(VoiceError::Other(
            "Voice input is only available on Windows so far.".to_owned(),
        ))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrases_are_joined_with_single_spaces() {
        let mut input = String::new();
        append_phrase(&mut input, " Fix the login bug ");
        append_phrase(&mut input, "on mobile.");
        assert_eq!(input, "Fix the login bug on mobile.");

        let mut typed = "Look at this:\n".to_owned();
        append_phrase(&mut typed, "the header");
        assert_eq!(typed, "Look at this:\nthe header");

        append_phrase(&mut typed, "   ");
        assert_eq!(typed, "Look at this:\nthe header");
    }

    /// Checks that Windows can set up dictation, without turning on the microphone.
    /// Its result depends on this PC's speech settings, so it only runs when asked for:
    /// `cargo test -- --ignored voice --nocapture`
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn windows_dictation_can_be_set_up() {
        match platform::prepare() {
            Ok(_) => println!("dictation is ready"),
            Err(error) => panic!("{}", error.message()),
        }
    }
}
