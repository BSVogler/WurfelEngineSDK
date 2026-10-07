//! The command console: port of the Java engine's `core.console` package, without any rendering.
//!
//! The Java console is a text field and a log inside the libGDX stage. Here it is split in two: this
//! module parses and executes lines and returns [`OutputLine`]s; the HTML overlay in
//! `wurfel-web/console.js` shows them.
//!
//! What carries over from Java:
//!
//! * The command set (`benchmark cd credits editor exit fillwithair fullscreen killall le loadmap ls
//!   man menu printmap reloadshaders save screenshake tp`, plus `clear`, which Java has as a class
//!   but never registers) with the Java manual texts for `man`.
//! * Resolution order: a command first; if the first word is no command it is tried as a cvar
//!   (`name` prints `cvar name has value x`, `name value` sets it); otherwise
//!   `"<line>: command not found"`. A command that fails is followed by `Failed executing command.`
//! * `cd` / `ls` navigate a path: nothing is the root cvar system, `map` is that map's cvars and
//!   `map:slot` the cvars of one save slot. The prompt is `<path> $ `.
//! * `lastConsoleCommand` is updated on every command, history with Up/Down, Tab completion.
//! * A number that does not parse is reported as `Command crashed: For input string: "x"` like the
//!   Java `NumberFormatException` handler (and counts as handled, as there).
//!
//! What is new or different:
//!
//! * **Where a command runs.** Some commands only make sense on the client (`clear`, `fullscreen`,
//!   `screenshake`...), some change the shared world and must run on the server (`killall`, `save`,
//!   `fillwithair`...). A client-side [`Console`] answers client commands itself and returns
//!   [`ExecResult::Forward`] for the rest; the server runs those with
//!   [`Console::execute_forwarded`]. Cvars follow the same split ([`cvar_scope`]).
//! * **Permissions.** World-changing commands and setting server cvars need admin rights
//!   ([`ConsoleHost::is_admin`], granted by `auth <token>`); reading is open to everybody.
//! * Quoting (`"my world"`), `help`, `set name value`, a server-side `teleport x y` (Java's `tp`
//!   moves the camera, which stays a client command), `printmap` prints a slice of the map (the Java
//!   version is commented out), and `exit` reports success (Java returned `false` on purpose).
//! * Setting a cvar goes through [`CVarSystem::set`], not [`CVarSystem::execute`]: `execute` looks
//!   the value up with `line.find(value)`, which finds it inside the cvar's own name when they
//!   overlap (`description desc`).

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::cvar::{CVarSystem, Value};

/// Version shown by `credits` (Java: `WE.VERSION`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const FAILED: &str = "Failed executing command.";
const HISTORY_LIMIT: usize = 100;
/// Largest `printmap` slice, so one line cannot flood the output.
const PRINTMAP_MAX: (i32, i32) = (80, 60);

// ------------------------------------------------------------------------------------- output

/// Written in lowercase on the wire (`"info"`), which is also what the HTML console expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Info,
    Warn,
    Error,
    /// The line the user typed, shown with its prompt.
    Echo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputLine {
    pub level: Level,
    pub text: String,
}

impl OutputLine {
    fn new(level: Level, text: impl Into<String>) -> Self {
        OutputLine { level, text: text.into() }
    }
    pub fn info(text: impl Into<String>) -> Self {
        Self::new(Level::Info, text)
    }
    pub fn warn(text: impl Into<String>) -> Self {
        Self::new(Level::Warn, text)
    }
    pub fn error(text: impl Into<String>) -> Self {
        Self::new(Level::Error, text)
    }
    pub fn echo(text: impl Into<String>) -> Self {
        Self::new(Level::Echo, text)
    }
}

/// What running a line produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    pub lines: Vec<OutputLine>,
    /// The `clear` command: wipe the log before showing `lines`.
    pub clear: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecResult {
    /// Finished locally.
    Done(Output),
    /// Has to run on the server. Send `line` and `path` to it, show `echo` now, and show whatever
    /// [`Console::execute_forwarded`] returns there when the answer arrives.
    Forward { line: String, path: String, echo: Vec<OutputLine> },
}

/// A line as typed, without surrounding space and the `/` or `:` a chat-style console may put in
/// front (`/give Torch` is `give Torch`).
pub fn normalize_line(line: &str) -> &str {
    line.trim().trim_start_matches(['/', ':'])
}

/// The command word of a (normalized) line, in lowercase: `Give Torch` is `give`.
pub fn command_name(line: &str) -> String {
    line.split_whitespace().next().unwrap_or("").to_lowercase()
}

// ------------------------------------------------------------------------------------ commands

/// Where a command (or cvar) is executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Client,
    Server,
}

#[derive(Clone, Copy)]
pub struct CommandInfo {
    pub name: &'static str,
    pub manual: &'static str,
    pub scope: Scope,
    /// Needs [`ConsoleHost::is_admin`].
    pub admin_only: bool,
    /// Runs the command; returns whether it succeeded (a failure adds `Failed executing command.`).
    run: Handler,
}

impl std::fmt::Debug for CommandInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandInfo").field("name", &self.name).field("scope", &self.scope).field("admin_only", &self.admin_only).finish()
    }
}

/// What a command's handler gets: its arguments, the console (`cd` changes its path), the game.
pub struct Call<'a> {
    console: &'a mut Console,
    args: &'a [String],
    path: &'a str,
    host: &'a mut dyn ConsoleHost,
    out: &'a mut Output,
}

