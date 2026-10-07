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
// C opens the crafting popup (a list of every recipe, craftable ones first): click one, or W/S or the
// arrows choose and Enter, Space or N craft it (when the pack has the ingredients) and close the popup; Esc, C, M, a right click or a click beside it close it. C
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
    ["Z / V", "switch item"], ["Wheel", "zoom"], ["C", "crafting"], ["R", "talk / build / ride (nearest)"], ["Tab", "players"],
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

  // Item sprites: the entity ids of the collectibles in the sprite atlas (`e<id>-0`), as
  // `entity_art` of src/sprites.rs has them. Items without one keep the coloured letter tile.
  const ITEM_SPRITE = {
    Rails: 16, Wood: 46, Explosives: 47, Gunpowder: 56, Ironore: 48, Coal: 49, Cristall: 50, Sulfur: 51,
    Stone: 52, Toolkit: 53, Torch: 54, Iron: 55, Powercable: 57, DropSpaceFlagConstructionKit: 23,
  };
  const ATLAS_URL = "assets/sprites/sprites.atlas";
  let atlas = null, atlasLoading = false;

  /** {regions: {name: {page, x, y, w, h}}, pages: [file]} from a libGDX .atlas text. */
  function parseAtlas(text) {
    const regions = {}, pages = [];
    let cur = null;
    for (const raw of text.split("\n")) {
      const line = raw.replace(/\r$/, "");
      if (!line.trim()) continue;
      if (!/^\s/.test(line)) {
        if (/\.png$/.test(line)) { pages.push(line.trim()); cur = null; }
        else if (!line.includes(":") && pages.length) { cur = { page: pages.length - 1, x: 0, y: 0, w: 0, h: 0 }; regions[line.trim()] = cur; }
      } else if (cur) {
        const m = /^\s+(xy|size):\s*(\d+),\s*(\d+)/.exec(line);
        if (m && m[1] === "xy") { cur.x = +m[2]; cur.y = +m[3]; }
        else if (m) { cur.w = +m[2]; cur.h = +m[3]; }
      }
    }
    return { regions, pages };
  }

  function loadAtlas() {
    if (atlas || atlasLoading) return;
    atlasLoading = true;
    fetch(ATLAS_URL).then((r) => (r.ok ? r.text() : Promise.reject(r.status)))
      .then((text) => { atlas = parseAtlas(text); if (craft) renderCraft(); })
      .catch(() => { atlasLoading = false; });
  }

  /** The item's sprite as an element of `size` pixels, or null when it has none (yet). */
  function sprite(name, size) {
    const id = ITEM_SPRITE[name];
    const region = atlas && id !== undefined ? atlas.regions["e" + id + "-0"] : null;
    if (!region || !region.w || !region.h) return null;
    const scale = size / Math.max(region.w, region.h);
    const img = el("span", "clhud-sprite");
    img.style.width = Math.round(region.w * scale) + "px";
    img.style.height = Math.round(region.h * scale) + "px";
    img.style.backgroundImage = "url(assets/sprites/" + atlas.pages[region.page] + ")";
    img.style.backgroundSize = Math.round(2048 * scale) + "px auto";
    img.style.backgroundPosition = "-" + Math.round(region.x * scale) + "px -" + Math.round(region.y * scale) + "px";
    return img;
  }

  /** A stable colour per name, for the items without a sprite. */
  function tileColor(name) {
    let h = 0;
    for (const ch of String(name)) h = (h * 31 + ch.charCodeAt(0)) % 360;
    return "hsl(" + h + ", 45%, 42%)";
  }

  function tile(name, dim, size) {
    const art = sprite(name, size - 4);
    const t = el("span", "clhud-tile" + (dim ? " clhud-dim" : ""));
    if (art) t.append(art);
    else { t.textContent = String(name).charAt(0).toUpperCase(); t.style.background = tileColor(name); }
    t.title = String(name);
    return t;
  }

  function renderCraft() {
    const keepScroll = craftBox.querySelector(".clhud-craftlist");
    const scroll = keepScroll ? keepScroll.scrollTop : 0;
    craftBox.textContent = "";
    const panel = el("div", "clhud-craftpanel");
    panel.append(el("div", "clhud-title", "Crafting"));
    if (lastRecipes.length === 0) {
      panel.append(el("div", "clhud-craftempty", "No recipes known."));
    } else {
      const pos = craftPosition();
      craft.position = pos;
      const list = el("div", "clhud-craftlist");
      let selected = null;
      lastRecipes.forEach((r, i) => {
        const card = el("div", "clhud-card" + (r.can ? " clhud-cancraft" : " clhud-cantcraft") + (i === pos ? " clhud-selected" : ""));
        card.append(tile(r.name, !r.can, 36));
        const body = el("div", "clhud-cardbody");
        body.append(el("div", "clhud-cardname", String(r.name)));
        const chips = el("div", "clhud-chips");
        (Array.isArray(r.ingredients) ? r.ingredients : []).forEach((ing) => {
          const chip = el("span", "clhud-chip" + (ing.have ? " clhud-have" : " clhud-missing"));
          chip.append(tile(ing.name, !ing.have, 22), el("span", "", String(ing.name)));
          chips.append(chip);
        });
        body.append(chips);
        card.append(body);
        card.addEventListener("mouseenter", () => {
          if (craft && craft.selectedIndex !== r.index) { craft.selectedIndex = r.index; renderCraft(); }
        });
        card.addEventListener("click", () => {
          craft.selectedIndex = r.index;
          if (r.can) confirmCraft(); else renderCraft();
        });
        if (i === pos) selected = card;
        list.append(card);
      });
      panel.append(list);
      craftBox.append(panel);
      list.scrollTop = scroll; // only works once the list is in the DOM
      if (selected) {
        const top = selected.offsetTop, bottom = top + selected.offsetHeight;
        if (top < list.scrollTop) list.scrollTop = top;
        else if (bottom > list.scrollTop + list.clientHeight) list.scrollTop = bottom - list.clientHeight;
      }
      panel.append(el("div", "clhud-dialog-hint", "Click or W/S + Enter to craft \u00b7 Esc close"));
      return;
    }
    panel.append(el("div", "clhud-dialog-hint", "Esc close"));
    craftBox.append(panel);
  }

  function openCraft() {
    if (!craftBox || craft) return;
    loadAtlas();
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
    const r = lastRecipes[craftPosition()];
    if (r && r.can) send("craft", r.index);
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
