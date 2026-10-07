//! The engine's console (`wurfel_sim::console`) in the page: `console-host.js` hands every line
//! to [`execute`] and every Tab to [`suggest`]. Client commands (`fullscreen`, `screenshake`,
//! `tp`, cvars of this browser...) run here; the rest goes to the server as a
//! [`ClientMsg::Command`], and so do the game mode's own commands (Caveland's `give`...).

use glam::Vec3;
use serde_json::json;
use wurfel_sim::console::{command_name, normalize_line, CVarTarget, Console, ConsoleHost, ExecResult, OutputLine, Side};
use wurfel_sim::cvar::CVarSystem;
use wurfel_sim::grid::to_iso;

use super::{call_js, set_editor, State};

/// Where this browser keeps its cvars (Java: `engine.wecvar` next to the jar).
const CVARS_KEY: &str = "wurfel.cvars";

/// The console's part of the client state.
pub(super) struct ClientConsole {
    console: Console,
    /// The root cvars this browser owns (graphics, sound, keys...); the server owns the rest.
    cvars: CVarSystem,
    /// `tp`: the camera looks at this point until we walk.
    pub(super) camera_hold: Option<Vec3>,
}

impl ClientConsole {
    pub(super) fn new() -> Self {
        let mut cvars = CVarSystem::root();
        if let Some(text) = storage().and_then(|s| s.get_item(CVARS_KEY).ok().flatten()) {
            cvars.load_str(&text);
        }
        ClientConsole { console: Console::new(Side::Client), cvars, camera_hold: None }
    }
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// The game as the console sees it on this side of the connection.
struct ClientHost<'a> {
    s: &'a mut State,
}

impl ConsoleHost for ClientHost<'_> {
    fn cvars(&mut self, target: &CVarTarget) -> Option<&mut CVarSystem> {
        match target {
            CVarTarget::Root => Some(&mut self.s.console.cvars),
            // Map and save cvars live on the server: lines about them are forwarded.
            _ => None,
        }
    }

    fn toggle_fullscreen(&mut self) -> Result<(), String> {
        let document = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
        if document.fullscreen_element().is_some() {
            document.exit_fullscreen();
            return Ok(());
        }
        let root = document.document_element().ok_or("no document")?;
        root.request_fullscreen().map_err(|_| "Fullscreen is not available here.".to_string())
    }

    fn reload_shaders(&mut self) -> Result<(), String> {
        Err("the shaders are built into the page: reload the page to reload them".into())
    }

    fn toggle_light_engine(&mut self) -> Result<(), String> {
        let lighting = &mut self.s.lighting;
        lighting.enabled = !lighting.enabled;
        let _ = self.s.console.cvars.set_bool("enableLightEngine", lighting.enabled);
        Ok(())
    }

    fn show_menu(&mut self) -> Result<(), String> {
        call_js("wurfelMenu", "openMenu", &"pause".into());
        Ok(())
    }

    fn start_editor(&mut self) -> Result<(), String> {
        let on = !self.s.editor.active();
        set_editor(self.s, on).map_err(str::to_string)
    }

    fn exit(&mut self) -> Result<(), String> {
        call_js("wurfelMenu", "leave", &wasm_bindgen::JsValue::UNDEFINED);
        Ok(())
    }

    fn camera_focus(&mut self, x: i32, y: i32) -> Result<(), String> {
        // Java puts the camera's centre on the column at height 0; the ground is more useful.
        let z = wurfel_sim::entity::physics::ground_height(&self.s.world, x, y);
        let (gx, gy) = to_iso(x, y);
        self.s.console.camera_hold = Some(Vec3::new(gx, gy, z));
        Ok(())
    }

    fn screenshake(&mut self, camera: i32, amplitude: f32, millis: f32) -> Result<(), String> {
        if camera != 0 {
            return Err("Camera ID out of range".into());
        }
        self.s.shake.add(amplitude, millis);
        Ok(())
    }
}

/// The game mode's commands, which the server answers (`help` lists them too).
fn mode_commands(s: &State) -> &'static [(&'static str, &'static str)] {
    s.mode.as_ref().map_or(&[], |m| m.commands())
}