type Handler = fn(&mut Call<'_>) -> Success;

const fn cmd(name: &'static str, scope: Scope, admin_only: bool, manual: &'static str, run: Handler) -> CommandInfo {
    CommandInfo { name, manual, scope, admin_only, run }
}

/// All commands, in the order `help` lists them (alphabetical). Manuals are the Java texts.
pub const COMMANDS: &[CommandInfo] = &[
    cmd("auth", Scope::Server, false, "log in as administrator: auth <token>", run_auth),
    cmd("benchmark", Scope::Server, true, "spawns a benchmark ball", |c| report(c.out, c.host.spawn_benchmark_ball())),
    cmd("cd", Scope::Client, false, "change the directory", |c| c.console.change_directory(c.args, c.host, c.out)),
    cmd("clear", Scope::Client, false, "clear the content of the console", |c| {
        c.out.clear = true;
        true
    }),
    cmd("credits", Scope::Client, false, "outputs the credits in the console", run_credits),
    cmd("editor", Scope::Client, false, "loads the editor", |c| report(c.out, c.host.start_editor())),
    // The Java command returns false on purpose ("hey, you're getting a response"); here the host
    // decides what leaving means and a success is a success.
    cmd("exit", Scope::Client, false, "exits the game", |c| report(c.out, c.host.exit())),
    cmd("fillwithair", Scope::Server, true, "fills chunk <x> <y> with air ", run_fill_with_air),
    cmd("fullscreen", Scope::Client, false, "toggles the fullscreen", |c| report(c.out, c.host.toggle_fullscreen())),
    cmd("help", Scope::Client, false, "lists all commands", run_help),
    cmd("killall", Scope::Server, true, "disposes every entity on the map", |c| {
        let killed = c.host.kill_all_entities().map(|n| format!("disposed {n} entities"));
        report_message(c.out, killed)
    }),
    cmd("le", Scope::Client, false, "toggles the light engine", |c| report(c.out, c.host.toggle_light_engine())),
    cmd("loadmap", Scope::Server, true, "tries to load a map at a new save slot", |c| {
        let Some(name) = c.args.first().filter(|n| !n.is_empty()) else { return false };
        let loaded = c.host.load_map(name);
        report_message(c.out, loaded)
    }),
    cmd("ls", Scope::Client, false, "shows the content of the directory.", run_ls),
    cmd("man", Scope::Client, false, "outputs the manual entry for this command", run_man),
    cmd("menu", Scope::Client, false, "goes to the main menu", |c| report(c.out, c.host.show_menu())),
    cmd(
        "printmap",
        Scope::Server,
        false,
        "prints a slice of the map in the console. Parameters: [x] [y] [z] [width] [height]",
        run_printmap,
    ),
    cmd("reloadshaders", Scope::Client, false, "reloads the shaders", |c| report(c.out, c.host.reload_shaders())),
    cmd("save", Scope::Server, true, "saves the currently loaded map in the currenty active save slot", |c| {
        let saved = c.host.save();
        report_message(c.out, saved)
    }),
    cmd(
        "screenshake",
        Scope::Client,
        false,
        "Shakes the screen. works only if in the game. Parameters: [cameraID] [amplitude] [time]",
        run_screenshake,
    ),
    cmd("teleport", Scope::Server, true, "moves your player to a block column.\nParameters: [x] [y]", |c| {
        column_command(c, |host, x, y| host.teleport_player(x, y))
    }),
    cmd("tp", Scope::Client, false, "set the focus of the camera.\nParameters: [x game world][y game world]", |c| {
        column_command(c, |host, x, y| host.camera_focus(x, y))
    }),
];

// ------------------------------------------------------------------------------- the handlers

/// An error from the host is printed; success is quiet.
fn report(out: &mut Output, result: Result<(), String>) -> Success {
    report_message(out, result.map(|()| String::new()))
}

/// Like [`report`], but a success prints its message (if any).
fn report_message(out: &mut Output, result: Result<String, String>) -> Success {
    match result {
        Ok(message) => {
            if !message.is_empty() {
                out.lines.push(OutputLine::info(message));
            }
            true
        }
        Err(e) => {
            out.lines.push(OutputLine::error(e));
            false
        }
    }
}

/// Java parses with `Integer.valueOf` and answers a bad number with a crash message (which counts
/// as handled: the caller returns `true`).
fn int(out: &mut Output, text: &str) -> Option<i32> {
    text.parse().map_err(|_| crashed(out, text)).ok()
}

fn float(out: &mut Output, text: &str) -> Option<f32> {
    text.parse().map_err(|_| crashed(out, text)).ok()
}

fn crashed(out: &mut Output, text: &str) {
    out.lines.push(OutputLine::error(format!("Command crashed: For input string: \"{text}\"")));
}

fn run_auth(c: &mut Call<'_>) -> Success {
    match c.args.first() {
        None => {
            c.out.lines.push(OutputLine::error("Parameter missing"));
            false
        }
        Some(token) if c.host.authenticate(token) => {
            c.out.lines.push(OutputLine::info("logged in as administrator"));
            true
        }
        Some(_) => {
            c.out.lines.push(OutputLine::error("wrong token"));
            false
        }
    }
}

fn run_credits(c: &mut Call<'_>) -> Success {
    c.out.lines.push(OutputLine::info(format!(
        "Wurfel Engine Version:{VERSION}\nFor a list of available commands visit the GitHub Wiki.\n\
         Wurfel Engine ({VERSION})\n\nCreated by:\nBenedikt S. Vogler\n\nThanks to:\nThomas Vogt\n\n\
         Wurfel Engine uses libGDX."
    )));
    true
}

fn run_fill_with_air(c: &mut Call<'_>) -> Success {
    let Some(x) = c.args.first() else { return false };
    let Some(y) = c.args.get(1) else {
        c.out.lines.push(OutputLine::error("Expected more parameters"));
        return false;
    };
    let (Some(x), Some(y)) = (int(c.out, x), int(c.out, y)) else { return true };
    report(c.out, c.host.fill_chunk_with_air(x, y))
}

fn run_help(c: &mut Call<'_>) -> Success {
    for command in COMMANDS {
        let first_line = command.manual.lines().next().unwrap_or("");
        c.out.lines.push(OutputLine::info(format!("{:<14}{}", command.name, first_line)));
    }
    true
}

fn run_ls(c: &mut Call<'_>) -> Success {
    let entries = if c.path.is_empty() { c.host.worlds() } else { c.host.saves(c.path.split(':').next().unwrap_or(c.path)) };
    c.out.lines.extend(entries.into_iter().map(OutputLine::info));
    true
}

fn run_man(c: &mut Call<'_>) -> Success {
    let Some(name) = c.args.first() else {
        c.out.lines.push(OutputLine::error("Parameter missing"));
        return false;
    };
    match command(name) {
        Some(found) => {
            c.out.lines.push(OutputLine::info(found.manual));
            true
        }
        None => {
            c.out.lines.push(OutputLine::error("Not found"));
            false
        }
    }
}

fn run_printmap(c: &mut Call<'_>) -> Success {
    let args = c.args;
    let number = |i: usize, default: i32, out: &mut Output| match args.get(i) {
        Some(text) => int(out, text),
        None => Some(default),
    };
    let (Some(x), Some(y), Some(z), Some(w), Some(h)) =
        (number(0, 0, c.out), number(1, 0, c.out), number(2, 1, c.out), number(3, 40, c.out), number(4, 20, c.out))
    else {
        return true;
    };
    if w <= 0 || h <= 0 || w > PRINTMAP_MAX.0 || h > PRINTMAP_MAX.1 {
        c.out.lines.push(OutputLine::error(format!("slice must be 1..{} wide and 1..{} high", PRINTMAP_MAX.0, PRINTMAP_MAX.1)));
        return false;
    }
    match c.host.print_map(x, y, z, w, h) {
        Ok(rows) => {
            c.out.lines.extend(rows.into_iter().map(OutputLine::info));
            true
        }
        Err(e) => report(c.out, Err(e)),
    }
}

fn run_screenshake(c: &mut Call<'_>) -> Success {
    let mut numbers = [10.0f32, 500.0];
    let id = match c.args.first() {
        Some(text) => match int(c.out, text) {
            Some(id) => id,
            None => return true,
        },
        None => 0,
    };
    for (slot, text) in numbers.iter_mut().zip(c.args.iter().skip(1)) {
        match float(c.out, text) {
            Some(v) => *slot = v,
            None => return true,
        }
    }
    report(c.out, c.host.screenshake(id, numbers[0], numbers[1]))
}

/// `teleport` and `tp`: two numbers, a block column.
fn column_command(c: &mut Call<'_>, act: fn(&mut dyn ConsoleHost, i32, i32) -> Result<(), String>) -> Success {
    let (Some(x), Some(y)) = (c.args.first(), c.args.get(1)) else {
        c.out.lines.push(OutputLine::error("Expected more parameters"));
        return false;
    };
    let (Some(x), Some(y)) = (int(c.out, x), int(c.out, y)) else { return true };
    report(c.out, act(c.host, x, y))
}

pub fn command(name: &str) -> Option<&'static CommandInfo> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// Which side owns a cvar of the given system: map and save cvars always belong to the server,
/// root cvars to the server when they were registered as server cvars (they change the simulation,
/// see [`CVarSystem::register_server`]), and everything else (rendering, sound, keys...) to the
/// client. `cvars` is the system as this side knows it.
pub fn cvar_scope(target: &CVarTarget, cvars: Option<&CVarSystem>, name: &str) -> Scope {
    match target {
        CVarTarget::Root if !cvars.and_then(|c| c.get(name)).is_some_and(|c| c.is_server()) => Scope::Client,
        _ => Scope::Server,
    }
}

// ------------------------------------------------------------------------------------- the host

/// Which cvar system a path refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CVarTarget {
    Root,
    Map(String),
    Save(String, String),
}

