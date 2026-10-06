// The map editor's toolbar and EDITOR indicator (plain DOM). The editor itself (mode, tools, what a
// click does) lives in the wasm client (src/editor.rs); this file only shows its state and forwards
// toolbar clicks:
//
//   wurfelEditor.update(json)  {"active", "tool", "selected", "tools": [...], "blocks": [...], "cursor", "undo", "redo"}
//                              pushed by the client whenever one of them changes
//   wurfelEditor.active        true while the editor is on
//   wurfelEditor.toggle(mode)  "on" | "off" | anything else toggles; returns a message when it is
//                              refused (offline, Caveland), else "". Used by the console `editor` command.
//
// Calls into the client: wurfelNet.editor(mode), wurfelNet.editorTool(name), wurfelNet.editorBlock(index),
// wurfelNet.editorHistory(undo). F2 toggles the editor (handled by the client). Mouse use: left button
// = the selected tool (hold to paint), right button = erase, middle button or Alt + left = pick,
// number keys = block, Ctrl/Cmd+Z = undo, plus Shift = redo.
(function () {
  "use strict";

  const TOOLS = { draw: "Draw", bucket: "Bucket", replace: "Replace", erase: "Erase", pick: "Pick" };
  const TOOL_HINTS = {
    draw: "Place the block next to the one you click",
    bucket: "Press, drag and release to fill the rectangle",
    replace: "Overwrite the block you click",
    erase: "Remove the block you click",
    pick: "Take the kind of the block you click",
  };
  const BLOCK_COLORS = { stone: "#8a8f98", dirt: "#8b5a2b", grass: "#5aa83c", sand: "#e0cf8a" };

  let root = null, toolRow = null, blockRow = null, cursorLine = null, hintLine = null, undoButton = null, redoButton = null;
  let state = { active: false, tool: "draw", selected: 0, tools: [], blocks: [], cursor: "", undo: false, redo: false };
  let built = { tools: "", blocks: "" };

  function el(tag, className, text) {
    const e = document.createElement(tag);
    if (className) e.className = className;
    if (text !== undefined) e.textContent = text;
    return e;
  }

  function net(method, arg) {
    const n = window.wurfelNet;
    if (n && typeof n[method] === "function") return n[method](arg);
    return "";
  }

  function build() {
    root = el("div", "ed");
    root.hidden = true;
    // The toolbar sits over the game: clicks on it must not reach the game's mouse handlers.
    // Keep the keyboard for the game: a focused button would also react to Space (jump).
    root.addEventListener("click", (e) => { if (e.target && e.target.blur) e.target.blur(); });
    for (const type of ["mousedown", "mouseup", "wheel"]) root.addEventListener(type, (e) => e.stopPropagation());

    const badge = el("div", "ed-badge");
    badge.append(el("span", "ed-badge-text", "EDITOR"));
    const leave = el("button", "ed-leave", "Leave (F2)");
    leave.type = "button";
    leave.addEventListener("click", () => net("editor", "off"));
    badge.append(leave);

    const bar = el("div", "ed-bar");
    bar.setAttribute("role", "toolbar");
    bar.setAttribute("aria-label", "Editor tools");
    toolRow = el("div", "ed-row");
    blockRow = el("div", "ed-row");
    const historyRow = el("div", "ed-row");
    undoButton = el("button", "ed-tool", "Undo");
    redoButton = el("button", "ed-tool", "Redo");
    undoButton.title = "Ctrl/Cmd+Z";
    redoButton.title = "Ctrl/Cmd+Shift+Z";
    for (const [button, undo] of [[undoButton, true], [redoButton, false]]) {
      button.type = "button";
      button.addEventListener("click", () => net("editorHistory", undo));
      historyRow.append(button);
    }
    hintLine = el("div", "ed-hint");
    bar.append(toolRow, blockRow, historyRow, hintLine);

    cursorLine = el("div", "ed-cursor");
    root.append(badge, bar, cursorLine);
    document.body.append(root);
  }

  function render() {
    if (!root) build();
    root.hidden = !state.active;
    if (!state.active) return;

    const toolKey = state.tools.join();
    if (built.tools !== toolKey) {
      built.tools = toolKey;
      toolRow.replaceChildren(...state.tools.map((name) => {
        const b = el("button", "ed-tool", TOOLS[name] || name);
        b.type = "button";
        b.dataset.tool = name;
        b.addEventListener("click", () => net("editorTool", name));
        return b;
      }));
    }
    const blockKey = state.blocks.join();
    if (built.blocks !== blockKey) {
      built.blocks = blockKey;
      blockRow.replaceChildren(...state.blocks.map((name, i) => {
        const b = el("button", "ed-block");
        b.type = "button";
        b.title = name;
        b.dataset.index = String(i);
        const swatch = el("span", "ed-swatch");
        swatch.style.background = BLOCK_COLORS[name] || "#888";
        b.append(el("kbd", "", String(i + 1)), swatch, el("span", "", name));
        b.addEventListener("click", () => net("editorBlock", i));
        return b;
      }));
    }
    for (const b of toolRow.children) b.classList.toggle("ed-on", b.dataset.tool === state.tool);
    for (const b of blockRow.children) b.classList.toggle("ed-on", Number(b.dataset.index) === state.selected);
    undoButton.disabled = !state.undo;
    redoButton.disabled = !state.redo;
    hintLine.textContent = `Left: ${TOOL_HINTS[state.tool] || state.tool} · Right: erase · Middle or Alt: pick`;
    cursorLine.textContent = state.cursor || "";
    cursorLine.hidden = !state.cursor;
  }

  window.wurfelEditor = {
    get active() { return state.active; },
    update(json) {
      try { state = Object.assign(state, typeof json === "string" ? JSON.parse(json) : json); } catch (e) { return; }
      render();
    },
    toggle(mode) { return net("editor", mode === "on" || mode === "off" ? mode : "toggle") || ""; },
  };

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", build);
  else build();
})();