/// The server answered a forwarded line: hand it to the page's console.
pub(super) fn reply(lines: &[OutputLine]) {
    call_js("wurfelConsoleHost", "reply", &wasm_bindgen::JsValue::from_str(&json!({"lines": lines}).to_string()));
}

/// Run a console line. Returns JSON: `{"lines": [...], "clear": bool}` when it is done, or
/// `{"forwarded": true, "lines": [...]}` when it went to the server, whose answer arrives as a
/// [`ServerMsg::ConsoleReply`](wurfel_sim::protocol::ServerMsg::ConsoleReply) (see [`reply`]).
pub(super) fn execute(s: &mut State, line: &str) -> String {
    let line = normalize_line(line);
    let name = command_name(line);
    let echo = OutputLine::echo(format!("{}{}", s.console.console.prompt(), line));
    if mode_commands(s).iter().any(|(n, _)| *n == name) {
        return forward(s, line, String::new(), vec![echo]);
    }
    let result = {
        // The console and its host both live in `State`: take the console out while it runs.
        let mut console = std::mem::replace(&mut s.console.console, Console::new(Side::Client));
        let result = console.execute(line, &mut ClientHost { s });
        s.console.console = console;
        result
    };
    if let Some(storage) = storage() {
        let _ = storage.set_item(CVARS_KEY, &s.console.cvars.save_string());
    }
    match result {
        ExecResult::Done(mut output) => {
            if name == "help" {
                output.lines.extend(mode_commands(s).iter().map(|(n, manual)| OutputLine::info(format!("{n:<14}{}", manual.lines().next().unwrap_or("")))));
            }
            json!({"lines": output.lines, "clear": output.clear}).to_string()
        }
        ExecResult::Forward { line, path, echo } => forward(s, &line, path, echo),
    }
}

fn forward(s: &mut State, line: &str, path: String, echo: Vec<OutputLine>) -> String {
    if !s.connected {
        let mut lines = echo;
        lines.push(OutputLine::error("Not connected to a game."));
        return json!({"lines": lines}).to_string();
    }
    super::send(s, &wurfel_sim::protocol::ClientMsg::Command { line: line.to_string(), path });
    json!({"forwarded": true, "lines": echo}).to_string()
}

/// Tab completion: whole replacement lines for the text before the caret.
pub(super) fn suggest(s: &mut State, prefix: &str) -> Vec<String> {
    let console = std::mem::replace(&mut s.console.console, Console::new(Side::Client));
    let mut lines = console.suggest_lines(prefix, &mut ClientHost { s });
    s.console.console = console;
    if !prefix.contains(char::is_whitespace) {
        lines.extend(mode_commands(s).iter().map(|(n, _)| n.to_string()).filter(|n| n.starts_with(prefix)));
        lines.sort();
        lines.dedup();
    }
    lines
}

pub(super) fn prompt(s: &State) -> String {
    s.console.console.prompt()
}

/// `?auth=<token>` in the page address logs in as administrator on every join (`auth <token>`),
/// handy for development. The answer is printed to the console.
pub(super) fn auth_from_url(s: &mut State) {
    let search = web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default();
    let token = search.trim_start_matches('?').split('&').find_map(|part| part.strip_prefix("auth="));
    if let Some(token) = token.and_then(|t| js_sys::decode_uri_component(t).ok()).map(String::from).filter(|t| !t.is_empty()) {
        super::send(s, &wurfel_sim::protocol::ClientMsg::Command { line: format!("auth {token}"), path: String::new() });
    }
}

/// The session secret this browser got from the server at `server_url` (see the server's
/// `users.rs`): it brings our user back after a reload. Empty if there is none yet.
pub(super) fn stored_session(server_url: &str) -> String {
    storage().and_then(|s| s.get_item(&session_key(server_url)).ok().flatten()).unwrap_or_default()
}

pub(super) fn store_session(server_url: &str, secret: &str) {
    if let Some(storage) = storage() {
        let _ = storage.set_item(&session_key(server_url), secret);
    }
}

/// One secret per server: each has its own users.
fn session_key(server_url: &str) -> String {
    format!("wurfel.session.{server_url}")
}