impl CVarTarget {
    /// `""` is the root system, `map` a map's cvars, `map:slot` the cvars of a save slot.
    pub fn from_path(path: &str) -> CVarTarget {
        match path.split_once(':') {
            _ if path.is_empty() => CVarTarget::Root,
            Some((map, slot)) => CVarTarget::Save(map.to_string(), slot.to_string()),
            None => CVarTarget::Map(path.to_string()),
        }
    }
}

fn unavailable<T>() -> Result<T, String> {
    Err("not available here".to_string())
}

/// How console commands reach the game. A client and a server each implement the parts they own;
/// everything else keeps the default, which reports that the action is not available.
pub trait ConsoleHost {
    /// The cvar system for a path, if it exists and is loaded.
    fn cvars(&mut self, target: &CVarTarget) -> Option<&mut CVarSystem>;

    /// Maps (worlds), for `ls` at the root and as arguments of `cd` and `loadmap`.
    fn worlds(&self) -> Vec<String> {
        Vec::new()
    }
    /// Save slots of a map.
    fn saves(&self, _world: &str) -> Vec<String> {
        Vec::new()
    }
    /// Generator ids, offered as values of the `generator` cvar.
    fn generator_ids(&self) -> Vec<String> {
        Vec::new()
    }

    /// May the caller run admin-only commands? Local hosts (a single-player client) can keep `true`;
    /// the server answers per connection.
    fn is_admin(&self) -> bool {
        true
    }
    /// Check an admin token and, if it is right, remember that this connection is an admin.
    fn authenticate(&mut self, _token: &str) -> bool {
        false
    }

    // --- client actions
    fn toggle_fullscreen(&mut self) -> Result<(), String> {
        unavailable()
    }
    fn reload_shaders(&mut self) -> Result<(), String> {
        unavailable()
    }
    fn toggle_light_engine(&mut self) -> Result<(), String> {
        unavailable()
    }
    fn show_menu(&mut self) -> Result<(), String> {
        unavailable()
    }
    fn start_editor(&mut self) -> Result<(), String> {
        unavailable()
    }
    fn exit(&mut self) -> Result<(), String> {
        unavailable()
    }
    /// Move the camera focus to a block column (Java `tp`).
    fn camera_focus(&mut self, _x: i32, _y: i32) -> Result<(), String> {
        unavailable()
    }
    /// Shake a camera. Return an error such as "Camera ID out of range" for an unknown camera.
    fn screenshake(&mut self, _camera: i32, _amplitude: f32, _millis: f32) -> Result<(), String> {
        unavailable()
    }

    // --- server actions
    fn spawn_benchmark_ball(&mut self) -> Result<(), String> {
        unavailable()
    }
    fn kill_all_entities(&mut self) -> Result<usize, String> {
        unavailable()
    }
    fn fill_chunk_with_air(&mut self, _chunk_x: i32, _chunk_y: i32) -> Result<(), String> {
        unavailable()
    }
    /// A text slice of the map at height `z`, one string per row.
    fn print_map(&mut self, _x: i32, _y: i32, _z: i32, _width: i32, _height: i32) -> Result<Vec<String>, String> {
        unavailable()
    }
    fn save(&mut self) -> Result<String, String> {
        unavailable()
    }
    fn load_map(&mut self, _name: &str) -> Result<String, String> {
        unavailable()
    }
    /// Move the calling player to a block column.
    fn teleport_player(&mut self, _x: i32, _y: i32) -> Result<(), String> {
        unavailable()
    }
}

// ------------------------------------------------------------------------------------- parsing

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    UnterminatedQuote,
}

/// Split a line into words. Whitespace separates words; `"..."` or `'...'` keep spaces inside one
/// word, and inside double quotes `\"` and `\\` are escapes. Quotes may be glued to text:
/// `a"b c"d` is one word, `ab cd`.
pub fn tokenize(line: &str) -> Result<Vec<String>, ParseError> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            quote @ ('"' | '\'') => {
                in_word = true;
                loop {
                    match chars.next() {
                        None => return Err(ParseError::UnterminatedQuote),
                        Some(c) if c == quote => break,
                        Some('\\') if quote == '"' => match chars.next() {
                            Some(escaped @ ('"' | '\\')) => current.push(escaped),
                            Some(other) => {
                                current.push('\\');
                                current.push(other);
                            }
                            None => return Err(ParseError::UnterminatedQuote),
                        },
                        Some(c) => current.push(c),
                    }
                }
            }
            c => {
                in_word = true;
                current.push(c);
            }
        }
    }
    if in_word {
        words.push(current);
    }
    Ok(words)
}

// ------------------------------------------------------------------------------------- history

/// Previously entered lines, navigated with Up and Down. Going back down past the newest entry
/// restores what was being typed (the Java console has a TODO for that).
#[derive(Debug, Clone, Default)]
pub struct History {
    entries: VecDeque<String>,
    cursor: Option<usize>,
    draft: String,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// Remember an executed line. Empty lines and a repeat of the previous line are skipped.
    pub fn push(&mut self, line: &str) {
        self.cursor = None;
        if line.is_empty() || self.entries.back().is_some_and(|last| last == line) {
            return;
        }
        if self.entries.len() == HISTORY_LIMIT {
            self.entries.pop_front();
        }
        self.entries.push_back(line.to_string());
    }

