//! Console variables ("cvars"): named, typed settings such as `gravity` or `playerfriction`.
//!
//! Port of the Java engine's `core.cvar` package. What carries over:
//!
//! * Three independent systems with their own defaults: [`CVarSystem::root`] (engine wide, Java
//!   `CVarSystemRoot`), [`CVarSystem::map`] (per map, `CVarSystemMap`) and [`CVarSystem::save`]
//!   (per save slot, `CVarSystemSave`). The Java systems do **not** inherit from each other: the console
//!   just picks one depending on where you are, and so does the caller here. `groundBlockID` is 2 in
//!   the root system and 1 in the map system, on purpose.
//! * Names are case-insensitive. A cvar has a type (float, int, bool, string), a default and a
//!   [`Flags`] value that decides persistence.
//! * The file format: one `name value` pair per line, written for every `Archive` cvar that differs
//!   from its default and for every `InstantSave` cvar, never for `Volatile` ones. Loading only
//!   touches cvars that are already registered. See [`CVarSystem::save_string`].
//! * The console syntax: `name` prints the value, `name value` sets it ([`CVarSystem::execute`]).
//!
//! Differences from Java, on purpose:
//!
//! * No hidden side effects. Java saves to disk from inside `setValue` (and does so inconsistently:
//!   floats, bools and strings on `Archive`, ints only on `InstantSave`). Here setters only change
//!   memory; call [`CVarSystem::save`] when you want a file. This also keeps the type free of file and
//!   global state, so it is `Send + Sync` and can sit behind an `Arc<RwLock<_>>` ([`SharedCVars`]).
//! * Errors are values: an unknown name, a wrong type or an unparsable value returns a [`CVarError`]
//!   instead of an NPE, a `ClassCastException` or a console line.
//! * Booleans accept `1`/`true` and `0`/`false`; anything else is an error. Java treated every string
//!   except `"1"` as false, so `enableHSD true` silently turned the option off.
//! * String values keep spaces when loaded (Java kept only the first word).
//! * Saved lines are sorted by name, so files are stable (Java wrote in hash order).
//! * No global singleton (`WE.getCVars()`) and no static `CustomMapCVarRegistration` hook: construct
//!   the system you need and call [`CVarSystem::register`] on it to add game specific cvars.
//!
//! ```
//! use wurfel_sim::cvar::CVarSystem;
//!
//! let mut cvars = CVarSystem::root();
//! assert_eq!(cvars.get_f32("gravity").unwrap(), 9.81);
//! cvars.execute("gravity 5").unwrap();
//! assert_eq!(cvars.get_f32("Gravity").unwrap(), 5.0);
//! ```

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, RwLock};

/// The map file version the Java engine writes (`Map.MAPVERSION`).
pub const MAP_VERSION: i32 = 4;

/// libGDX key codes used as defaults for the key binding cvars (`Input.Keys`).
const KEY_TAB: i32 = 61;
const KEY_F1: i32 = 131;

/// When a cvar is written to a file. Java: `CVarFlags` (including its `VOlATILE` spelling).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flags {
    /// Saved when the system is saved, but only if it differs from its default.
    Archive,
    /// Never saved.
    Volatile,
    /// Always saved, even at its default (used for the map version).
    InstantSave,
}

/// The value of a cvar. The variant is the cvar's type and never changes after registration.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Float(f32),
    Int(i32),
    Bool(bool),
    Str(String),
}

impl Value {
    fn type_name(&self) -> &'static str {
        match self {
            Value::Float(_) => "float",
            Value::Int(_) => "int",
            Value::Bool(_) => "bool",
            Value::Str(_) => "string",
        }
    }

    /// Java's `defaultValue.equals(value)`: floats compare by bits (so `NaN == NaN`, `0.0 != -0.0`).
    fn same(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
            _ => self == other,
        }
    }

    /// Parse text into a value of the same type as `self`.
    fn parse_like(&self, name: &str, text: &str) -> Result<Value, CVarError> {
        let bad = || CVarError::Parse {
            name: name.to_string(),
            expected: self.type_name(),
            input: text.to_string(),
        };
        match self {
            Value::Float(_) => {
                // Java's Float.parseFloat tolerates surrounding spaces and an f/d type suffix.
                let t = text.trim();
                let t = t.strip_suffix(['f', 'F', 'd', 'D']).unwrap_or(t);
                t.parse::<f32>().map(Value::Float).map_err(|_| bad())
            }
            Value::Int(_) => text.trim().parse::<i32>().map(Value::Int).map_err(|_| bad()),
            Value::Bool(_) => match text.trim() {
                "1" | "true" => Ok(Value::Bool(true)),
                "0" | "false" => Ok(Value::Bool(false)),
                _ => Err(bad()),
            },
            Value::Str(_) => Ok(Value::Str(text.to_string())),
        }
    }
}

