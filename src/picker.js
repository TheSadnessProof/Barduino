// Injected into every page the Barduino browser opens. Barduino turns picking
// on with `window.__barduino.pick(true)`; the user then hovers to highlight an
// element and clicks to send its details back through `window.ipc`.
(() => {
  if (window.__barduino) return;

  let active = false;
  let box = null;
  let label = null;
  let current = null;

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

  const highlight = (el) => {
    const r = el.getBoundingClientRect();
    Object.assign(box.style, { left: r.left + "px", top: r.top + "px", width: r.width + "px", height: r.height + "px" });
    label.textContent = describe(el) + "  " + Math.round(r.width) + " × " + Math.round(r.height);
    label.style.left = Math.max(0, r.left) + "px";
    label.style.top = (r.top > 24 ? r.top - 22 : r.bottom + 4) + "px";
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
    const r = el.getBoundingClientRect();
    stop();
    window.ipc.postMessage(
      JSON.stringify({
        kind: "picked",
        url: location.href,
        selector: selectorFor(el),
        tag: describe(el),
        text: (el.innerText || "").trim().slice(0, 300),
        html: el.outerHTML.slice(0, 3000),
        width: Math.round(r.width),
        height: Math.round(r.height),
      }),
    );
  };

  const onKey = (event) => {
    if (event.key !== "Escape") return;
    block(event);
    stop();
    window.ipc.postMessage(JSON.stringify({ kind: "cancelled" }));
  };

  // Clicks must not reach the page while picking, or they'd follow links and press buttons.
  const listeners = [
    ["mousemove", onMove],
    ["click", onClick],
    ["mousedown", block],
    ["mouseup", block],
    ["keydown", onKey],
  ];

  function start() {
    if (active) return;
    active = true;
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
    if (!active) return;
    active = false;
    box.remove();
    label.remove();
    current = null;
    for (const [type, listener] of listeners) window.removeEventListener(type, listener, true);
    document.documentElement.style.cursor = "";
  }

  window.__barduino = { pick: (on) => (on ? start() : stop()) };
})();