    /// Up: the next older entry. `current` is the text in the input, kept as the draft the first time.
    pub fn up(&mut self, current: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let next = match self.cursor {
            None => {
                self.draft = current.to_string();
                self.entries.len() - 1
            }
            Some(i) => i.saturating_sub(1),
        };
        self.cursor = Some(next);
        Some(self.entries[next].clone())
    }

    /// Down: the next newer entry, and finally the draft again.
    pub fn down(&mut self) -> Option<String> {
        let i = self.cursor?;
        if i + 1 < self.entries.len() {
            self.cursor = Some(i + 1);
            Some(self.entries[i + 1].clone())
        } else {
            self.cursor = None;
            Some(std::mem::take(&mut self.draft))
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(String::as_str)
    }
}

// ----------------------------------------------------------------------------------- suggestions

/// Completion candidates for the word being typed at the end of a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestions {
    /// Byte offset in the line where the word being completed starts.
    pub start: usize,
    /// Words that could replace it, sorted, without duplicates.
    pub candidates: Vec<String>,
}

// -------------------------------------------------------------------------------------- console

/// Which side of the connection a [`Console`] runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Client,
    Server,
}

pub struct Console {
    side: Side,
    path: String,
    history: History,
}

/// Result of running a command: did it succeed (a failure adds `Failed executing command.`)?
type Success = bool;

impl Console {
    pub fn new(side: Side) -> Self {
        Console { side, path: String::new(), history: History::new() }
    }

    /// Current path: `""` (root), `map` or `map:slot`.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The prompt shown before the input, like the Java `path + " $ "`.
    pub fn prompt(&self) -> String {
        format!("{} $ ", self.path)
    }

    pub fn history_mut(&mut self) -> &mut History {
        &mut self.history
    }

    /// Run a line typed by the user (client side). Echoes it first, like the Java log.
    pub fn execute(&mut self, line: &str, host: &mut dyn ConsoleHost) -> ExecResult {
        let line = line.trim();
        let secret = line.split_whitespace().next().is_some_and(|w| w.eq_ignore_ascii_case("auth"));
        // Never keep or show an admin token.
        let shown = if secret { "auth ********" } else { line };
        let echo = OutputLine::echo(format!("{}{}", self.prompt(), shown));
        if line.is_empty() {
            return ExecResult::Done(Output { lines: vec![echo], clear: false });
        }
        if !secret {
            self.history.push(line);
            if let Some(root) = host.cvars(&CVarTarget::Root) {
                let _ = root.set_str("lastConsoleCommand", line);
            }
        }

        let path = self.path.clone();
        match self.dispatch(line, &path, host) {
            Dispatched::Forward => ExecResult::Forward { line: line.to_string(), path, echo: vec![echo] },
            Dispatched::Done(mut output) => {
                output.lines.insert(0, echo);
                ExecResult::Done(output)
            }
        }
    }

    /// Run a line that a client forwarded (server side). `path` is the client's current path, which
    /// decides whether a cvar is a map or a save cvar. Returns what to send back to that client.
    pub fn execute_forwarded(&mut self, line: &str, path: &str, host: &mut dyn ConsoleHost) -> Vec<OutputLine> {
        match self.dispatch(line.trim(), path, host) {
            Dispatched::Done(output) => output.lines,
            // A server never forwards anywhere.
            Dispatched::Forward => vec![OutputLine::error("cannot forward from the server")],
        }
    }

    fn dispatch(&mut self, line: &str, path: &str, host: &mut dyn ConsoleHost) -> Dispatched {
        let mut out = Output::default();
        let words = match tokenize(line) {
            Ok(words) => words,
            Err(ParseError::UnterminatedQuote) => {
                out.lines.push(OutputLine::error("unterminated quote"));
                out.lines.push(OutputLine::error(FAILED));
                return Dispatched::Done(out);
            }
        };
        let Some(first) = words.first().map(|w| w.to_lowercase()) else {
            return Dispatched::Done(out);
        };
        let args = &words[1..];

        // 1. a command
        if let Some(info) = command(&first) {
            if self.side == Side::Client && info.scope == Scope::Server {
                return Dispatched::Forward;
            }
            if self.side == Side::Server && info.scope == Scope::Client {
                out.lines.push(OutputLine::error(format!("{first}: this command runs on the client")));
                out.lines.push(OutputLine::error(FAILED));
                return Dispatched::Done(out);
            }
            if info.admin_only && !host.is_admin() {
                out.lines.push(OutputLine::error(format!(
                    "{first}: permission denied. Log in with `auth <token>`"
                )));
                out.lines.push(OutputLine::error(FAILED));
                return Dispatched::Done(out);
            }
            let ok = self.run_command(info, args, path, host, &mut out);
            if !ok {
                out.lines.push(OutputLine::error(FAILED));
            }
            return Dispatched::Done(out);
        }

        // 2. a cvar: `name`, `name value`, or `set name [value]`
        let (name, value_words) = if first == "set" && !args.is_empty() {
            (args[0].to_lowercase(), &args[1..])
        } else {
            (first.clone(), args)
        };
        let target = CVarTarget::from_path(path);
        let scope = cvar_scope(&target, host.cvars(&target).map(|c| &*c), &name);
        if self.side == Side::Client && scope == Scope::Server {
            // Not ours to answer, and the server knows whether the name exists.
            return Dispatched::Forward;
        }
        let known = host.cvars(&target).is_some_and(|c| c.get(&name).is_some());
        if known {
            let ok = self.run_cvar(&target, &name, value_words, scope, host, &mut out);
            if !ok {
                out.lines.push(OutputLine::error(FAILED));
            }
        } else {
            out.lines.push(OutputLine::error(format!("{line}: command not found")));
            out.lines.push(OutputLine::error(FAILED));
        }
        Dispatched::Done(out)
    }

    fn run_cvar(
        &mut self,
        target: &CVarTarget,
        name: &str,
        value_words: &[String],
        scope: Scope,
        host: &mut dyn ConsoleHost,
        out: &mut Output,
    ) -> Success {
        // Setting a server cvar changes the game for everybody.
        if !value_words.is_empty() && self.side == Side::Server && scope == Scope::Server && !host.is_admin() {
            out.lines.push(OutputLine::error(format!(
                "{name}: permission denied. Log in with `auth <token>`"
            )));
            return false;
        }
        let Some(cvars) = host.cvars(target) else {
            out.lines.push(OutputLine::error("cvars of this path are not loaded"));
            return false;
        };
        if !value_words.is_empty() {
            // Strings take the rest of the line; every other type takes the first word.
            let text = value_words.join(" ");
            if let Err(e) = cvars.set(name, &text) {
                out.lines.push(OutputLine::error(e.to_string()));
                return false;
            }
        }
        let Some(cvar) = cvars.get(name) else { return false };
        out.lines.push(OutputLine::info(format!("cvar {} has value {}", name, cvar.value())));
        true
    }

