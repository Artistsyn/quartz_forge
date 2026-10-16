//! PathForge's runtime library, loaded when a preview first opens: its C interface
//! (path_forge/runtime/include/path_forge.h) wrapped for the preview. Looked for in
//! `PATH_FORGE_RUNTIME` (the library file), beside the editor, then in the PathForge checkout
//! build.rs found (`target/release`, `target/debug`).

use serde::Deserialize;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The oldest interface this plugin can use (pf_runtime_open_place, pf_runtime_go_plan and
/// pf_journey_outline came in 2).
const NEEDS_ABI: u32 = 2;

type OpenPlace = unsafe extern "C" fn(*const c_char, *const c_char, *mut c_char, usize) -> *mut c_void;
type OpenJourney = unsafe extern "C" fn(*const c_char, *mut c_char, usize) -> *mut c_void;
type Free = unsafe extern "C" fn(*mut c_void);
type Step = unsafe extern "C" fn(*mut c_void, f32, f32) -> i32;
type Frame = unsafe extern "C" fn(*mut c_void, u32, u32, *mut u8, usize) -> i32;
type GoPlan = unsafe extern "C" fn(*mut c_void, *mut c_char, usize) -> i32;
type State = unsafe extern "C" fn(*const c_void, *mut c_char, usize) -> i32;
type Choose = unsafe extern "C" fn(*mut c_void, i32) -> i32;
type SetF = unsafe extern "C" fn(*mut c_void, f32) -> i32;
type Intro = unsafe extern "C" fn(*mut c_void) -> i32;
type Outline = unsafe extern "C" fn(*const c_char, *mut c_char, usize) -> i32;
type Abi = unsafe extern "C" fn() -> u32;

pub struct Lib {
    _lib: libloading::Library,
    pub path: PathBuf,
    open_place: OpenPlace,
    open_journey: OpenJourney,
    free: Free,
    step: Step,
    frame: Frame,
    go_plan: GoPlan,
    state: State,
    choose: Choose,
    set_hour: SetF,
    set_day_length: SetF,
    restart_intro: Intro,
    outline: Outline,
}

static LIB: OnceLock<Lib> = OnceLock::new();

/// The library's file name here (libpath_forge_runtime.so, .dylib, path_forge_runtime.dll).
pub fn file_name() -> String {
    format!("{}path_forge_runtime{}", std::env::consts::DLL_PREFIX, std::env::consts::DLL_SUFFIX)
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("PATH_FORGE_RUNTIME") { out.push(PathBuf::from(p)); }
    if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) { out.push(d.join(file_name())); }
    let dir = env!("QF_PATH_FORGE_DIR");
    if !dir.is_empty() {
        for p in ["release", "debug"] { out.push(Path::new(dir).join("target").join(p).join(file_name())); }
    }
    out
}

/// How to get the preview working, for when the library is missing.
pub fn how_to_install() -> String {
    let dir = env!("QF_PATH_FORGE_DIR");
    if dir.is_empty() {
        format!("PathForge is not installed: clone it beside this editor (or set PATH_FORGE_DIR and rebuild), build its runtime with `cargo build --release -p path_forge_runtime`, or point PATH_FORGE_RUNTIME at a built {}.", file_name())
    } else {
        format!("PathForge's runtime library is not built: run `cargo build --release -p path_forge_runtime` in {dir} (or point PATH_FORGE_RUNTIME at a built {}).", file_name())
    }
}

/// The library, loaded on first use. Not found is not cached: build it and the next preview
/// that opens finds it.
pub fn lib() -> Result<&'static Lib, String> {
    if let Some(l) = LIB.get() { return Ok(l); }
    let path = candidates().into_iter().find(|p| p.exists()).ok_or_else(how_to_install)?;
    let l = unsafe { load(&path) }.map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(LIB.get_or_init(|| l))
}

unsafe fn load(path: &Path) -> Result<Lib, String> {
    let lib = libloading::Library::new(path).map_err(|e| e.to_string())?;
    let abi: Abi = *lib.get::<Abi>(b"pf_abi_version\0").map_err(|_| format!("too old a PathForge (no pf_abi_version; this editor needs interface {NEEDS_ABI})"))?;
    let v = abi();
    if v < NEEDS_ABI { return Err(format!("PathForge's interface is {v}; this editor needs {NEEDS_ABI} or later: update and rebuild PathForge")); }
    macro_rules! sym { ($t:ty, $n:literal) => { *lib.get::<$t>(concat!($n, "\0").as_bytes()).map_err(|e| format!("{}: {e}", $n))? }; }
    Ok(Lib {
        open_place: sym!(OpenPlace, "pf_runtime_open_place"),
        open_journey: sym!(OpenJourney, "pf_runtime_open_journey"),
        free: sym!(Free, "pf_runtime_free"),
        step: sym!(Step, "pf_runtime_step"),
        frame: sym!(Frame, "pf_runtime_frame"),
        go_plan: sym!(GoPlan, "pf_runtime_go_plan"),
        state: sym!(State, "pf_runtime_state"),
        choose: sym!(Choose, "pf_runtime_choose"),
        set_hour: sym!(SetF, "pf_runtime_set_hour"),
        set_day_length: sym!(SetF, "pf_runtime_set_day_length"),
        restart_intro: sym!(Intro, "pf_runtime_restart_intro"),
        outline: sym!(Outline, "pf_journey_outline"),
        path: path.to_path_buf(),
        _lib: lib,
    })
}

