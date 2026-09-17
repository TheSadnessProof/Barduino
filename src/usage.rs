//! Token usage reported by the agent CLIs, added up per provider and per day.

use std::collections::BTreeMap;
use std::ops::AddAssign;

use serde::{Deserialize, Serialize};

use crate::agent::Provider;

/// Tokens used by one turn, or a sum of turns.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Usage {
    pub turns: u64,
    /// Input tokens that weren't read from or written to the cache.
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// The CLI's own cost estimate in US dollars, for CLIs that report one.
    pub cost_usd: Option<f64>,
}

impl Usage {
    pub fn total_tokens(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
}

impl AddAssign for Usage {
    fn add_assign(&mut self, other: Self) {
        self.turns += other.turns;
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
        self.cost_usd = match (self.cost_usd, other.cost_usd) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Today,
    Week,
    Month,
    AllTime,
}

impl Period {
    pub const ALL: [Period; 4] = [Self::Today, Self::Week, Self::Month, Self::AllTime];

    pub fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Week => "Last 7 days",
            Self::Month => "Last 30 days",
            Self::AllTime => "All time",
        }
    }

    /// The first day included, as "YYYY-MM-DD", or None for all time.
    fn first_day(self, today: chrono::NaiveDate) -> Option<String> {
        let days_back = match self {
            Self::Today => 0,
            Self::Week => 6,
            Self::Month => 29,
            Self::AllTime => return None,
        };
        Some(day_key(today - chrono::Days::new(days_back)))
    }
}

/// How long a self-set token limit lasts before it starts over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LimitPeriod {
    Day,
    Week,
    Month,
}

impl LimitPeriod {
    pub const ALL: [LimitPeriod; 3] = [Self::Day, Self::Week, Self::Month];

    pub fn label(self) -> &'static str {
        match self {
            Self::Day => "per day",
            Self::Week => "per week",
            Self::Month => "per month",
        }
    }

    /// How the spent part is described, e.g. "used today".
    pub fn window(self) -> &'static str {
        match self {
            Self::Day => "today",
            Self::Week => "in the last 7 days",
            Self::Month => "in the last 30 days",
        }
    }

    pub fn usage_period(self) -> Period {
        match self {
            Self::Day => Period::Today,
            Self::Week => Period::Week,
            Self::Month => Period::Month,
        }
    }
}

/// A token limit the user set for one provider. No agent CLI reports how much of a
/// subscription is left, so Barduino measures what it spent against a chosen number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenLimit {
    pub tokens: u64,
    pub period: LimitPeriod,
}

impl TokenLimit {
    /// What a new limit starts at, in tokens.
    pub const DEFAULT_TOKENS: u64 = 5_000_000;

    pub fn weekly() -> Self {
        Self { tokens: Self::DEFAULT_TOKENS, period: LimitPeriod::Week }
    }

    pub fn progress(self, used: u64) -> LimitProgress {
        LimitProgress {
            fraction: if self.tokens == 0 { 1.0 } else { (used as f64 / self.tokens as f64) as f32 },
            left: self.tokens.saturating_sub(used),
            over: used.saturating_sub(self.tokens),
        }
    }
}

/// Where usage stands against a limit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LimitProgress {
    /// The share of the limit used, which goes above 1.0 once it's passed.
    pub fraction: f32,
    pub left: u64,
    /// How far past the limit, or 0 while there's still something left.
    pub over: u64,
}

/// Usage per day (local time) and provider, for turns run through Barduino.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageLog {
    days: BTreeMap<String, BTreeMap<Provider, Usage>>,
}

impl UsageLog {
    pub fn record(&mut self, provider: Provider, usage: Usage) {
        self.record_on(chrono::Local::now().date_naive(), provider, usage);
    }

    fn record_on(&mut self, day: chrono::NaiveDate, provider: Provider, usage: Usage) {
        *self.days.entry(day_key(day)).or_default().entry(provider).or_default() += usage;
    }