    fn run_command(
        &mut self,
        info: &CommandInfo,
        args: &[String],
        path: &str,
        host: &mut dyn ConsoleHost,
        out: &mut Output,
    ) -> Success {
        (info.run)(&mut Call { console: self, args, path, host, out })
    }

    /// `cd`: `/` goes to the root, `..` up one level, anything else descends (map, then save slot).
    fn change_directory(&mut self, args: &[String], host: &mut dyn ConsoleHost, out: &mut Output) -> Success {
        let Some(entered) = args.first() else {
            out.lines.push(OutputLine::error("Parameter missing"));
            return false;
        };
        let new_path = match entered.as_str() {
            "/" => String::new(),
            // Java jumps straight to the root; going up one level is what `..` means everywhere else.
            ".." => self.path.rsplit_once(':').map(|(map, _)| map.to_string()).unwrap_or_default(),
            name if self.path.is_empty() => name.to_string(),
            name => format!("{}:{}", self.path, name),
        };
        if Self::valid_path(&new_path, host) {
            self.path = new_path;
            true
        } else {
            out.lines.push(OutputLine::info("not a valid path"));
            true // Java prints the message but the command itself succeeded
        }
    }

    /// `checkPath`: the root, an existing map, or an existing save slot of an existing map.
    fn valid_path(path: &str, host: &dyn ConsoleHost) -> bool {
        if path.is_empty() {
            return true;
        }
        let mut parts = path.split(':');
        let map = parts.next().unwrap_or("");
        if !host.worlds().iter().any(|w| w == map) {
            return false;
        }
        match (parts.next(), parts.next()) {
            (None, _) => true,
            (Some(slot), None) => !slot.is_empty() && host.saves(map).iter().any(|s| s == slot),
            _ => false,
        }
    }

    // ----------------------------------------------------------------------------- completion

    /// Completion candidates for the word at the end of `line`.
    pub fn suggest(&self, line: &str, host: &mut dyn ConsoleHost) -> Suggestions {
        let start = if line.ends_with(char::is_whitespace) || line.is_empty() {
            line.len()
        } else {
            line.rfind(char::is_whitespace).map_or(0, |i| i + line[i..].chars().next().map_or(1, char::len_utf8))
        };
        let word = line[start..].to_lowercase();
        let before: Vec<&str> = line[..start].split_whitespace().collect();
        let target = CVarTarget::from_path(&self.path);

        let mut candidates: Vec<String> = match before.as_slice() {
            // The command word: commands and cvars.
            [] => {
                let mut c: Vec<String> =
                    COMMANDS.iter().filter(|c| c.name.starts_with(&word)).map(|c| c.name.to_string()).collect();
                if let Some(cvars) = host.cvars(&target) {
                    c.extend(cvars.suggestions(&word));
                }
                c
            }
            [first] => match first.to_lowercase().as_str() {
                "cd" => {
                    let entries = if self.path.is_empty() {
                        host.worlds()
                    } else if self.path.contains(':') {
                        Vec::new()
                    } else {
                        host.saves(&self.path)
                    };
                    let mut entries: Vec<String> = ["/", ".."].iter().map(|s| s.to_string()).chain(entries).collect();
                    entries.retain(|e| e.to_lowercase().starts_with(&word));
                    entries
                }
                "loadmap" => host.worlds().into_iter().filter(|w| w.to_lowercase().starts_with(&word)).collect(),
                "man" => COMMANDS.iter().filter(|c| c.name.starts_with(&word)).map(|c| c.name.to_string()).collect(),
                "set" => host.cvars(&target).map(|c| c.suggestions(&word)).unwrap_or_default(),
                cvar_name => self.value_suggestions(cvar_name, &word, &target, host),
            },
            // `set <name> <value>`
            [set, name] if set.eq_ignore_ascii_case("set") => {
                self.value_suggestions(&name.to_lowercase(), &word, &target, host)
            }
            _ => Vec::new(),
        };
        candidates.sort();
        candidates.dedup();
        Suggestions { start, candidates }
    }

    fn value_suggestions(&self, name: &str, word: &str, target: &CVarTarget, host: &mut dyn ConsoleHost) -> Vec<String> {
        if name == "generator" {
            return host.generator_ids().into_iter().filter(|g| g.starts_with(word)).collect();
        }
        match host.cvars(target).and_then(|c| c.get(name)).map(|c| c.value()) {
            Some(Value::Bool(_)) => ["true", "false"].iter().filter(|b| b.starts_with(word)).map(|b| b.to_string()).collect(),
            _ => Vec::new(),
        }
    }

    /// Like [`Console::suggest`] but as complete replacement lines (`line` up to the word, plus each
    /// candidate), which is what the HTML console wants.
    pub fn suggest_lines(&self, line: &str, host: &mut dyn ConsoleHost) -> Vec<String> {
        let s = self.suggest(line, host);
        s.candidates.into_iter().map(|c| format!("{}{}", &line[..s.start], c)).collect()
    }
}

enum Dispatched {
    Done(Output),
    Forward,
}