impl fmt::Display for Value {
    /// The text written to files and shown in the console, matching Java's `toString`: `1.0` for
    /// floats, `1`/`0` for booleans.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Float(v) => write!(f, "{v:?}"),
            Value::Int(v) => write!(f, "{v}"),
            Value::Bool(v) => f.write_str(if *v { "1" } else { "0" }),
            Value::Str(v) => f.write_str(v),
        }
    }
}

/// One console variable.
#[derive(Debug, Clone)]
pub struct CVar {
    name: String,
    value: Value,
    default: Value,
    flags: Flags,
    /// Owned by the server because it changes the simulation (`gravity`); the console forwards it
    /// there. See [`CVarSystem::register_server`].
    server: bool,
}

impl CVar {
    /// The name as it was registered (lookups ignore case).
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn value(&self) -> &Value {
        &self.value
    }
    pub fn default_value(&self) -> &Value {
        &self.default
    }
    pub fn flags(&self) -> Flags {
        self.flags
    }
    /// Does the server own it (see [`CVarSystem::register_server`])?
    pub fn is_server(&self) -> bool {
        self.server
    }
    /// Does it still have its default value?
    pub fn is_default(&self) -> bool {
        self.value.same(&self.default)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CVarError {
    /// Nothing is registered under that name.
    Unknown(String),
    /// Asked for a cvar as the wrong type, e.g. `get_i32("gravity")`.
    TypeMismatch { name: String, expected: &'static str, found: &'static str },
    /// Text that is not a valid value for the cvar's type.
    Parse { name: String, expected: &'static str, input: String },
    /// An empty console line.
    EmptyCommand,
}

impl fmt::Display for CVarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CVarError::Unknown(name) => write!(f, "cvar \"{name}\" is not defined"),
            CVarError::TypeMismatch { name, expected, found } => {
                write!(f, "cvar \"{name}\" is a {found}, not a {expected}")
            }
            CVarError::Parse { name, expected, input } => {
                write!(f, "cannot set \"{name}\" to \"{input}\": expected a {expected}")
            }
            CVarError::EmptyCommand => f.write_str("empty command"),
        }
    }
}

impl std::error::Error for CVarError {}

/// What [`CVarSystem::load_str`] did with a file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LoadReport {
    /// Cvars that were set.
    pub applied: usize,
    /// Names in the file that are not registered. Java ignores these silently, so should you.
    pub unknown: Vec<String>,
    /// `(name, text)` pairs whose text was not a valid value. The cvar keeps its old value.
    pub invalid: Vec<(String, String)>,
}

/// A cvar system that can be shared between threads.
pub type SharedCVars = Arc<RwLock<CVarSystem>>;

/// A set of cvars. Java: `AbstractCVarSystem`.
#[derive(Debug, Clone, Default)]
pub struct CVarSystem {
    /// Keyed by lowercase name.
    cvars: HashMap<String, CVar>,
}

/// Lowercase only when needed, so lookups with an already lowercase name do not allocate.
fn key(name: &str) -> Cow<'_, str> {
    if name.chars().any(|c| c.is_uppercase()) {
        Cow::Owned(name.to_lowercase())
    } else {
        Cow::Borrowed(name)
    }
}