    pub fn total(&self, provider: Provider, period: Period) -> Usage {
        self.total_on(chrono::Local::now().date_naive(), provider, period)
    }

    fn total_on(&self, today: chrono::NaiveDate, provider: Provider, period: Period) -> Usage {
        let first_day = period.first_day(today).unwrap_or_default();
        let mut total = Usage::default();
        // Day keys are ISO dates, so they sort in date order.
        for per_provider in self.days.range(first_day..).map(|(_, day)| day) {
            if let Some(usage) = per_provider.get(&provider) {
                total += *usage;
            }
        }
        total
    }

    pub fn is_empty(&self) -> bool {
        self.days.is_empty()
    }

    pub fn clear(&mut self) {
        self.days.clear();
    }
}

fn day_key(day: chrono::NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

/// A short token count such as "950", "12.3K" or "1.24M".
pub fn format_tokens(tokens: u64) -> String {
    match tokens {
        0..1_000 => tokens.to_string(),
        1_000..1_000_000 => format!("{:.1}K", tokens as f64 / 1_000.0),
        _ => format!("{:.2}M", tokens as f64 / 1_000_000.0),
    }
}

pub fn format_cost(cost: Option<f64>) -> String {
    match cost {
        Some(cost) if cost < 0.01 && cost > 0.0 => "< $0.01".to_owned(),
        Some(cost) => format!("${cost:.2}"),
        None => "—".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    fn turn(input: u64, cost: Option<f64>) -> Usage {
        Usage { turns: 1, input, output: 10, cost_usd: cost, ..Default::default() }
    }

    #[test]
    fn totals_respect_the_period_and_provider() {
        let mut log = UsageLog::default();
        log.record_on(day("2026-09-17"), Provider::Claude, turn(100, Some(0.5)));
        log.record_on(day("2026-09-17"), Provider::Claude, turn(200, Some(0.25)));
        log.record_on(day("2026-09-12"), Provider::Claude, turn(1000, Some(1.0)));
        log.record_on(day("2026-08-01"), Provider::Claude, turn(5000, Some(2.0)));
        log.record_on(day("2026-09-17"), Provider::Antigravity, turn(7, None));

        let today = day("2026-09-17");
        let claude_today = log.total_on(today, Provider::Claude, Period::Today);
        assert_eq!((claude_today.turns, claude_today.input, claude_today.cost_usd), (2, 300, Some(0.75)));
        assert_eq!(log.total_on(today, Provider::Claude, Period::Week).input, 1300);
        assert_eq!(log.total_on(today, Provider::Claude, Period::Month).input, 1300);
        assert_eq!(log.total_on(today, Provider::Claude, Period::AllTime).input, 6300);

        let agy = log.total_on(today, Provider::Antigravity, Period::AllTime);
        assert_eq!((agy.turns, agy.total_tokens(), agy.cost_usd), (1, 17, None));
    }

    #[test]
    fn limits_report_what_is_left_and_what_is_over() {
        let limit = TokenLimit { tokens: 1_000_000, period: LimitPeriod::Week };
        let half = limit.progress(500_000);
        assert_eq!((half.fraction, half.left, half.over), (0.5, 500_000, 0));

        let past = limit.progress(1_250_000);
        assert_eq!((past.fraction, past.left, past.over), (1.25, 0, 250_000));

        // A limit of zero counts as fully used, so it can't show as "0% used".
        assert_eq!(TokenLimit { tokens: 0, period: LimitPeriod::Day }.progress(0).fraction, 1.0);
        assert_eq!(LimitPeriod::Week.usage_period(), Period::Week);
    }

    #[test]
    fn formats_numbers_compactly() {
        assert_eq!(format_tokens(950), "950");
        assert_eq!(format_tokens(12_345), "12.3K");
        assert_eq!(format_tokens(1_240_000), "1.24M");
        assert_eq!(format_cost(None), "—");
        assert_eq!(format_cost(Some(0.004)), "< $0.01");
        assert_eq!(format_cost(Some(3.456)), "$3.46");
    }
}
