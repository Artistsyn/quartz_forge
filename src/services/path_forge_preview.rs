//! Preview of a PathForge background inside the editor.
//!
//! Live mode runs PathForge's own runtime (`path_forge::runtime::Runtime`, the
//! same renderer the game's plugin uses) at a small size, so the preview is the
//! game's picture, walking, and a journey can be walked through its stops with
//! the same "walk on" / "choose a branch" the game's events send. Frames mode
//! cycles the exported start loop's PNGs.
//!
//! path_forge is linked without its studio (no GUI or GPU dependencies); it is
//! a self-contained library, unlike quartz, so it does not tie the editor to
//! the engine's state.

use crate::core::project::{PathForgeBackground, PathForgeMode};
use path_forge::journey::Journey;
use path_forge::runtime::{Runtime, WalkState};
use path_forge::scene::transition::Branch;
use std::path::{Path, PathBuf};

pub struct PathForgePreview {
    /// What this preview was opened from: (resolved source, mode).
    key: (PathBuf, PathForgeMode),
    source: Source,
    pub error: Option<String>,
    /// Journey stops and where each leads, for the panel.
    pub stops: Vec<String>,
    /// The latest plan or choice, in words.
    pub note: Option<String>,
}

enum Source {
    Live(Box<Runtime>),
    Frames { files: Vec<PathBuf>, fps: f32, at: f32, cache: Option<(usize, image::RgbaImage)> },
    None,
}

/// The source as a path under the project root, or as given for `preset:`.
pub fn resolve(pf: &PathForgeBackground, project_root: Option<&Path>) -> PathBuf {
    let s = pf.source.trim();
    if s.starts_with("preset:") || Path::new(s).is_absolute() {
        return PathBuf::from(s);
    }
    project_root.map(|r| r.join(s)).unwrap_or_else(|| PathBuf::from(s))
}

fn describe(j: &Journey) -> Vec<String> {
    use path_forge::journey::Next;
    j.stops.iter().map(|(id, st)| {
        let next = match &st.next {
            Next::End => "end".to_owned(),
            Next::Go { to, .. } => format!("→ {to}"),
            Next::Fork { left, right, .. } => format!("fork: left {left} / right {right}"),
        };
        format!("{}{id} ({}) {next}", if *id == j.start { "▶ " } else { "" }, st.scene)
    }).collect()
}

impl PathForgePreview {
    pub fn open(pf: &PathForgeBackground, project_root: Option<&Path>) -> PathForgePreview {
        let path = resolve(pf, project_root);
        let mut me = PathForgePreview { key: (path.clone(), pf.mode), source: Source::None, error: None, stops: Vec::new(), note: None };
        let text = path.to_string_lossy().into_owned();
        let res: Result<Source, String> = match pf.mode {
            PathForgeMode::Live if text.ends_with(".journey.json") => {
                if let Ok(j) = Journey::load(&path) {
                    me.stops = describe(&j);
                    let probs = j.problems(path.parent());
                    if !probs.is_empty() { me.error = Some(probs.join("; ")); }
                }
                Runtime::from_journey(&path).map(|r| Source::Live(Box::new(r)))
            }
            PathForgeMode::Live => path_forge::journey::load_place(&text, None).map(|(s, d)| Source::Live(Box::new(Runtime::new(s, d)))),
            PathForgeMode::Frames => frames_of(&path),
        };
        match res { Ok(s) => me.source = s, Err(e) => me.error = Some(e) }
        me
    }

    /// Still the preview of `pf` (same file and mode)?
    pub fn is_for(&self, pf: &PathForgeBackground, project_root: Option<&Path>) -> bool {
        self.key == (resolve(pf, project_root), pf.mode)
    }

    /// Walk on `dt` seconds and draw a `w` x `h` frame (RGBA8).
    pub fn frame(&mut self, dt: f32, speed: Option<f32>, w: u32, h: u32) -> Option<(u32, u32, Vec<u8>)> {
        match &mut self.source {
            Source::Live(rt) => {
                let v = speed.unwrap_or_else(|| rt.state().speed);
                rt.step(dt, v);
                Some((w, h, rt.frame(w, h).to_vec()))
            }
            Source::Frames { files, fps, at, cache } => {
                *at = (*at + dt * *fps) % files.len().max(1) as f32;
                let k = *at as usize;
                if cache.as_ref().map(|c| c.0) != Some(k) {
                    *cache = image::open(&files[k]).ok().map(|i| (k, i.to_rgba8()));
                }
                cache.as_ref().map(|(_, i)| (i.width(), i.height(), i.as_raw().clone()))
            }
            Source::None => None,
        }
    }