// ------------------------------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    /// Records what the console asked the game to do.
    struct Host {
        root: CVarSystem,
        map: CVarSystem,
        admin: bool,
        token: &'static str,
        calls: Vec<String>,
    }

    impl Host {
        fn new() -> Self {
            Host { root: CVarSystem::root(), map: CVarSystem::map(), admin: true, token: "secret", calls: Vec::new() }
        }
    }

    impl ConsoleHost for Host {
        fn cvars(&mut self, target: &CVarTarget) -> Option<&mut CVarSystem> {
            match target {
                CVarTarget::Root => Some(&mut self.root),
                CVarTarget::Map(m) if m == "alpha" => Some(&mut self.map),
                _ => None,
            }
        }
        fn worlds(&self) -> Vec<String> {
            vec!["alpha".into(), "beta".into()]
        }
        fn saves(&self, world: &str) -> Vec<String> {
            if world == "alpha" { vec!["0".into(), "1".into()] } else { Vec::new() }
        }
        fn generator_ids(&self) -> Vec<String> {
            vec!["island".into(), "caveland".into(), "air".into()]
        }
        fn is_admin(&self) -> bool {
            self.admin
        }
        fn authenticate(&mut self, token: &str) -> bool {
            self.admin = token == self.token;
            self.admin
        }
        fn kill_all_entities(&mut self) -> Result<usize, String> {
            self.calls.push("killall".into());
            Ok(3)
        }
        fn fill_chunk_with_air(&mut self, x: i32, y: i32) -> Result<(), String> {
            self.calls.push(format!("fillwithair {x} {y}"));
            Ok(())
        }
        fn screenshake(&mut self, camera: i32, amp: f32, ms: f32) -> Result<(), String> {
            self.calls.push(format!("screenshake {camera} {amp} {ms}"));
            if camera == 0 { Ok(()) } else { Err("Camera ID out of range".into()) }
        }
        fn camera_focus(&mut self, x: i32, y: i32) -> Result<(), String> {
            self.calls.push(format!("tp {x} {y}"));
            Ok(())
        }
        fn teleport_player(&mut self, x: i32, y: i32) -> Result<(), String> {
            self.calls.push(format!("teleport {x} {y}"));
            Ok(())
        }
        fn print_map(&mut self, x: i32, y: i32, z: i32, w: i32, h: i32) -> Result<Vec<String>, String> {
            self.calls.push(format!("printmap {x} {y} {z} {w} {h}"));
            Ok(vec!["#.#".into(), ".#.".into()])
        }
        fn save(&mut self) -> Result<String, String> {
            Ok("saved".into())
        }
        fn toggle_fullscreen(&mut self) -> Result<(), String> {
            self.calls.push("fullscreen".into());
            Ok(())
        }
    }

    fn texts(lines: &[OutputLine]) -> Vec<&str> {
        lines.iter().map(|l| l.text.as_str()).collect()
    }

    fn done(result: ExecResult) -> Output {
        match result {
            ExecResult::Done(output) => output,
            other => panic!("expected a local result, got {other:?}"),
        }
    }

    fn client() -> Console {
        Console::new(Side::Client)
    }

    // --- parsing

    #[test]
    fn tokenize_splits_on_any_whitespace() {
        assert_eq!(tokenize("  tp   3\t4 ").unwrap(), ["tp", "3", "4"]);
        assert_eq!(tokenize("").unwrap(), Vec::<String>::new());
        assert_eq!(tokenize("   ").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn tokenize_handles_quotes_and_escapes() {
        assert_eq!(tokenize(r#"loadmap "my world""#).unwrap(), ["loadmap", "my world"]);
        assert_eq!(tokenize("loadmap 'my world'").unwrap(), ["loadmap", "my world"]);
        assert_eq!(tokenize(r#"say "a \"b\" \\ c""#).unwrap(), ["say", r#"a "b" \ c"#]);
        assert_eq!(tokenize(r#"a"b c"d"#).unwrap(), ["ab cd"], "quotes glue to neighbouring text");
        assert_eq!(tokenize(r#"x "" y"#).unwrap(), ["x", "", "y"], "an empty quoted word is a word");
        assert_eq!(tokenize(r#"it's"#), Err(ParseError::UnterminatedQuote));
        assert_eq!(tokenize(r#""open"#), Err(ParseError::UnterminatedQuote));
    }

    #[test]
    fn an_unterminated_quote_is_reported_not_executed() {
        let out = done(client().execute("loadmap \"oops", &mut Host::new()));
        assert_eq!(texts(&out.lines)[1..], ["unterminated quote", FAILED]);
    }

    // --- resolution order and messages

    #[test]
    fn the_line_is_echoed_with_the_prompt() {
        let out = done(client().execute("credits", &mut Host::new()));
        assert_eq!(out.lines[0], OutputLine::echo(" $ credits"));
        assert!(out.lines[1].text.contains("Benedikt S. Vogler"));
    }

    #[test]
    fn unknown_commands_get_the_java_messages() {
        let out = done(client().execute("frobnicate now", &mut Host::new()));
        assert_eq!(
            out.lines[1..],
            [OutputLine::error("frobnicate now: command not found"), OutputLine::error(FAILED)]
        );
    }

    #[test]
    fn commands_are_case_insensitive_and_empty_lines_do_nothing() {
        let mut host = Host::new();
        let mut console = client();
        let out = done(console.execute("CREDITS", &mut host));
        assert!(out.lines.len() > 1);
        let out = done(console.execute("   ", &mut host));
        assert_eq!(out.lines.len(), 1, "just the echo");
    }

    #[test]
    fn clear_asks_the_ui_to_wipe_the_log() {
        let out = done(client().execute("clear", &mut Host::new()));
        assert!(out.clear);
    }

    #[test]
    fn man_prints_the_java_manual() {
        let mut host = Host::new();
        let mut console = client();
        let out = done(console.execute("man tp", &mut host));
        assert_eq!(out.lines[1].text, "set the focus of the camera.\nParameters: [x game world][y game world]");
        let out = done(console.execute("man nonsense", &mut host));
        assert_eq!(texts(&out.lines)[1..], ["Not found", FAILED]);
        let out = done(console.execute("man", &mut host));
        assert_eq!(texts(&out.lines)[1..], ["Parameter missing", FAILED]);
    }

    #[test]
    fn help_lists_every_command_once() {
        let out = done(client().execute("help", &mut Host::new()));
        for c in COMMANDS {
            assert_eq!(out.lines.iter().filter(|l| l.text.starts_with(&format!("{:<14}", c.name))).count(), 1, "{}", c.name);
        }
    }

    // --- cvars

    #[test]
    fn a_client_cvar_is_read_and_set_through_the_console() {
        let mut host = Host::new();
        let mut console = client();
        let out = done(console.execute("music", &mut host));
        assert_eq!(out.lines[1], OutputLine::info("cvar music has value 1.0"));
        let out = done(console.execute("MUSIC 0.5", &mut host));
        assert_eq!(out.lines[1], OutputLine::info("cvar music has value 0.5"));
        assert_eq!(host.root.get_f32("music"), Ok(0.5));
        let out = done(console.execute("set music 0.25", &mut host));
        assert_eq!(out.lines[1], OutputLine::info("cvar music has value 0.25"));
    }

    #[test]
    fn a_bad_cvar_value_is_an_error_and_changes_nothing() {
        let mut host = Host::new();
        let out = done(client().execute("music loud", &mut host));
        assert_eq!(out.lines[1].level, Level::Error);
        assert!(out.lines[1].text.contains("music"), "{}", out.lines[1].text);
        assert_eq!(out.lines[2], OutputLine::error(FAILED));
        assert_eq!(host.root.get_f32("music"), Ok(1.0));
    }

    #[test]
    fn string_cvars_take_the_rest_of_the_line_even_when_the_value_overlaps_the_name() {
        let mut host = Host::new();
        let mut console = client();
        done(console.execute("cd alpha", &mut host));
        // `CVarSystem::execute` would have stored "description desc" here.
        let result = console.execute("description \"a desc of it\"", &mut host);
        assert!(matches!(result, ExecResult::Forward { .. }), "map cvars live on the server");

        let mut server = Console::new(Side::Server);
        let lines = server.execute_forwarded("description desc", "alpha", &mut host);
        assert_eq!(texts(&lines), ["cvar description has value desc"]);
        assert_eq!(host.map.get_str("description"), Ok("desc"));
        let lines = server.execute_forwarded("description a b  c", "alpha", &mut host);
        assert_eq!(host.map.get_str("description"), Ok("a b c"));
        assert_eq!(texts(&lines), ["cvar description has value a b c"]);
    }

    #[test]
    fn every_command_updates_last_console_command() {
        let mut host = Host::new();
        client().execute("music 0.5", &mut host);
        assert_eq!(host.root.get_str("lastConsoleCommand"), Ok("music 0.5"));
    }

    // --- scope routing and permissions

    #[test]
    fn client_commands_run_locally_and_server_commands_are_forwarded() {
        let mut host = Host::new();
        let mut console = client();
        done(console.execute("fullscreen", &mut host));
        assert_eq!(host.calls, ["fullscreen"]);

        match console.execute("killall", &mut host) {
            ExecResult::Forward { line, path, echo } => {
                assert_eq!((line.as_str(), path.as_str()), ("killall", ""));
                assert_eq!(echo, [OutputLine::echo(" $ killall")]);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(host.calls, ["fullscreen"], "nothing ran locally");
    }

    #[test]
    fn server_cvars_are_forwarded_and_client_cvars_are_not() {
        let mut host = Host::new();
        let mut console = client();
        for line in ["gravity 5", "gravity", "playerWalkingSpeed 6", "set friction 0.1"] {
            assert!(matches!(console.execute(line, &mut host), ExecResult::Forward { .. }), "{line}");
        }
        assert_eq!(host.root.get_f32("gravity"), Ok(9.81), "the local copy is untouched");
        assert!(matches!(console.execute("music 0.3", &mut host), ExecResult::Done(_)));
        assert_eq!(cvar_scope(&CVarTarget::Map("m".into()), None, "mapname"), Scope::Server);
        // The owner is a property of the cvar, set when it is registered.
        let root = CVarSystem::root();
        assert_eq!(cvar_scope(&CVarTarget::Root, Some(&root), "timeSpeed"), Scope::Server);
        assert_eq!(cvar_scope(&CVarTarget::Root, Some(&root), "music"), Scope::Client);
    }

    #[test]
    fn the_server_runs_forwarded_commands_and_refuses_client_ones() {
        let mut host = Host::new();
        let mut server = Console::new(Side::Server);
        assert_eq!(texts(&server.execute_forwarded("killall", "", &mut host)), ["disposed 3 entities"]);
        assert_eq!(host.calls, ["killall"]);
        let lines = server.execute_forwarded("fullscreen", "", &mut host);
        assert_eq!(texts(&lines), ["fullscreen: this command runs on the client", FAILED]);
        assert_eq!(host.calls, ["killall"]);
    }

    #[test]
    fn admin_commands_and_cvar_writes_need_a_login_but_reading_does_not() {
        let mut host = Host::new();
        host.admin = false;
        let mut server = Console::new(Side::Server);

        let lines = server.execute_forwarded("killall", "", &mut host);
        assert!(lines[0].text.contains("permission denied"), "{lines:?}");
        assert!(host.calls.is_empty());

        let lines = server.execute_forwarded("gravity 1", "", &mut host);
        assert!(lines[0].text.contains("permission denied"), "{lines:?}");
        assert_eq!(host.root.get_f32("gravity"), Ok(9.81));

        let lines = server.execute_forwarded("gravity", "", &mut host);
        assert_eq!(texts(&lines), ["cvar gravity has value 9.81"], "reading is open");
        let lines = server.execute_forwarded("printmap 0 0 1 3 2", "", &mut host);
        assert_eq!(texts(&lines), ["#.#", ".#."], "read-only commands are open too");

        let lines = server.execute_forwarded("auth wrong", "", &mut host);
        assert_eq!(texts(&lines), ["wrong token", FAILED]);
        assert!(!host.admin);
        let lines = server.execute_forwarded("auth secret", "", &mut host);
        assert_eq!(texts(&lines), ["logged in as administrator"]);
        assert_eq!(texts(&server.execute_forwarded("killall", "", &mut host)), ["disposed 3 entities"]);
    }

    #[test]
    fn auth_tokens_are_never_echoed_or_remembered() {
        let mut host = Host::new();
        let mut console = client();
        match console.execute("auth hunter2", &mut host) {
            ExecResult::Forward { echo, .. } => assert_eq!(echo, [OutputLine::echo(" $ auth ********")]),
            other => panic!("{other:?}"),
        }
        assert_eq!(console.history.entries().count(), 0);
        assert_eq!(host.root.get_str("lastConsoleCommand"), Ok(""));
    }

    // --- individual commands

    #[test]
    fn numeric_arguments_are_parsed_like_java() {
        let mut host = Host::new();
        let mut server = Console::new(Side::Server);
        assert!(server.execute_forwarded("fillwithair 2 -1", "", &mut host).is_empty());
        assert_eq!(host.calls, ["fillwithair 2 -1"]);

        let lines = server.execute_forwarded("fillwithair x 1", "", &mut host);
        assert_eq!(texts(&lines), ["Command crashed: For input string: \"x\""], "handled, so no 'Failed' line");
        assert_eq!(texts(&server.execute_forwarded("fillwithair", "", &mut host)), [FAILED]);
        assert_eq!(texts(&server.execute_forwarded("fillwithair 4", "", &mut host)), ["Expected more parameters", FAILED]);
        assert_eq!(host.calls.len(), 1);
    }

    #[test]
    fn tp_moves_the_camera_and_teleport_moves_the_player() {
        let mut host = Host::new();
        let mut console = client();
        done(console.execute("tp 3 4", &mut host));
        assert_eq!(host.calls, ["tp 3 4"]);
        let out = done(console.execute("tp 3", &mut host));
        assert_eq!(texts(&out.lines)[1..], ["Expected more parameters", FAILED]);

        let mut server = Console::new(Side::Server);
        server.execute_forwarded("teleport 7 8", "", &mut host);
        assert_eq!(host.calls.last().unwrap(), "teleport 7 8");
    }

    #[test]
    fn screenshake_has_java_defaults_and_reports_a_bad_camera() {
        let mut host = Host::new();
        let mut console = client();
        done(console.execute("screenshake", &mut host));
        done(console.execute("screenshake 0 3.5 100", &mut host));
        let out = done(console.execute("screenshake 2", &mut host));
        assert_eq!(host.calls, ["screenshake 0 10 500", "screenshake 0 3.5 100", "screenshake 2 10 500"]);
        assert_eq!(texts(&out.lines)[1..], ["Camera ID out of range", FAILED]);
    }

    #[test]
    fn printmap_has_defaults_and_a_size_limit() {
        let mut host = Host::new();
        let mut server = Console::new(Side::Server);
        server.execute_forwarded("printmap", "", &mut host);
        assert_eq!(host.calls, ["printmap 0 0 1 40 20"]);
        let lines = server.execute_forwarded("printmap 0 0 0 500 5", "", &mut host);
        assert!(lines[0].text.starts_with("slice must be"));
        assert_eq!(host.calls.len(), 1, "too large a slice never reaches the game");
    }

    #[test]
    fn actions_the_host_does_not_offer_report_that() {
        let out = done(client().execute("reloadshaders", &mut Host::new()));
        assert_eq!(texts(&out.lines)[1..], ["not available here", FAILED]);
    }

    // --- cd and ls

    #[test]
    fn cd_walks_from_the_root_to_a_map_and_a_save_and_back() {
        let mut host = Host::new();
        let mut console = client();
        let out = done(console.execute("ls", &mut host));
        assert_eq!(texts(&out.lines)[1..], ["alpha", "beta"]);

        done(console.execute("cd alpha", &mut host));
        assert_eq!((console.path(), console.prompt().as_str()), ("alpha", "alpha $ "));
        assert_eq!(texts(&done(console.execute("ls", &mut host)).lines)[1..], ["0", "1"]);

        done(console.execute("cd 1", &mut host));
        assert_eq!(console.path(), "alpha:1");
        done(console.execute("cd ..", &mut host));
        assert_eq!(console.path(), "alpha");
        done(console.execute("cd ..", &mut host));
        assert_eq!(console.path(), "");
        done(console.execute("cd alpha", &mut host));
        done(console.execute("cd /", &mut host));
        assert_eq!(console.path(), "");
    }

    #[test]
    fn cd_refuses_unknown_maps_and_slots() {
        let mut host = Host::new();
        let mut console = client();
        let out = done(console.execute("cd gamma", &mut host));
        assert_eq!(texts(&out.lines)[1..], ["not a valid path"]);
        assert_eq!(console.path(), "");
        done(console.execute("cd alpha", &mut host));
        done(console.execute("cd 9", &mut host));
        assert_eq!(console.path(), "alpha");
        done(console.execute("cd 1", &mut host));
        let out = done(console.execute("cd 0", &mut host));
        assert_eq!(texts(&out.lines)[1..], ["not a valid path"], "a save has no sub-directories");
        let out = done(console.execute("cd", &mut host));
        assert_eq!(texts(&out.lines)[1..], ["Parameter missing", FAILED]);
    }

    #[test]
    fn inside_a_map_cvars_are_map_cvars_and_go_to_the_server() {
        let mut host = Host::new();
        let mut console = client();
        done(console.execute("cd alpha", &mut host));
        match console.execute("mapname", &mut host) {
            ExecResult::Forward { path, echo, .. } => {
                assert_eq!(path, "alpha");
                assert_eq!(echo, [OutputLine::echo("alpha $ mapname")]);
            }
            other => panic!("{other:?}"),
        }
        let mut server = Console::new(Side::Server);
        assert_eq!(texts(&server.execute_forwarded("groundBlockID", "alpha", &mut host)), ["cvar groundblockid has value 1"]);
        assert_eq!(texts(&server.execute_forwarded("groundBlockID", "", &mut host)), ["cvar groundblockid has value 2"], "root differs from map");
    }

    // --- suggestions

    fn suggest(console: &Console, line: &str) -> Vec<String> {
        console.suggest(line, &mut Host::new()).candidates
    }

    #[test]
    fn commands_and_cvars_complete_by_prefix_case_insensitively() {
        let console = client();
        assert_eq!(suggest(&console, "ki"), ["killall"]);
        assert_eq!(suggest(&console, "KI"), ["killall"]);
        assert_eq!(suggest(&console, "grav"), ["gravity"]);
        let both = suggest(&console, "s");
        assert!(both.contains(&"save".to_string()) && both.contains(&"screenshake".to_string()));
        assert!(both.contains(&"sound".to_string()), "cvars are offered too: {both:?}");
        assert!(both.windows(2).all(|w| w[0] < w[1]), "sorted and unique");
        assert!(suggest(&console, "zzz").is_empty());
    }

    #[test]
    fn arguments_complete_from_the_host() {
        let mut console = client();
        assert_eq!(suggest(&console, "man ki"), ["killall"]);
        assert_eq!(suggest(&console, "loadmap "), ["alpha", "beta"]);
        assert_eq!(suggest(&console, "loadmap b"), ["beta"]);
        assert_eq!(suggest(&console, "cd a"), ["alpha"]);
        assert_eq!(suggest(&console, "cd "), ["..", "/", "alpha", "beta"]);
        assert_eq!(suggest(&console, "generator "), ["air", "caveland", "island"]);
        assert_eq!(suggest(&console, "generator is"), ["island"]);
        assert_eq!(suggest(&console, "set generator c"), ["caveland"]);
        assert_eq!(suggest(&console, "set grav"), ["gravity"]);
        assert_eq!(suggest(&console, "enableHSD t"), ["true"]);
        assert!(suggest(&console, "music ").is_empty(), "no candidates for a number");

        done(console.execute("cd alpha", &mut Host::new()));
        // `cd` was run against a throwaway host above, so go there with a real one.
        let mut host = Host::new();
        done(console.execute("cd alpha", &mut host));
        assert_eq!(console.suggest("cd ", &mut host).candidates, ["..", "/", "0", "1"]);
    }

    #[test]
    fn suggestions_know_where_the_word_starts_and_can_build_whole_lines() {
        let console = client();
        let s = console.suggest("man ki", &mut Host::new());
        assert_eq!((s.start, s.candidates.as_slice()), (4, ["killall".to_string()].as_slice()));
        assert_eq!(console.suggest_lines("man ki", &mut Host::new()), ["man killall"]);
        assert_eq!(console.suggest("", &mut Host::new()).start, 0);
        assert!(console.suggest_lines("ki", &mut Host::new()).contains(&"killall".to_string()));
    }

    // --- history

    #[test]
    fn history_walks_back_and_forth_and_restores_the_draft() {
        let mut h = History::new();
        assert_eq!(h.up("x"), None, "nothing to go back to");
        h.push("first");
        h.push("second");
        h.push("second"); // repeats are skipped
        h.push("");
        assert_eq!(h.entries().collect::<Vec<_>>(), ["first", "second"]);

        assert_eq!(h.up("draft"), Some("second".into()));
        assert_eq!(h.up("ignored"), Some("first".into()));
        assert_eq!(h.up("ignored"), Some("first".into()), "stays at the oldest");
        assert_eq!(h.down(), Some("second".into()));
        assert_eq!(h.down(), Some("draft".into()), "back to what was typed");
        assert_eq!(h.down(), None, "nothing newer than the draft");
    }

    #[test]
    fn history_is_bounded_and_restarts_after_a_new_entry() {
        let mut h = History::new();
        for i in 0..HISTORY_LIMIT + 10 {
            h.push(&format!("cmd {i}"));
        }
        assert_eq!(h.entries().count(), HISTORY_LIMIT);
        assert_eq!(h.entries().next(), Some("cmd 10"));
        h.up("");
        h.push("new");
        assert_eq!(h.up(""), Some("new".into()), "navigation starts again from the newest");
    }

    #[test]
    fn executing_fills_the_history() {
        let mut host = Host::new();
        let mut console = client();
        console.execute("credits", &mut host);
        console.execute("tp 1 2", &mut host);
        assert_eq!(console.history_mut().up(""), Some("tp 1 2".into()));
    }
}
