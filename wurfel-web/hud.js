// The in-game HUD of game modes (Caveland): health, jetpack, the three pack slots, the crafting list
// and short messages. It is plain DOM, driven by the wasm client:
//
//   wurfelHud.show(bool)       show or hide the HUD (the client shows it in Caveland worlds)
//   wurfelHud.update(json)     {"health": 0-100, "jetpack": 0-1, "items": ["Wood", ...],
//                               "recipes": [["Torch", true], ...]}   recipes in craft-key order
//   wurfelHud.toast(text)      a short message that fades out
//
// Nothing in here talks to the server: the keys are handled by the client.
(function () {
  "use strict";

  const HINTS = [
    ["WASD", "walk"], ["Space", "jump / jetpack"], ["F / click", "swing, hold to charge"],
    ["G", "use item"], ["R", "use machine"], ["C", "hold + release: throw"], ["X", "drop"],
    ["Z / V", "switch item"], ["1-9", "craft"],
  ];

  let root = null, healthFill = null, healthText = null, jetFill = null, slots = [], recipeList = null, toasts = null;

  function el(tag, className, text) {
    const e = document.createElement(tag);
    if (className) e.className = className;
    if (text !== undefined) e.textContent = text;
    return e;
  }

  function build() {
    root = el("div", "clhud");
    root.hidden = true;

    const bars = el("div", "clhud-bars");
    const health = el("div", "clhud-bar clhud-health");
    healthFill = el("div", "clhud-fill");
    healthText = el("span", "clhud-bar-text", "100");
    health.append(healthFill, healthText);
    const jet = el("div", "clhud-bar clhud-jet");
    jetFill = el("div", "clhud-fill");
    jet.append(jetFill);
    bars.append(health, jet);

    const pack = el("div", "clhud-pack");
    for (let i = 0; i < 3; i++) {
      const slot = el("div", "clhud-slot");
      slots.push(slot);
      pack.append(slot);
    }

    const craft = el("div", "clhud-craft");
    craft.append(el("div", "clhud-title", "Crafting"));
    recipeList = el("ol", "clhud-recipes");
    craft.append(recipeList);

    const hints = el("div", "clhud-hints");
    for (const [key, what] of HINTS) {
      const row = el("div", "clhud-hint");
      row.append(el("kbd", "", key), el("span", "", what));
      hints.append(row);
    }

    toasts = el("div", "clhud-toasts");
    root.append(bars, pack, craft, hints, toasts);
    document.body.append(root);
  }

  function update(json) {
    let state;
    try { state = typeof json === "string" ? JSON.parse(json) : json; } catch (e) { return; }
    if (!root || !state) return;
    const health = Math.max(0, Math.min(100, Number(state.health) || 0));
    healthFill.style.width = health + "%";
    healthText.textContent = String(Math.round(health));
    root.classList.toggle("clhud-hurt", health < 30);
    jetFill.style.width = Math.max(0, Math.min(1, Number(state.jetpack) || 0)) * 100 + "%";

    const items = Array.isArray(state.items) ? state.items : [];
    slots.forEach((slot, i) => {
      slot.textContent = items[i] || "";
      slot.classList.toggle("clhud-empty", !items[i]);
      slot.classList.toggle("clhud-hand", i === 0);
      slot.title = i === 0 ? "In hand" : "";
    });

    recipeList.textContent = "";
    (Array.isArray(state.recipes) ? state.recipes : []).slice(0, 9).forEach(([name, can], i) => {
      const li = el("li", can ? "clhud-can" : "clhud-cannot");
      li.append(el("kbd", "", String(i + 1)), el("span", "", name));
      recipeList.append(li);
    });
  }

  function toast(text) {
    if (!toasts) return;
    const t = el("div", "clhud-toast", String(text));
    toasts.append(t);
    while (toasts.children.length > 4) toasts.firstChild.remove();
    setTimeout(() => t.remove(), 2600);
  }

  function show(on) {
    if (root) root.hidden = !on;
  }

  window.wurfelHud = { show, update, toast };
  if (document.body) build();
  else document.addEventListener("DOMContentLoaded", build);
})();