/// A C function's JSON (or error) text: called with a buffer, again with a larger one if the
/// text did not fit. Ok for a non-negative return, Err with the text for -2.
fn json_call(f: impl Fn(*mut c_char, usize) -> i32) -> Result<String, String> {
    let mut buf = vec![0 as c_char; 16 * 1024];
    let mut n = f(buf.as_mut_ptr(), buf.len());
    if n >= 0 && n as usize >= buf.len() {
        buf = vec![0 as c_char; n as usize + 1];
        n = f(buf.as_mut_ptr(), buf.len());
    }
    let text = unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned();
    if n >= 0 { Ok(text) } else if n == -1 { Err("no runtime".into()) } else { Err(text) }
}

fn cstr(s: &str) -> Result<CString, String> { CString::new(s).map_err(|_| format!("{s:?} holds a NUL")) }

/// The day now (`pf_runtime_state`'s "day").
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct DayState { pub hour: f32, pub altitude: f32, pub is_day: bool, pub daylight: f32, pub phase: String }

/// Where the walk stands (`pf_runtime_state`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct WalkState {
    pub scene: String,
    pub distance: f32,
    pub time: f32,
    pub stop: Option<String>,
    pub progress: Option<f32>,
    pub choose_within: Option<f32>,
    pub speed: f32,
    pub end: bool,
    pub day: Option<DayState>,
}

/// What `go` planned (`pf_runtime_go_plan`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Plan { pub kind: String, pub notes: Vec<String>, pub warnings: Vec<String> }

/// A journey's outline (`pf_journey_outline`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct JourneyOutline { pub start: String, pub stops: Vec<Stop>, pub problems: Vec<String> }
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Stop { pub id: String, pub scene: String, pub next: serde_json::Value }

pub fn journey_outline(path: &Path) -> Result<JourneyOutline, String> {
    let l = lib()?;
    let p = cstr(&path.to_string_lossy())?;
    let text = json_call(|b, n| unsafe { (l.outline)(p.as_ptr(), b, n) })?;
    serde_json::from_str(&text).map_err(|e| format!("journey outline: {e}"))
}

/// One of PathForge's runtimes, freed when dropped.
pub struct Runtime { lib: &'static Lib, ptr: *mut c_void }

impl Runtime {
    fn opened(lib: &'static Lib, ptr: *mut c_void, err: &[c_char]) -> Result<Runtime, String> {
        if ptr.is_null() { return Err(unsafe { CStr::from_ptr(err.as_ptr()) }.to_string_lossy().into_owned()); }
        Ok(Runtime { lib, ptr })
    }

    /// A scene file or `preset:Name` (files relative to `dir`).
    pub fn open_place(name: &str, dir: Option<&Path>) -> Result<Runtime, String> {
        let l = lib()?;
        let (n, d) = (cstr(name)?, dir.map(|d| cstr(&d.to_string_lossy())).transpose()?);
        let mut err = vec![0 as c_char; 1024];
        let ptr = unsafe { (l.open_place)(n.as_ptr(), d.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()), err.as_mut_ptr(), err.len()) };
        Runtime::opened(l, ptr, &err)
    }

    pub fn open_journey(path: &Path) -> Result<Runtime, String> {
        let l = lib()?;
        let p = cstr(&path.to_string_lossy())?;
        let mut err = vec![0 as c_char; 1024];
        let ptr = unsafe { (l.open_journey)(p.as_ptr(), err.as_mut_ptr(), err.len()) };
        Runtime::opened(l, ptr, &err)
    }

    pub fn step(&mut self, dt: f32, speed: f32) { unsafe { (self.lib.step)(self.ptr, dt, speed) }; }

    /// A `w` x `h` RGBA8 frame of the walk.
    pub fn frame(&mut self, w: u32, h: u32) -> Option<Vec<u8>> {
        let mut px = vec![0u8; w as usize * h as usize * 4];
        (unsafe { (self.lib.frame)(self.ptr, w, h, px.as_mut_ptr(), px.len()) } == 0).then_some(px)
    }

    pub fn go(&mut self) -> Result<Plan, String> {
        let text = json_call(|b, n| unsafe { (self.lib.go_plan)(self.ptr, b, n) })?;
        serde_json::from_str(&text).map_err(|e| format!("plan: {e}"))
    }

    pub fn state(&self) -> WalkState {
        json_call(|b, n| unsafe { (self.lib.state)(self.ptr, b, n) }).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    /// Take a branch of the fork ahead (right or left): whether it was taken.
    pub fn choose(&mut self, right: bool) -> bool { unsafe { (self.lib.choose)(self.ptr, right as i32) == 1 } }

    /// The hour on every scene drawn (None: each scene's own).
    pub fn set_hour(&mut self, hour: Option<f32>) { unsafe { (self.lib.set_hour)(self.ptr, hour.unwrap_or(-1.0)) }; }

    pub fn set_day_length(&mut self, seconds: f32) { unsafe { (self.lib.set_day_length)(self.ptr, seconds) }; }

    pub fn restart_intro(&mut self) { unsafe { (self.lib.restart_intro)(self.ptr) }; }
}

impl Drop for Runtime {
    fn drop(&mut self) { unsafe { (self.lib.free)(self.ptr) } }
}
