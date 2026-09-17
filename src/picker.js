// Injected into every page the Barduino browser opens. Barduino turns picking on
// with `window.__barduino.pick(true)` and commenting with
// `window.__barduino.comment(true, nextNumber)`; the user then hovers to highlight
// an element and clicks to send its details back through `window.ipc`. Picking
// sends one element and stops; commenting leaves a numbered pin on the page and
// carries on, so several places can be marked in a row.
(() => {
  if (window.__barduino) return;

  let mode = null; // null, "pick" or "comment"
  let box = null;
  let label = null;
  let current = null;
  let nextNumber = 1;
  const pins = [];

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
    label.textContent = describe(el) + "  " + Math.round(r.width) + " × " + Math.round(r.height);
    label.style.left = Math.max(0, r.left) + "px";
    label.style.top = (r.top > 24 ? r.top - 22 : r.bottom + 4) + "px";
  };

  // A numbered marker that stays on its element while the page scrolls.
  const addPin = (el, number) => {
    const marker = document.createElement("div");
    marker.textContent = number;
    marker.style.cssText =
      "position:fixed;z-index:2147483647;pointer-events:none;min-width:20px;height:20px;" +
      "box-sizing:border-box;padding:0 5px;border-radius:10px;background:#d9773f;color:#fff;" +
      "font:600 12px/20px ui-sans-serif,system-ui,sans-serif;text-align:center;" +
      "box-shadow:0 1px 4px rgba(0,0,0,.4);";
    const outline = document.createElement("div");
    outline.style.cssText =
      "position:fixed;z-index:2147483646;pointer-events:none;border:1px dashed #d9773f;" +
      "background:rgba(217,119,63,.10);border-radius:2px;";
    document.documentElement.append(outline, marker);
    pins.push({ number, el, marker, outline });
    layoutPins();
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
  };

  const clearPins = () => {
    for (const pin of pins) {
      pin.marker.remove();
      pin.outline.remove();
    }
    pins.length = 0;
  };

  const removePin = (number) => {
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
      const message = details(el, "commented");
      message.number = number;
      addPin(el, number);
      window.ipc.postMessage(JSON.stringify(message));
      return;
    }
    const message = details(el, "picked");
    stop();
    window.ipc.postMessage(JSON.stringify(message));
  };

  const onKey = (event) => {
    if (event.key !== "Escape") return;
    block(event);
    stop();
    window.ipc.postMessage(JSON.stringify({ kind: "cancelled" }));
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
    box.style.cssText =
      "position:fixed;z-index:2147483647;pointer-events:none;background:rgba(66,133,244,.18);" +
      "outline:2px solid #4285f4;border-radius:2px;";
    label = document.createElement("div");
    label.style.cssText =
      "position:fixed;z-index:2147483647;pointer-events:none;background:#1f2937;color:#fff;" +
      "font:12px/18px ui-monospace,Consolas,monospace;padding:0 6px;border-radius:3px;white-space:nowrap;";
    document.documentElement.append(box, label);
    for (const [type, listener] of listeners) window.addEventListener(type, listener, true);
    document.documentElement.style.cursor = "crosshair";
  }

  function stop() {
    if (!mode) return;
    mode = null;
    box.remove();
    label.remove();
    current = null;
    for (const [type, listener] of listeners) window.removeEventListener(type, listener, true);
    document.documentElement.style.cursor = "";
  }

  // Pins are placed in viewport coordinates, so they follow the page as it moves.
  window.addEventListener("scroll", layoutPins, true);
  window.addEventListener("resize", layoutPins, true);

  window.__barduino = {
    pick: (on) => (on ? start("pick") : stop()),
    comment: (on, from) => (on ? start("comment", from) : stop()),
    clearPins,
    removePin,
  };
})();