    pub fn state(&self) -> Option<WalkState> {
        match &self.source { Source::Live(rt) => Some(rt.state()), _ => None }
    }

    /// The game's `next`: walk on to where the journey leads.
    pub fn next(&mut self) {
        if let Source::Live(rt) = &mut self.source {
            match rt.go() {
                Ok(p) => self.note = Some(format!("{}: {}{}", p.kind, p.notes.join(" "), if p.warnings.is_empty() { String::new() } else { format!(" ⚠ {}", p.warnings.join(" ⚠ ")) })),
                Err(e) => self.note = Some(e),
            }
        }
    }

    /// The game's `choose:left|right`.
    pub fn choose(&mut self, b: Branch) {
        if let Source::Live(rt) = &mut self.source {
            self.note = Some(if rt.choose(b) { format!("took the {} branch", b.name()) } else { "no fork to choose here (or too late)".to_owned() });
        }
    }

    /// Back to the start.
    pub fn restart(&mut self, pf: &PathForgeBackground, project_root: Option<&Path>) { *self = PathForgePreview::open(pf, project_root); }
}

/// The start loop of an export (`<name>.journey.json` or a loop `<name>.json`).
fn frames_of(path: &Path) -> Result<Source, String> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let read = |p: &Path| -> Result<serde_json::Value, String> {
        serde_json::from_str(&std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("{}: {e}", p.display()))
    };
    let mut meta = read(path)?;
    if let Some(stops) = meta.get("stops") {
        let start = meta["start"].as_str().unwrap_or_default();
        let lp = stops[start]["loop"].as_array().and_then(|a| a.iter().filter_map(|v| v.as_str()).find(|f| f.ends_with(".json")).map(str::to_owned))
            .ok_or_else(|| format!("start stop `{start}` names no loop"))?;
        meta = read(&dir.join(lp))?;
    }
    let folder = meta["files"].as_array().and_then(|a| a.iter().filter_map(|v| v.as_str()).find(|f| f.ends_with("_frames")).map(str::to_owned))
        .ok_or("no _frames folder: export with the png format")?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir.join(&folder)).map_err(|e| format!("{folder}: {e}"))?
        .filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "png")).collect();
    files.sort();
    if files.is_empty() { return Err(format!("{folder} has no frames")); }
    Ok(Source::Frames { files, fps: meta["fps"].as_f64().unwrap_or(24.0) as f32, at: 0.0, cache: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_preview_walks_a_preset_and_a_journey() {
        let mut p = PathForgePreview::open(&PathForgeBackground::new("preset:Forest Path"), None);
        assert!(p.error.is_none(), "{:?}", p.error);
        let (w, h, px) = p.frame(0.1, None, 27, 48).unwrap();
        assert_eq!((w, h, px.len()), (27, 48, 27 * 48 * 4));

        let dir = std::env::temp_dir().join(format!("qf_pf_preview_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("g.journey.json"), r#"{"version":1,"name":"g","start":"a","stops":{
            "a":{"scene":"preset:Stone Dungeon","next":{"Go":{"to":"b","transition":{}}}},
            "b":{"scene":"preset:Forest Path","next":"End"}}}"#).unwrap();
        let pf = PathForgeBackground::new("g.journey.json");
        let mut p = PathForgePreview::open(&pf, Some(&dir));
        assert!(p.error.is_none(), "{:?}", p.error);
        assert_eq!(p.stops.len(), 2);
        assert!(p.is_for(&pf, Some(&dir)));
        p.next();
        assert!(p.note.as_deref().unwrap_or("").starts_with("Doorway"), "{:?}", p.note);
        for _ in 0..200 { p.frame(0.25, Some(20.0), 18, 32); }
        assert_eq!(p.state().unwrap().stop.as_deref(), Some("b"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_source_is_an_error_not_a_panic() {
        let p = PathForgePreview::open(&PathForgeBackground::new("nope/missing.json"), None);
        assert!(p.error.is_some());
    }
}
