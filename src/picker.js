// Injected into every page the Viper browser opens. Viper turns picking on
// with `window.__viper.pick(true)` and commenting with
// `window.__viper.comment(true, nextNumber)`; the user then hovers to highlight
// an element and clicks to send its details back through `window.ipc`. Picking
// sends one element and stops; commenting opens an in-page comment popover right
// where the user clicked, pins a numbered marker on the element, and sends the
// note back to the session.
(() => {
  if (window.__viper) return;

  let mode = null; // null, "pick" or "comment"
  let box = null;
  let label = null;
  let current = null;
  let nextNumber = 1;
  const pins = [];
  let activePopover = null;

  const describe = (el) =>
    el.localName +
    (el.id ? "#" + el.id : "") +
    [...el.classList].slice(0, 2).map((c) => "." + c).join("");

  // A CSS selector that finds this element again, as short as an id allows.
  const selectorFor = (el) => {
    const parts = [];
    while (el && el.nodeType === Node.ELEMENT_NODE && el !== document.documentElement) {
      if (el.id) {
        parts.unshift("#" + CSS.escape(el.id));
        break;
      }
      let part = el.localName + [...el.classList].slice(0, 2).map((c) => "." + CSS.escape(c)).join("");
      const parent = el.parentElement;
      if (parent) {
        const sameTag = [...parent.children].filter((child) => child.localName === el.localName);
        if (sameTag.length > 1) part += ":nth-of-type(" + (sameTag.indexOf(el) + 1) + ")";
      }
      parts.unshift(part);
      el = parent;
    }
    return parts.join(" > ");
  };

  const details = (el, kind) => {
    const r = el.getBoundingClientRect();
    return {
      kind,
      url: location.href,
      selector: selectorFor(el),
      tag: describe(el),
      text: (el.innerText || "").trim().slice(0, 300),
      html: el.outerHTML.slice(0, 3000),
      width: Math.round(r.width),
      height: Math.round(r.height),
    };
  };

  const highlight = (el) => {
    const r = el.getBoundingClientRect();
    Object.assign(box.style, { left: r.left + "px", top: r.top + "px", width: r.width + "px", height: r.height + "px" });
    if (mode === "comment") {
      box.style.outlineColor = "#d9773f";
      box.style.background = "rgba(217,119,63,.15)";
      label.textContent = describe(el) + "  ·  Click to comment";
    } else {
      box.style.outlineColor = "#4285f4";
      box.style.background = "rgba(66,133,244,.18)";
      label.textContent = describe(el) + "  " + Math.round(r.width) + " × " + Math.round(r.height);
    }
    label.style.left = Math.max(0, r.left) + "px";
    label.style.top = (r.top > 24 ? r.top - 22 : r.bottom + 4) + "px";
  };

  // A numbered marker that stays on its element while the page scrolls.
  const addPin = (el, number, note = "") => {
    const marker = document.createElement("div");
    marker.textContent = number;
    marker.title = note ? `#${number}: ${note}` : `#${number} (click to view/edit)`;
    marker.style.cssText =
      "position:fixed;z-index:2147483646;pointer-events:auto;cursor:pointer;min-width:20px;height:20px;" +
      "box-sizing:border-box;padding:0 5px;border-radius:10px;background:#d9773f;color:#fff;" +
      "font:600 12px/20px ui-sans-serif,system-ui,sans-serif;text-align:center;" +
      "box-shadow:0 1px 4px rgba(0,0,0,.4);transition:transform .12s ease;";
    marker.addEventListener("mouseenter", () => { marker.style.transform = "scale(1.15)"; });
    marker.addEventListener("mouseleave", () => { marker.style.transform = ""; });
    marker.addEventListener("click", (e) => {
      block(e);
      const found = pins.find((p) => p.number === number);
      if (found) openCommentPopover(found.el, found.number, found.note || "", false);
    });

    const outline = document.createElement("div");
    outline.style.cssText =
      "position:fixed;z-index:2147483645;pointer-events:none;border:1px dashed #d9773f;" +
      "background:rgba(217,119,63,.10);border-radius:2px;";
    document.documentElement.append(outline, marker);
    pins.push({ number, el, marker, outline, note });
    layoutPins();
  };

  const closeActivePopover = (cancelIfNew = true) => {
    if (!activePopover) return;
    const { popover, isNew, number } = activePopover;
    popover.remove();
    activePopover = null;
    if (cancelIfNew && isNew) {
      removePin(number);
    }
  };

  const openCommentPopover = (el, number, initialNote = "", isNew = true) => {
    closeActivePopover(true);

    const popover = document.createElement("div");
    popover.style.cssText =
      "position:fixed;z-index:2147483647;width:280px;box-sizing:border-box;" +
      "background:#18181b;color:#f4f4f5;border:1px solid #3f3f46;border-radius:8px;" +
      "box-shadow:0 12px 28px rgba(0,0,0,.5),0 2px 8px rgba(0,0,0,.25);" +
      "padding:10px 12px;font:13px/1.4 ui-sans-serif,system-ui,-apple-system,sans-serif;";

    const updatePos = () => {
      if (!popover.isConnected) return;
      const r = el.getBoundingClientRect();
      const popW = 280;
      const popH = 150;
      let left = r.left;
      if (left + popW > window.innerWidth - 12) {
        left = Math.max(12, window.innerWidth - popW - 12);
      }
      if (left < 12) left = 12;
      let top = r.bottom + 8;
      if (top + popH > window.innerHeight - 12 && r.top > popH + 16) {
        top = r.top - popH - 8;
      }
      popover.style.left = Math.round(left) + "px";
      popover.style.top = Math.round(top) + "px";
    };

    popover.innerHTML = `
      <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:8px;">
        <div style="display:flex;align-items:center;gap:6px;min-width:0;">
          <span style="background:#d9773f;color:#fff;border-radius:4px;padding:1px 6px;font-weight:700;font-size:11px;">#${number}</span>
          <span style="color:#a1a1aa;font-family:ui-monospace,Consolas,monospace;font-size:11px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;">${describe(el)}</span>
        </div>
        <button class="v-close" style="background:none;border:none;color:#71717a;cursor:pointer;font-size:16px;padding:0 2px;line-height:1;" title="Close (Esc)">×</button>
      </div>
      <textarea class="v-note" placeholder="What should change here?" rows="3" style="width:100%;box-sizing:border-box;height:64px;resize:none;background:#27272a;border:1px solid #52525b;border-radius:6px;color:#fafafa;font-size:12.5px;padding:6px 8px;font-family:inherit;outline:none;line-height:1.35;"></textarea>
      <div style="display:flex;align-items:center;justify-content:space-between;margin-top:8px;">
        <span style="color:#71717a;font-size:10.5px;">Enter to submit</span>
        <div style="display:flex;gap:6px;">
          ${!isNew ? '<button class="v-del" style="background:transparent;border:1px solid #dc2626;color:#f87171;border-radius:4px;padding:3px 8px;font-size:11px;cursor:pointer;">Delete</button>' : ''}
          <button class="v-cancel" style="background:transparent;border:1px solid #3f3f46;color:#a1a1aa;border-radius:4px;padding:3px 8px;font-size:11px;cursor:pointer;">Cancel</button>
          <button class="v-save" style="background:#d9773f;border:none;color:#fff;font-weight:600;border-radius:4px;padding:3px 10px;font-size:11px;cursor:pointer;">${isNew ? 'Add Comment' : 'Save'}</button>
        </div>
      </div>
    `;

    document.documentElement.append(popover);
    updatePos();

    activePopover = { popover, el, number, isNew, updatePos };

    const textarea = popover.querySelector(".v-note");
    textarea.value = initialNote;
    textarea.addEventListener("focus", () => {
      textarea.style.borderColor = "#d9773f";
      textarea.style.boxShadow = "0 0 0 1px #d9773f";
    });
    textarea.addEventListener("blur", () => {
      textarea.style.borderColor = "#52525b";
      textarea.style.boxShadow = "none";
    });

    // Don't let clicks or keys inside the card bubble up to page handlers
    popover.addEventListener("mousedown", (e) => e.stopPropagation());
    popover.addEventListener("mouseup", (e) => e.stopPropagation());
    popover.addEventListener("click", (e) => e.stopPropagation());
    popover.addEventListener("keydown", (e) => e.stopPropagation());

    const submit = () => {
      const note = textarea.value.trim();
      const pin = pins.find((p) => p.number === number);
      if (pin) {
        pin.note = note;
        pin.marker.title = note ? `#${number}: ${note}` : `#${number} (click to view/edit)`;
      }
      const message = details(el, "commented");
      message.number = number;
      message.note = note;
      if (window.ipc) {
        window.ipc.postMessage(JSON.stringify(message));
      }
      closeActivePopover(false);
    };

    const cancel = () => {
      closeActivePopover(true);
    };

    const deletePin = () => {
      removePin(number);
      if (window.ipc) {
        window.ipc.postMessage(JSON.stringify({ kind: "comment_removed", number }));
      }
      closeActivePopover(false);
    };

    textarea.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        submit();
      } else if (e.key === "Escape") {
        e.preventDefault();
        cancel();
      }
    });

    popover.querySelector(".v-save").addEventListener("click", submit);
    popover.querySelector(".v-cancel").addEventListener("click", cancel);
    popover.querySelector(".v-close").addEventListener("click", cancel);
    const delBtn = popover.querySelector(".v-del");
    if (delBtn) delBtn.addEventListener("click", deletePin);

    setTimeout(() => {
      textarea.focus();
      textarea.select();
    }, 15);
  };

  const layoutPins = () => {
    for (const pin of pins) {
      const r = pin.el.getBoundingClientRect();
      const gone = !pin.el.isConnected || (r.width === 0 && r.height === 0);
      const display = gone ? "none" : "block";
      pin.marker.style.display = display;
      pin.outline.style.display = display;
      if (gone) continue;
      Object.assign(pin.outline.style, {
        left: r.left + "px",
        top: r.top + "px",
        width: r.width + "px",
        height: r.height + "px",
      });
      pin.marker.style.left = Math.max(2, r.left - 8) + "px";
      pin.marker.style.top = Math.max(2, r.top - 10) + "px";
    }
    if (activePopover && activePopover.updatePos) {
      activePopover.updatePos();
    }
  };

  const clearPins = () => {
    closeActivePopover(false);
    for (const pin of pins) {
      pin.marker.remove();
      pin.outline.remove();
    }
    pins.length = 0;
  };

  const removePin = (number) => {
    if (activePopover && activePopover.number === number) {
      activePopover.popover.remove();
      activePopover = null;
    }
    const index = pins.findIndex((pin) => pin.number === number);
    if (index < 0) return;
    pins[index].marker.remove();
    pins[index].outline.remove();
    pins.splice(index, 1);
  };

  const block = (event) => {
    event.preventDefault();
    event.stopPropagation();
    event.stopImmediatePropagation();
  };

  const onMove = (event) => {
    current = event.target;
    highlight(current);
  };

  const onClick = (event) => {
    block(event);
    const el = current || event.target;
    if (mode === "comment") {
      const number = nextNumber++;
      addPin(el, number);
      openCommentPopover(el, number, "", true);
      return;
    }
    const message = details(el, "picked");
    stop();
    if (window.ipc) {
      window.ipc.postMessage(JSON.stringify(message));
    }
  };

  const onKey = (event) => {
    if (event.key !== "Escape") return;
    block(event);
    if (activePopover) {
      closeActivePopover(true);
      return;
    }
    stop();
    if (window.ipc) {
      window.ipc.postMessage(JSON.stringify({ kind: "cancelled" }));
    }
  };

  // Clicks must not reach the page while picking, or they would follow links and press buttons.
  const listeners = [
    ["mousemove", onMove],
    ["click", onClick],
    ["mousedown", block],
    ["mouseup", block],
    ["keydown", onKey],
  ];

  function start(which, from) {
    if (mode === which) return;
    if (mode) stop();
    mode = which;
    if (from) nextNumber = from;
    box = document.createElement("div");
    const outlineColor = which === "comment" ? "#d9773f" : "#4285f4";
    const bg = which === "comment" ? "rgba(217,119,63,.15)" : "rgba(66,133,244,.18)";
    box.style.cssText =
      `position:fixed;z-index:2147483647;pointer-events:none;background:${bg};` +
      `outline:2px solid ${outlineColor};border-radius:2px;`;
    label = document.createElement("div");
    label.style.cssText =
      "position:fixed;z-index:2147483647;pointer-events:none;background:#18181b;color:#fff;" +
      "font:12px/18px ui-monospace,Consolas,monospace;padding:2px 8px;border-radius:4px;white-space:nowrap;" +
      "box-shadow:0 2px 8px rgba(0,0,0,.4);border:1px solid #3f3f46;";
    document.documentElement.append(box, label);
    for (const [type, listener] of listeners) window.addEventListener(type, listener, true);
    document.documentElement.style.cursor = "crosshair";
  }

  function stop() {
    if (!mode) return;
    closeActivePopover(true);
    mode = null;
    if (box) box.remove();
    if (label) label.remove();
    box = null;
    label = null;
    current = null;
    for (const [type, listener] of listeners) window.removeEventListener(type, listener, true);
    document.documentElement.style.cursor = "";
  }

  // Pins are placed in viewport coordinates, so they follow the page as it moves.
  window.addEventListener("scroll", layoutPins, true);
  window.addEventListener("resize", layoutPins, true);

  window.__viper = {
    pick: (on) => (on ? start("pick") : stop()),
    comment: (on, from) => (on ? start("comment", from) : stop()),
    clearPins,
    removePin,
  };
})();
