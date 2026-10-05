// The in-game HUD of game modes (Caveland): health, jetpack, the three pack slots, the crafting popup
// (C) and short messages. It is plain DOM, driven by the wasm client:
//
//   wurfelHud.show(bool)       show or hide the HUD (the client shows it in Caveland worlds)
//   wurfelHud.update(json)     {"health": 0-100, "jetpack": 0-1, "items": ["Wood", ...],
//                               "recipes": [{"index", "name", "can", "ingredients": [{"name", "have"}]}, ...]}
//                              recipes already in menu order (craftable first); `index` is the
//                              server's fixed recipe number, what ("craft", index) takes
//   wurfelHud.toast(text)      a short message that fades out
//   wurfelHud.dialog(json)     {"title", "text", "mode": "simple"|"boolean"|"selection",
//                               "options": [{"id", "label"}], "money"}  an NPC's line, a construction
//                              site, the shop. Answered with wurfelNet.action("choose", id) or
//                              ("cancel", 0).
//   wurfelHud.liftOffer(json)  the cave entry asks "build a lift here?": answered with
//                              wurfelNet.action("confirm_lift" | "decline_lift", 0)
//   wurfelHud.closeDialog()    the server closed it
//   wurfelHud.active           true while the HUD is shown (a Caveland map)
//
// C opens the crafting popup (one recipe at a time): W/S or the arrows choose, Enter, Space or N
// craft the shown recipe and close it; Esc, C, M, a right click or a click beside it close it. C
// inside a server dialog does nothing. The popup closes when a server dialog arrives, the world
// changes (show) or the player dies.
//
// While a dialog is open window.wurfelDialogOpen is true (the client ignores the keys then) and this
// file takes the keys: 1-9 pick the option, Enter or Space the first/yes, Y and N yes and no, Esc
// closes. wurfelNet is installed by the wasm client and sends to the server.
(function () {
  "use strict";

  const HINTS = [
    ["WASD", "walk"], ["Space", "jump / jetpack"], ["F / click", "swing, hold to charge"],
    ["G", "use item"], ["M / right click", "hold, release: throw (long hold: drop)"], ["X", "drop"],
    ["Z / V", "switch item"], ["C", "crafting"], ["R", "talk / build / ride (nearest)"], ["Tab", "players"],
  ];

  let root = null, healthFill = null, healthText = null, jetFill = null, slots = [], toasts = null;
  let moneyText = null, dialogBox = null, dialog = null;
  let craftBox = null, craft = null, lastRecipes = [];

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
    moneyText = el("div", "clhud-money", "");
    bars.append(health, jet, moneyText);

    const pack = el("div", "clhud-pack");
    for (let i = 0; i < 3; i++) {
      const slot = el("div", "clhud-slot");
      slots.push(slot);
      pack.append(slot);
    }

    craftBox = el("div", "clhud-craftmenu");
    craftBox.hidden = true;
    craftBox.addEventListener("mousedown", (e) => { if (e.target === craftBox) closeCraft(); });
    craftBox.addEventListener("contextmenu", (e) => { e.preventDefault(); closeCraft(); });

    const hints = el("div", "clhud-hints");
    for (const [key, what] of HINTS) {
      const row = el("div", "clhud-hint");
      row.append(el("kbd", "", key), el("span", "", what));
      hints.append(row);
    }

    toasts = el("div", "clhud-toasts");
    dialogBox = el("div", "clhud-dialog");
    dialogBox.hidden = true;
    root.append(bars, pack, hints, toasts, dialogBox, craftBox);
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
    lastMoney = Number(state.money) || 0;
    moneyText.textContent = lastMoney > 0 ? "Money " + lastMoney : "";

    const items = Array.isArray(state.items) ? state.items : [];
    slots.forEach((slot, i) => {
      slot.textContent = items[i] || "";
      slot.classList.toggle("clhud-empty", !items[i]);
      slot.classList.toggle("clhud-hand", i === 0);
      slot.title = i === 0 ? "In hand" : "";
    });

    lastRecipes = (Array.isArray(state.recipes) ? state.recipes : []).filter((r) => r && typeof r === "object");
    if (health <= 0) closeCraft();
    else if (craft) renderCraft();
  }

  // ---- crafting popup -----------------------------------------------------------------------

  /** Position of the selected recipe in the menu list; the selection follows the fixed index. */
  function craftPosition() {
    const at = lastRecipes.findIndex((r) => r.index === craft.selectedIndex);
    if (at >= 0) return at;
    // The recipe vanished: stay at the nearest position and pin the selection there.
    const pos = Math.max(0, Math.min(craft.position, lastRecipes.length - 1));
    if (lastRecipes[pos]) craft.selectedIndex = lastRecipes[pos].index;
    return pos;
  }

  function renderCraft() {
    craftBox.textContent = "";
    const panel = el("div", "clhud-craftpanel");
    panel.append(el("div", "clhud-title", "Crafting"));
    if (lastRecipes.length === 0) {
      panel.append(el("div", "clhud-craftempty", "Not enough ingredients."));
    } else {
      const pos = craftPosition();
      craft.position = pos;
      const r = lastRecipes[pos];
      panel.append(el("div", "clhud-craftmore", pos > 0 ? "/\\" : ""));
      const row = el("div", "clhud-craftrow");
      (Array.isArray(r.ingredients) ? r.ingredients : []).forEach((ing, i) => {
        if (i > 0) row.append(el("span", "clhud-craftop", "+"));
        row.append(el("span", ing.have ? "" : "clhud-dim", String(ing.name)));
      });
      row.append(el("span", "clhud-craftop", "="), el("span", r.can ? "clhud-can" : "clhud-dim", String(r.name)));
      panel.append(row);
      panel.append(el("div", "clhud-craftmore", pos < lastRecipes.length - 1 ? "\\/" : ""));
    }
    panel.append(el("div", "clhud-dialog-hint", "W/S choose \u00b7 Enter craft \u00b7 Esc close"));
    craftBox.append(panel);
  }

  function openCraft() {
    if (!craftBox || craft) return;
    craft = { selectedIndex: lastRecipes.length ? lastRecipes[0].index : -1, position: 0 };
    craftBox.hidden = false;
    window.wurfelDialogOpen = true;
    renderCraft();
  }

  function closeCraft() {
    if (!craft) return;
    craft = null;
    if (craftBox) craftBox.hidden = true;
    window.wurfelDialogOpen = !!dialog;
  }

  function moveCraft(delta) {
    if (!lastRecipes.length) return;
    const pos = Math.max(0, Math.min(lastRecipes.length - 1, craftPosition() + delta));
    craft.position = pos;
    craft.selectedIndex = lastRecipes[pos].index;
    renderCraft();
  }

  function confirmCraft() {
    if (lastRecipes.length) send("craft", lastRecipes[craftPosition()].index);
    closeCraft();
  }

  function toast(text) {
    if (!toasts) return;
    const t = el("div", "clhud-toast", String(text));
    toasts.append(t);
    while (toasts.children.length > 4) toasts.firstChild.remove();
    setTimeout(() => t.remove(), 2600);
  }

  let lastMoney = 0;

  function show(on) {
    if (root) root.hidden = !on;
    api.active = !!on;
    closeCraft();
    if (!on) closeDialog();
  }

  // ---- dialogs ------------------------------------------------------------------------------

  function send(name, arg) {
    if (window.wurfelNet && typeof window.wurfelNet.action === "function") window.wurfelNet.action(name, arg | 0);
  }

  /** Show a dialog. `view` = {title, text, buttons: [{key, label, run}], cancel} */
  function openDialog(view) {
    if (!dialogBox) return;
    closeCraft();
    dialog = view;
    dialogBox.textContent = "";
    dialogBox.append(el("div", "clhud-dialog-title", view.title || ""));
    if (view.text) dialogBox.append(el("div", "clhud-dialog-text", view.text));
    if (view.money !== undefined) dialogBox.append(el("div", "clhud-dialog-money", "Money " + view.money));
    const buttons = el("div", "clhud-dialog-buttons");
    view.buttons.forEach((b, i) => {
      const button = el("button", "clhud-dialog-button");
      button.type = "button";
      button.append(el("kbd", "", String(i + 1)), el("span", "", b.label));
      button.addEventListener("click", () => b.run());
      buttons.append(button);
    });
    dialogBox.append(buttons, el("div", "clhud-dialog-hint", "Esc to close"));
    dialogBox.hidden = false;
    window.wurfelDialogOpen = true;
  }

  function closeDialog() {
    dialog = null;
    if (dialogBox) dialogBox.hidden = true;
    window.wurfelDialogOpen = !!craft;
  }

  function parse(json) {
    try { return typeof json === "string" ? JSON.parse(json) : json; } catch (e) { return null; }
  }

  function showServerDialog(json) {
    const d = parse(json);
    if (!d) return;
    const mode = d.mode;
    let buttons;
    if (mode === "selection") {
      buttons = (Array.isArray(d.options) ? d.options : []).map((o) => ({ label: String(o.label), run: () => send("choose", o.id) }));
    } else if (mode === "boolean") {
      buttons = [{ label: "Yes", run: () => send("choose", 1) }, { label: "No", run: () => send("choose", 0) }];
    } else {
      buttons = [{ label: "Next", run: () => send("choose", 1) }];
    }
    openDialog({
      title: d.title, text: d.text, buttons, money: mode === "selection" ? Number(d.money) || 0 : undefined,
      cancel: () => { send("cancel", 0); closeDialog(); },
      // Enter confirms a line or a question; a shop list needs a deliberate number.
      yes: mode === "selection" ? -1 : 0, no: mode === "boolean" ? 1 : -1,
    });
  }

  function showLiftOffer() {
    openDialog({
      title: "Lift", text: "Build a lift construction site here?",
      buttons: [
        { label: "Build", run: () => { send("confirm_lift", 0); closeDialog(); } },
        { label: "Not now", run: () => { send("decline_lift", 0); closeDialog(); } },
      ],
      cancel: () => { send("decline_lift", 0); closeDialog(); },
      yes: 0, no: 1,
    });
  }

  // Capture phase: C toggles the crafting popup, and the popup takes the keys while it is open.
  window.addEventListener("keydown", (e) => {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const t = e.target;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
    const key = e.key;
    if (craft) {
      if (key === "ArrowUp" || key === "w" || key === "W") moveCraft(-1);
      else if (key === "ArrowDown" || key === "s" || key === "S") moveCraft(1);
      else if (key === "Enter" || key === " " || key === "n" || key === "N") confirmCraft();
      else if (key === "Escape" || key === "c" || key === "C" || key === "m" || key === "M") closeCraft();
      e.preventDefault();
      e.stopImmediatePropagation();
    } else if ((key === "c" || key === "C") && api.active && !window.wurfelMenuOpen && !window.wurfelConsoleOpen) {
      // Swallowed always, so the game never sees C; inside another dialog it does nothing.
      if (!dialog) openCraft();
      e.preventDefault();
      e.stopImmediatePropagation();
    }
  }, true);

  // Capture phase: runs before the game's and the menu's key handlers.
  window.addEventListener("keydown", (e) => {
    if (!dialog || e.ctrlKey || e.metaKey || e.altKey) return;
    const key = e.key;
    let handled = true;
    if (key === "Escape") dialog.cancel();
    else if (/^[1-9]$/.test(key)) { const b = dialog.buttons[Number(key) - 1]; if (b) b.run(); }
    else if ((key === "Enter" || key === " ") && dialog.yes >= 0) dialog.buttons[dialog.yes].run();
    else if ((key === "y" || key === "Y") && dialog.yes >= 0) dialog.buttons[dialog.yes].run();
    else if ((key === "n" || key === "N") && dialog.no >= 0) dialog.buttons[dialog.no].run();
    else handled = false;
    // Everything else is swallowed too, so no key reaches the game behind the dialog (walking is
    // already off because wurfelDialogOpen is set).
    e.preventDefault();
    e.stopImmediatePropagation();
    void handled;
  }, true);

  const api = { show, update, toast, dialog: showServerDialog, liftOffer: showLiftOffer, closeDialog, active: false };
  window.wurfelHud = api;
  if (document.body) build();
  else document.addEventListener("DOMContentLoaded", build);
})();