impl CVarSystem {
    /// An empty system without any cvars.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_shared(self) -> SharedCVars {
        Arc::new(RwLock::new(self))
    }

    /// Register a cvar. The value's variant fixes its type, and `value` is also its default.
    ///
    /// Registering a name that exists already overwrites the value and the default but keeps the
    /// original name and flags, like Java.
    pub fn register(&mut self, name: &str, value: Value, flags: Flags) {
        match self.cvars.get_mut(key(name).as_ref()) {
            Some(existing) => {
                existing.default = value.clone();
                existing.value = value;
            }
            None => {
                let cvar = CVar { name: name.to_string(), default: value.clone(), value, flags, server: false };
                self.cvars.insert(name.to_lowercase(), cvar);
            }
        }
    }

    /// [`register`](Self::register) a cvar that the server owns because it changes the simulation
    /// (gravity, friction...): the console runs reads and writes of it on the server.
    pub fn register_server(&mut self, name: &str, value: Value, flags: Flags) {
        self.register(name, value, flags);
        if let Some(cvar) = self.cvars.get_mut(key(name).as_ref()) {
            cvar.server = true;
        }
    }

    /// [`register`](Self::register) with the default flag, [`Flags::Archive`].
    pub fn register_archive(&mut self, name: &str, value: Value) {
        self.register(name, value, Flags::Archive);
    }

    pub fn get(&self, name: &str) -> Option<&CVar> {
        self.cvars.get(key(name).as_ref())
    }

    pub fn len(&self) -> usize {
        self.cvars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cvars.is_empty()
    }

    fn lookup(&self, name: &str) -> Result<&CVar, CVarError> {
        self.get(name).ok_or_else(|| CVarError::Unknown(name.to_string()))
    }

    fn mismatch(cvar: &CVar, expected: &'static str) -> CVarError {
        CVarError::TypeMismatch { name: cvar.name.clone(), expected, found: cvar.value.type_name() }
    }

    /// Java: `getValueF`.
    pub fn get_f32(&self, name: &str) -> Result<f32, CVarError> {
        match self.lookup(name)? {
            CVar { value: Value::Float(v), .. } => Ok(*v),
            other => Err(Self::mismatch(other, "float")),
        }
    }

    /// Java: `getValueI`.
    pub fn get_i32(&self, name: &str) -> Result<i32, CVarError> {
        match self.lookup(name)? {
            CVar { value: Value::Int(v), .. } => Ok(*v),
            other => Err(Self::mismatch(other, "int")),
        }
    }

    /// Java: `getValueB`.
    pub fn get_bool(&self, name: &str) -> Result<bool, CVarError> {
        match self.lookup(name)? {
            CVar { value: Value::Bool(v), .. } => Ok(*v),
            other => Err(Self::mismatch(other, "bool")),
        }
    }

    /// Java: `getValueS`.
    pub fn get_str(&self, name: &str) -> Result<&str, CVarError> {
        match self.lookup(name)? {
            CVar { value: Value::Str(v), .. } => Ok(v),
            other => Err(Self::mismatch(other, "string")),
        }
    }

    /// Set from text (what the console and files do). The text is parsed as the cvar's type; on
    /// error nothing changes.
    pub fn set(&mut self, name: &str, text: &str) -> Result<(), CVarError> {
        let cvar = self.cvars.get_mut(key(name).as_ref()).ok_or_else(|| CVarError::Unknown(name.to_string()))?;
        cvar.value = cvar.value.parse_like(&cvar.name, text)?;
        Ok(())
    }

    fn set_typed(&mut self, name: &str, value: Value) -> Result<(), CVarError> {
        let cvar = self.cvars.get_mut(key(name).as_ref()).ok_or_else(|| CVarError::Unknown(name.to_string()))?;
        if std::mem::discriminant(&cvar.value) != std::mem::discriminant(&value) {
            return Err(CVarError::TypeMismatch {
                name: cvar.name.clone(),
                expected: value.type_name(),
                found: cvar.value.type_name(),
            });
        }
        cvar.value = value;
        Ok(())
    }

    pub fn set_f32(&mut self, name: &str, value: f32) -> Result<(), CVarError> {
        self.set_typed(name, Value::Float(value))
    }

    pub fn set_i32(&mut self, name: &str, value: i32) -> Result<(), CVarError> {
        self.set_typed(name, Value::Int(value))
    }

    pub fn set_bool(&mut self, name: &str, value: bool) -> Result<(), CVarError> {
        self.set_typed(name, Value::Bool(value))
    }

    pub fn set_str(&mut self, name: &str, value: &str) -> Result<(), CVarError> {
        self.set_typed(name, Value::Str(value.to_string()))
    }

    /// Reset one cvar to its default.
    pub fn reset(&mut self, name: &str) -> Result<(), CVarError> {
        let cvar = self.cvars.get_mut(key(name).as_ref()).ok_or_else(|| CVarError::Unknown(name.to_string()))?;
        cvar.value = cvar.default.clone();
        Ok(())
    }

    /// Names (lowercase) that start with `prefix`, sorted. Java: `getSuggestions`, used for tab completion.
    pub fn suggestions(&self, prefix: &str) -> Vec<String> {
        let prefix = prefix.to_lowercase();
        let mut names: Vec<String> = self.cvars.keys().filter(|k| k.starts_with(&prefix)).cloned().collect();
        names.sort();
        names
    }

    /// Run a console line: `name` reads a cvar, `name value` sets it. A leading `set` is accepted
    /// too (`set gravity 5`). Returns the line the console would print, `cvar gravity has value 5.0`.
    pub fn execute(&mut self, line: &str) -> Result<String, CVarError> {
        let mut words = line.split_whitespace();
        let mut name = words.next().ok_or(CVarError::EmptyCommand)?;
        // `set` is only a prefix if no cvar is actually called `set`.
        if name.eq_ignore_ascii_case("set") && self.get("set").is_none() {
            name = words.next().ok_or(CVarError::EmptyCommand)?;
        }
        if let Some(first) = words.next() {
            let is_string = matches!(self.lookup(name)?.value, Value::Str(_));
            if is_string {
                // Strings take the rest of the line, so they can contain spaces.
                let start = line.find(first).unwrap_or(0);
                let rest = line[start..].trim_end();
                self.set(name, rest)?;
            } else {
                self.set(name, first)?;
            }
        }
        let cvar = self.lookup(name)?;
        Ok(format!("cvar {} has value {}", name.to_lowercase(), cvar.value))
    }

    /// The file contents Java's `save()` would write: `name value` lines, sorted by name, for every
    /// `Archive` cvar that differs from its default and every `InstantSave` cvar.
    pub fn save_string(&self) -> String {
        let mut lines: Vec<(&String, String)> = self
            .cvars
            .iter()
            .filter(|(_, c)| match c.flags {
                Flags::Archive => !c.is_default(),
                Flags::InstantSave => true,
                Flags::Volatile => false,
            })
            .map(|(k, c)| (k, format!("{k} {}\n", c.value)))
            .collect();
        lines.sort_by(|a, b| a.0.cmp(b.0));
        lines.into_iter().map(|(_, l)| l).collect()
    }

    /// Apply file contents. Like Java, only registered cvars are touched; blank lines and lines
    /// with just a name are ignored. Bad lines are reported and skipped, never fatal.
    pub fn load_str(&mut self, text: &str) -> LoadReport {
        let mut report = LoadReport::default();
        for line in text.lines() {
            let mut words = line.split_whitespace();
            let (Some(name), Some(first)) = (words.next(), words.next()) else { continue };
            let Some(cvar) = self.cvars.get(key(name).as_ref()) else {
                report.unknown.push(name.to_string());
                continue;
            };
            // Strings may contain spaces; everything else is a single word, as in Java.
            let data = if matches!(cvar.value, Value::Str(_)) {
                let after_name = line.trim_start()[name.len()..].trim();
                after_name.to_string()
            } else {
                first.to_string()
            };
            match self.set(name, &data) {
                Ok(()) => report.applied += 1,
                Err(_) => report.invalid.push((name.to_string(), data)),
            }
        }
        report
    }

    /// Write [`save_string`](Self::save_string) to a file, replacing it.
    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        std::fs::write(path, self.save_string())
    }

    /// Load a file with [`load_str`](Self::load_str). A missing file is not an error (nothing has been
    /// saved yet) and changes nothing; Java creates an empty file in that case, this does not.
    pub fn load(&mut self, path: impl AsRef<Path>) -> std::io::Result<LoadReport> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(self.load_str(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(LoadReport::default()),
            Err(e) => Err(e),
        }
    }

    fn f(&mut self, name: &str, v: f32) {
        self.register_archive(name, Value::Float(v));
    }
    fn i(&mut self, name: &str, v: i32) {
        self.register_archive(name, Value::Int(v));
    }
    fn b(&mut self, name: &str, v: bool) {
        self.register_archive(name, Value::Bool(v));
    }
    fn s(&mut self, name: &str, v: &str) {
        self.register_archive(name, Value::Str(v.to_string()));
    }

    /// The engine wide system with all defaults of Java's `CVarSystemRoot` (`WE.getCVars()`).
    pub fn root() -> Self {
        let mut c = Self::new();
        c.register_server("gravity", Value::Float(9.81), Flags::Archive);
        c.register_server("worldSpinAngle", Value::Int(-40), Flags::Archive);
        c.b("loadPixmap", false);
        c.f("LEazimutSpeed", 0.00078125);
        c.b("LEnormalMapRendering", false);
        c.i("renderResolutionWidth", 1920);
        c.b("enableLightEngine", true);
        c.f("fogR", 0.3);
        c.f("fogG", 0.4);
        c.f("fogB", 1.0);
        c.f("fogOffset", 2.0);
        c.f("fogFactor", 0.17);
        c.b("enableAutoShade", false);
        c.b("enableScalePrototype", false);
        c.b("enableHSD", true);
        c.b("mapChunkSwitch", true);
        c.b("mapUseChunks", true);
        c.b("DevMode", false);
        c.b("DevDebugRendering", false);
        c.b("editorVisible", false);
        c.register_server("groundBlockID", Value::Int(2), Flags::Archive);
        c.b("preventUnloading", true);
        c.b("shouldLoadMap", true);
        c.b("clearBeforeRendering", true);
        c.i("KeyConsole", KEY_F1);
        c.i("KeySuggestion", KEY_TAB);
        c.f("music", 1.0);
        c.f("sound", 1.0);
        c.i("limitFPS", 60);
        c.register_server("loadEntities", Value::Bool(true), Flags::Archive);
        c.b("enableMinimap", false);
        c.f("walkingAnimationSpeedCorrection", 1.0);
        c.register_server("playerWalkingSpeed", Value::Float(4.0), Flags::Archive);
        c.register_server("timeSpeed", Value::Float(1.0), Flags::Volatile);
        c.register_server("friction", Value::Float(0.001), Flags::Archive);
        c.register_server("playerfriction", Value::Float(0.03), Flags::Archive);
        c.i("soundDecay", 6000);
        c.b("enableControllers", false);
        c.i("controllermacButtonStart", 4);
        c.i("controllermacButtonSelect", 5);
        c.i("controllermacButtonLB", 8);
        c.i("controllermacButtonRB", 9);
        c.i("controllermacButtonX", 13);
        c.i("controllermacButtonB", 12);
        c.i("controllermacButtonA", 11);
        c.i("controllermacButtonY", 14);
        c.i("controllermacAxisRT", 1);
        c.i("controllermacAxisLX", 2);
        c.i("controllermacAxisLY", 3);
        c.i("controllerwindowsButtonA", 0);
        c.i("controllerwindowsButtonB", 1);
        c.i("controllerwindowsButtonX", 2);
        c.i("controllerwindowsButtonY", 3);
        c.i("controllerwindowsButtonLB", 4);
        c.i("controllerwindowsButtonRB", 5);
        c.i("controllerwindowsButtonSelect", 6);
        c.i("controllerwindowsButtonStart", 7);
        c.i("controllerwindowsAxisLY", 0);
        c.i("controllerwindowsAxisLX", 1);
        c.i("controllerwindowsAxisLT", 3);
        c.i("controllerwindowsAxisRT", 4);
        c.i("controllerlinuxButtonStart", 4);
        c.i("controllerlinuxButtonSelect", 5);
        c.i("controllerlinuxButtonLB", 8);
        c.i("controllerlinuxButtonRB", 9);
        c.i("controllerlinuxButtonX", 11);
        c.i("controllerlinuxButtonB", 12);
        c.i("controllerlinuxButtonA", 13);
        c.i("controllerlinuxButtonY", 14);
        c.i("controllerlinuxAxisRT", 1);
        c.i("controllerlinuxAxisLX", 2);
        c.i("controllerlinuxAxisLY", 3);
        c.i("resolutionX", 0);
        c.i("resolutionY", 0);
        c.i("MaxSprites", 3500);
        c.i("CameraLeapRadius", 90);
        c.f("ambientOcclusion", 0.5);
        c.f("MaxDelta", 200.0); // skip a frame's delta if under 5 FPS to prevent glitches
        c.i("numFramesAverageDelta", 10); // frames used for averaging delta
        c.register("loadedMap", Value::Str(String::new()), Flags::Volatile);
        c.s("lastConsoleCommand", "");
        c.i("undohistorySize", 20);
        c.i("mapIndexSpaceSize", 500); // size of the chunk hash map
        c.i("mapMaxMemoryUseBytes", 536_870_912); // 512 MB
        c.b("showMiniMapChunk", false);
        c.i("depthbuffer", 0); // 0 disabled, 1 zbuffer, 2 depth peeling
        c.i("depthSorter", 1); // 0 no sort, 1 toposort, 2 depthsort
        c.b("singleBatchRendering", true);
        c.b("enableVertexLighting", true);
        c.b("enableMultiThreadRendering", false);
        c
    }

    /// The per-map system (`meta.wecvar` in a map directory), defaults of Java's `CVarSystemMap`.
    /// Add game specific map cvars with [`register`](Self::register) afterwards, which is what Java's
    /// `CustomMapCVarRegistration` hook did.
    pub fn map() -> Self {
        let mut c = Self::new();
        c.register("MapVersion", Value::Int(MAP_VERSION), Flags::InstantSave);
        c.i("groundBlockID", 1);
        c.i("chunkBlocksX", 10);
        c.i("chunkBlocksY", 40);
        c.i("chunkBlocksZ", 32);
        c.s("mapname", "");
        c.s("description", "");
        // Not in Java, where the generator was chosen in code. Which map generator this map uses (an
        // id from `generator::generators()`) and its seed; see `generator::generator_from_cvars`.
        c.s("generator", "island");
        c.i("generatorSeed", 1);
        c.register("currentSaveSlot", Value::Int(-1), Flags::Volatile);
        c
    }

    /// The per-save-slot system, defaults of Java's `CVarSystemSave`.
    pub fn save_slot() -> Self {
        let mut c = Self::new();
        c.register("MapVersion", Value::Int(MAP_VERSION), Flags::InstantSave);
        c.f("LEsunAzimuth", 0.0);
        c.f("LEmoonAzimuth", 180.0);
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_has_the_java_defaults() {
        let c = CVarSystem::root();
        // 92 register calls in CVarSystemRoot.java, two of them (controllermacAxisLY and
        // controllerlinuxAxisLY) registered twice with the same value.
        assert_eq!(c.len(), 90);
        assert_eq!(c.get_f32("gravity"), Ok(9.81));
        assert_eq!(c.get_f32("friction"), Ok(0.001));
        assert_eq!(c.get_f32("playerfriction"), Ok(0.03));
        assert_eq!(c.get_f32("playerWalkingSpeed"), Ok(4.0));
        assert_eq!(c.get_f32("LEazimutSpeed"), Ok(0.00078125));
        assert_eq!(c.get_f32("MaxDelta"), Ok(200.0));
        assert_eq!(c.get_i32("groundBlockID"), Ok(2));
        assert_eq!(c.get_i32("mapMaxMemoryUseBytes"), Ok(536_870_912));
        assert_eq!(c.get_i32("worldSpinAngle"), Ok(-40));
        assert_eq!(c.get_i32("KeyConsole"), Ok(131)); // libGDX Input.Keys.F1
        assert_eq!(c.get_i32("KeySuggestion"), Ok(61)); // libGDX Input.Keys.TAB
        assert_eq!(c.get_bool("enableHSD"), Ok(true));
        assert_eq!(c.get_bool("DevMode"), Ok(false));
        assert_eq!(c.get_str("lastConsoleCommand"), Ok(""));
        assert_eq!(c.get("timeSpeed").unwrap().flags(), Flags::Volatile);
        assert_eq!(c.get("loadedMap").unwrap().flags(), Flags::Volatile);
        assert_eq!(c.get("gravity").unwrap().flags(), Flags::Archive);
        // Everything starts at its default, so a fresh root system saves nothing.
        assert_eq!(c.save_string(), "");
    }

    #[test]
    fn systems_are_independent_not_layered() {
        let root = CVarSystem::root();
        let map = CVarSystem::map();
        let save = CVarSystem::save_slot();
        // Same name, different defaults in different systems.
        assert_eq!(root.get_i32("groundBlockID"), Ok(2));
        assert_eq!(map.get_i32("groundBlockID"), Ok(1));
        // No fallback: the map system does not know the root's cvars.
        assert_eq!(map.get_f32("gravity"), Err(CVarError::Unknown("gravity".into())));
        assert_eq!(save.get_f32("LEmoonAzimuth"), Ok(180.0));
        assert_eq!(save.get_f32("LEsunAzimuth"), Ok(0.0));
        assert_eq!(map.get_i32("MapVersion"), Ok(MAP_VERSION));
    }

    #[test]
    fn map_chunk_size_defaults_match_this_crates_chunk_size() {
        let map = CVarSystem::map();
        assert_eq!(map.get_i32("chunkBlocksX"), Ok(crate::CHUNK_SIZE_X));
        assert_eq!(map.get_i32("chunkBlocksY"), Ok(crate::CHUNK_SIZE_Y));
        assert_eq!(map.get_i32("chunkBlocksZ"), Ok(crate::CHUNK_SIZE_Z));
    }

    #[test]
    fn lookups_ignore_case() {
        let c = CVarSystem::root();
        assert_eq!(c.get_f32("GRAVITY"), Ok(9.81));
        assert_eq!(c.get_f32("Gravity"), Ok(9.81));
        assert_eq!(c.get_bool("leNormalMapRendering"), Ok(false));
        // The registered spelling is kept for display.
        assert_eq!(c.get("gravity").unwrap().name(), "gravity");
        assert_eq!(c.get("enablehsd").unwrap().name(), "enableHSD");
    }

    #[test]
    fn typed_getters_report_unknown_names_and_type_mismatches() {
        let c = CVarSystem::root();
        assert_eq!(c.get_f32("nope"), Err(CVarError::Unknown("nope".into())));
        assert_eq!(
            c.get_i32("gravity"),
            Err(CVarError::TypeMismatch { name: "gravity".into(), expected: "int", found: "float" })
        );
        assert_eq!(
            c.get_f32("limitFPS"),
            Err(CVarError::TypeMismatch { name: "limitFPS".into(), expected: "float", found: "int" })
        );
        assert!(c.get_bool("gravity").is_err());
        assert!(c.get_str("gravity").is_err());
        let msg = c.get_i32("gravity").unwrap_err().to_string();
        assert!(msg.contains("gravity") && msg.contains("float") && msg.contains("int"), "{msg}");
    }

    #[test]
    fn set_parses_text_by_type() {
        let mut c = CVarSystem::root();
        c.set("gravity", "5").unwrap();
        assert_eq!(c.get_f32("gravity"), Ok(5.0));
        c.set("gravity", "1.5f").unwrap(); // Java's parseFloat accepts a type suffix
        assert_eq!(c.get_f32("gravity"), Ok(1.5));
        c.set("limitFPS", "-30").unwrap();
        assert_eq!(c.get_i32("limitFPS"), Ok(-30));
        c.set("enableHSD", "0").unwrap();
        assert_eq!(c.get_bool("enableHSD"), Ok(false));
        c.set("enableHSD", "true").unwrap();
        assert_eq!(c.get_bool("enableHSD"), Ok(true));
        c.set("lastConsoleCommand", "hello world").unwrap();
        assert_eq!(c.get_str("lastConsoleCommand"), Ok("hello world"));
    }

    #[test]
    fn bad_text_is_an_error_and_leaves_the_value_alone() {
        let mut c = CVarSystem::root();
        let err = c.set("gravity", "heavy").unwrap_err();
        assert_eq!(err, CVarError::Parse { name: "gravity".into(), expected: "float", input: "heavy".into() });
        assert_eq!(c.get_f32("gravity"), Ok(9.81));
        assert!(c.set("limitFPS", "30.5").is_err());
        assert!(c.set("limitFPS", "").is_err());
        assert_eq!(c.get_i32("limitFPS"), Ok(60));
        // Java would have turned "maybe" into false without telling anyone.
        assert!(c.set("enableHSD", "maybe").is_err());
        assert_eq!(c.get_bool("enableHSD"), Ok(true));
        assert_eq!(c.set("nope", "1"), Err(CVarError::Unknown("nope".into())));
    }

    #[test]
    fn typed_setters_check_the_type() {
        let mut c = CVarSystem::root();
        c.set_f32("gravity", 3.0).unwrap();
        c.set_i32("limitFPS", 30).unwrap();
        c.set_bool("enableHSD", false).unwrap();
        c.set_str("lastConsoleCommand", "x").unwrap();
        assert_eq!(c.get_f32("gravity"), Ok(3.0));
        assert!(matches!(c.set_i32("gravity", 3), Err(CVarError::TypeMismatch { .. })));
        assert!(matches!(c.set_f32("limitFPS", 3.0), Err(CVarError::TypeMismatch { .. })));
        assert_eq!(c.get_f32("gravity"), Ok(3.0));
    }

    #[test]
    fn reregistering_overwrites_value_and_default_but_keeps_flags() {
        let mut c = CVarSystem::new();
        c.register("speed", Value::Float(1.0), Flags::Volatile);
        c.set("speed", "9").unwrap();
        c.register("Speed", Value::Float(2.0), Flags::Archive);
        let cvar = c.get("speed").unwrap();
        assert_eq!(cvar.value(), &Value::Float(2.0));
        assert_eq!(cvar.default_value(), &Value::Float(2.0));
        assert_eq!(cvar.flags(), Flags::Volatile); // kept from the first registration
        assert_eq!(cvar.name(), "speed");
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn reset_restores_the_default() {
        let mut c = CVarSystem::root();
        c.set("gravity", "1").unwrap();
        assert!(!c.get("gravity").unwrap().is_default());
        c.reset("GRAVITY").unwrap();
        assert_eq!(c.get_f32("gravity"), Ok(9.81));
        assert!(c.reset("nope").is_err());
    }

    #[test]
    fn values_print_like_java() {
        assert_eq!(Value::Float(1.0).to_string(), "1.0");
        assert_eq!(Value::Float(200.0).to_string(), "200.0");
        assert_eq!(Value::Float(9.81).to_string(), "9.81");
        assert_eq!(Value::Float(0.00078125).to_string(), "0.00078125");
        assert_eq!(Value::Float(-3.5).to_string(), "-3.5");
        assert_eq!(Value::Int(-40).to_string(), "-40");
        assert_eq!(Value::Bool(true).to_string(), "1");
        assert_eq!(Value::Bool(false).to_string(), "0");
        assert_eq!(Value::Str("a b".into()).to_string(), "a b");
    }

    #[test]
    fn save_writes_changed_archive_cvars_sorted_and_never_volatile_ones() {
        let mut c = CVarSystem::root();
        c.set("gravity", "5").unwrap();
        c.set("enableHSD", "0").unwrap();
        c.set("limitFPS", "144").unwrap();
        c.set("timeSpeed", "2").unwrap(); // volatile: must not appear
        c.set("loadedMap", "cave").unwrap(); // volatile
        c.set("friction", "0.001").unwrap(); // set to its default: must not appear
        assert_eq!(c.save_string(), "enablehsd 0\ngravity 5.0\nlimitfps 144\n");
    }

    #[test]
    fn instant_save_cvars_are_always_written() {
        // Exactly what the Java map system writes to meta.wecvar for a fresh map.
        assert_eq!(CVarSystem::map().save_string(), "mapversion 4\n");
        let mut map = CVarSystem::map();
        map.set("mapname", "Caves").unwrap();
        map.set("currentSaveSlot", "2").unwrap(); // volatile
        assert_eq!(map.save_string(), "mapname Caves\nmapversion 4\n");
        assert_eq!(CVarSystem::save_slot().save_string(), "mapversion 4\n");
    }

    #[test]
    fn save_and_load_round_trip() {
        let mut a = CVarSystem::root();
        a.set("gravity", "4.25").unwrap();
        a.set("limitFPS", "30").unwrap();
        a.set("enableHSD", "0").unwrap();
        a.set("DevMode", "1").unwrap();
        a.set("lastConsoleCommand", "teleport 1 2 3").unwrap();
        let text = a.save_string();

        let mut b = CVarSystem::root();
        let report = b.load_str(&text);
        assert_eq!(report, LoadReport { applied: 5, ..Default::default() });
        assert_eq!(b.get_f32("gravity"), Ok(4.25));
        assert_eq!(b.get_i32("limitFPS"), Ok(30));
        assert_eq!(b.get_bool("enableHSD"), Ok(false));
        assert_eq!(b.get_bool("DevMode"), Ok(true));
        assert_eq!(b.get_str("lastConsoleCommand"), Ok("teleport 1 2 3")); // spaces survive (Java cut after "teleport")
        assert_eq!(b.get_f32("friction"), Ok(0.001)); // untouched
        assert_eq!(b.save_string(), text);
    }

    #[test]
    fn loads_a_file_written_by_the_java_engine() {
        // Hand written in the exact shape Java's save() produces; no non-empty Java-written file
        // exists in the repositories to compare against.
        let java_file = "mapversion 4\nmapname Cave\nchunkblocksx 10\n";
        let mut map = CVarSystem::map();
        let report = map.load_str(java_file);
        assert_eq!(report.applied, 3);
        assert_eq!(map.get_str("mapname"), Ok("Cave"));
        assert_eq!(map.get_i32("MapVersion"), Ok(4));
    }

    #[test]
    fn loading_ignores_unknown_names_blank_lines_and_bare_names_but_reports_bad_values() {
        let mut c = CVarSystem::root();
        let report = c.load_str("\n   \ngravity\nfuturecvar 12\ngravity abc\nlimitfps 99\nLIMITFPS 100\n");
        assert_eq!(c.get_f32("gravity"), Ok(9.81)); // bare name and bad value did nothing
        assert_eq!(c.get_i32("limitFPS"), Ok(100)); // later lines win, names ignore case
        assert_eq!(report.applied, 2);
        assert_eq!(report.unknown, vec!["futurecvar".to_string()]);
        assert_eq!(report.invalid, vec![("gravity".to_string(), "abc".to_string())]);
    }

    #[test]
    fn loading_does_not_change_defaults() {
        let mut c = CVarSystem::root();
        c.load_str("gravity 1\n");
        assert_eq!(c.get("gravity").unwrap().default_value(), &Value::Float(9.81));
        assert!(!c.get("gravity").unwrap().is_default());
    }

    #[test]
    fn execute_reads_and_sets_like_the_console() {
        let mut c = CVarSystem::root();
        assert_eq!(c.execute("gravity").unwrap(), "cvar gravity has value 9.81");
        assert_eq!(c.execute("gravity 5").unwrap(), "cvar gravity has value 5.0");
        assert_eq!(c.get_f32("gravity"), Ok(5.0));
        assert_eq!(c.execute("set gravity 4").unwrap(), "cvar gravity has value 4.0");
        assert_eq!(c.execute("SET Gravity 3  extra ignored").unwrap(), "cvar gravity has value 3.0");
        assert_eq!(c.execute("  enableHSD   0 ").unwrap(), "cvar enablehsd has value 0");
        assert_eq!(c.execute("lastConsoleCommand teleport 1 2 3").unwrap(), "cvar lastconsolecommand has value teleport 1 2 3");
        assert_eq!(c.get_str("lastConsoleCommand"), Ok("teleport 1 2 3"));
    }

    #[test]
    fn execute_reports_errors_without_changing_anything() {
        let mut c = CVarSystem::root();
        assert_eq!(c.execute(""), Err(CVarError::EmptyCommand));
        assert_eq!(c.execute("   "), Err(CVarError::EmptyCommand));
        assert_eq!(c.execute("set"), Err(CVarError::EmptyCommand));
        assert_eq!(c.execute("nosuch 1"), Err(CVarError::Unknown("nosuch".into())));
        assert_eq!(c.execute("set nosuch"), Err(CVarError::Unknown("nosuch".into())));
        assert!(matches!(c.execute("gravity heavy"), Err(CVarError::Parse { .. })));
        assert_eq!(c.get_f32("gravity"), Ok(9.81));
    }

    #[test]
    fn a_cvar_named_set_is_not_shadowed_by_the_set_prefix() {
        let mut c = CVarSystem::new();
        c.register_archive("set", Value::Int(1));
        assert_eq!(c.execute("set 7").unwrap(), "cvar set has value 7");
        assert_eq!(c.get_i32("set"), Ok(7));
    }

    #[test]
    fn suggestions_complete_by_prefix_sorted_and_case_insensitive() {
        let c = CVarSystem::root();
        assert_eq!(c.suggestions("player"), vec!["playerfriction", "playerwalkingspeed"]);
        assert_eq!(c.suggestions("PLAYER"), vec!["playerfriction", "playerwalkingspeed"]);
        assert_eq!(c.suggestions("fog"), vec!["fogb", "fogfactor", "fogg", "fogoffset", "fogr"]);
        assert!(c.suggestions("zzz").is_empty());
        assert_eq!(c.suggestions("").len(), c.len());
    }

    #[test]
    fn files_round_trip_and_a_missing_file_is_fine() {
        let dir = std::env::temp_dir().join(format!("wurfel-cvar-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("engine.wecvars");

        let mut missing = CVarSystem::root();
        assert_eq!(missing.load(&path).unwrap(), LoadReport::default());
        assert!(!path.exists(), "loading must not create the file");

        let mut a = CVarSystem::root();
        a.set("gravity", "2").unwrap();
        a.save(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "gravity 2.0\n");

        let mut b = CVarSystem::root();
        assert_eq!(b.load(&path).unwrap().applied, 1);
        assert_eq!(b.get_f32("gravity"), Ok(2.0));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn can_be_shared_between_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CVarSystem>();
        assert_send_sync::<SharedCVars>();

        let shared = CVarSystem::root().into_shared();
        let writer = {
            let shared = shared.clone();
            std::thread::spawn(move || shared.write().unwrap().set_f32("gravity", 1.0).unwrap())
        };
        writer.join().unwrap();
        assert_eq!(shared.read().unwrap().get_f32("gravity"), Ok(1.0));
    }
}
