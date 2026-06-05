use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::core::layout::ProjectLayoutPaths;
use crate::core::project::{EditorProjectState, ProjectManifest};
use crate::core::quartz_domain::CanvasOrientation;

const MANAGED_MAIN_MARKER: &str = "// quartz_forge-managed: main entrypoint";
const MANAGED_LIB_MARKER: &str = "// quartz_forge-managed: build_app scaffold";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectSyncFileRecord {
    rel_path: String,
    content_hash: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectSyncSnapshot {
    generated_at_utc: String,
    manifest_hash: u64,
    manifest: ProjectManifest,
    files: Vec<ProjectSyncFileRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSyncStatus {
    MissingSnapshot,
    InSync,
    SavedProjectAheadOfFiles,
    FilesChangedOutsideQuartzForge,
    Diverged,
}

#[derive(Debug, Clone)]
pub struct ProjectSyncReport {
    pub status: ProjectSyncStatus,
    pub summary: String,
    pub modified_files: Vec<String>,
    pub missing_files: Vec<String>,
    pub extra_files: Vec<String>,
    pub can_restore_project_from_last_export: bool,
    pub can_rewrite_files_from_project: bool,
    pub snapshot_generated_at_utc: Option<String>,
}

impl ProjectSyncReport {
    pub fn needs_user_action(&self) -> bool {
        !matches!(self.status, ProjectSyncStatus::MissingSnapshot | ProjectSyncStatus::InSync)
    }
}

pub fn create_new_project(project_name: String, root: &Path) -> Result<EditorProjectState> {
    let layout = ProjectLayoutPaths::from_root(root);
    layout.ensure_dirs()?;

    let state = EditorProjectState::new(project_name);
    save_manifest(&state.manifest, &layout)?;
    ensure_runtime_scaffold(&state, root)?;
    Ok(state)
}

pub fn load_project(root: &Path) -> Result<EditorProjectState> {
    let layout = ProjectLayoutPaths::from_root(root);
    let raw = std::fs::read_to_string(&layout.manifest_path)
        .with_context(|| format!("failed to read {}", layout.manifest_path.display()))?;
    let mut manifest: ProjectManifest = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", layout.manifest_path.display()))?;
    manifest.ensure_default_scene();

    let active_scene_index = manifest.active_scene_index().unwrap_or(0);
    Ok(EditorProjectState {
        manifest,
        active_scene_index,
        dirty: false,
    })
}

pub fn load_project_with_sync(root: &Path) -> Result<(EditorProjectState, ProjectSyncReport)> {
    let state = load_project(root)?;
    let report = validate_project_sync(&state, root)?;
    Ok((state, report))
}

pub fn save_project(state: &mut EditorProjectState, root: &Path) -> Result<()> {
    let layout = ProjectLayoutPaths::from_root(root);
    layout.ensure_dirs()?;
    state.manifest.touch_saved_time();
    save_manifest(&state.manifest, &layout)?;
    ensure_runtime_scaffold(state, root)?;
    state.dirty = false;
    Ok(())
}

pub fn write_sync_snapshot(state: &EditorProjectState, root: &Path) -> Result<()> {
    let layout = ProjectLayoutPaths::from_root(root);
    layout.ensure_dirs()?;

    let manifest_hash = manifest_hash(&state.manifest)?;
    let files = collect_sync_tracked_files(root)?
        .into_iter()
        .filter_map(|rel_path| {
            let path = root.join(&rel_path);
            let Ok(bytes) = std::fs::read(&path) else {
                return None;
            };
            Some(ProjectSyncFileRecord {
                rel_path,
                content_hash: fnv1a_hash(&bytes),
            })
        })
        .collect::<Vec<_>>();

    let snapshot = ProjectSyncSnapshot {
        generated_at_utc: Utc::now().to_rfc3339(),
        manifest_hash,
        manifest: state.manifest.clone(),
        files,
    };

    std::fs::write(
        &layout.sync_snapshot_path,
        serde_json::to_string_pretty(&snapshot).context("failed to serialize sync snapshot")?,
    )
    .with_context(|| format!("failed to write {}", layout.sync_snapshot_path.display()))
}

pub fn validate_project_sync(state: &EditorProjectState, root: &Path) -> Result<ProjectSyncReport> {
    let Some(snapshot) = read_sync_snapshot(root)? else {
        return Ok(ProjectSyncReport {
            status: ProjectSyncStatus::MissingSnapshot,
            summary: "No Quartz Forge sync snapshot exists yet. Export generated files once to establish reconciliation metadata.".to_owned(),
            modified_files: Vec::new(),
            missing_files: Vec::new(),
            extra_files: Vec::new(),
            can_restore_project_from_last_export: false,
            can_rewrite_files_from_project: true,
            snapshot_generated_at_utc: None,
        });
    };

    let manifest_matches_snapshot = manifest_hash(&state.manifest)? == snapshot.manifest_hash;

    let mut current_files = collect_sync_tracked_files(root)?
        .into_iter()
        .filter_map(|rel_path| {
            let path = root.join(&rel_path);
            let Ok(bytes) = std::fs::read(&path) else {
                return None;
            };
            Some((rel_path, fnv1a_hash(&bytes)))
        })
        .collect::<std::collections::BTreeMap<_, _>>();

    let mut modified_files = Vec::new();
    let mut missing_files = Vec::new();

    for tracked in &snapshot.files {
        match current_files.remove(&tracked.rel_path) {
            Some(hash) if hash == tracked.content_hash => {}
            Some(_) => modified_files.push(tracked.rel_path.clone()),
            None => missing_files.push(tracked.rel_path.clone()),
        }
    }

    let mut extra_files = current_files.into_keys().collect::<Vec<_>>();
    modified_files.sort();
    missing_files.sort();
    extra_files.sort();

    let files_match_snapshot = modified_files.is_empty() && missing_files.is_empty() && extra_files.is_empty();

    let (status, summary, can_restore_project_from_last_export) = match (manifest_matches_snapshot, files_match_snapshot) {
        (true, true) => (
            ProjectSyncStatus::InSync,
            "Project save state matches the last exported Quartz Forge file set.".to_owned(),
            false,
        ),
        (false, true) => (
            ProjectSyncStatus::SavedProjectAheadOfFiles,
            "The saved project data differs from the last exported file set, but the tracked files still match the last export. You can rewrite files from the saved project state or restore the project save state to the last exported files.".to_owned(),
            true,
        ),
        (true, false) => (
            ProjectSyncStatus::FilesChangedOutsideQuartzForge,
            "Tracked project files changed outside the saved Quartz Forge project state. Review the mismatch before exporting again.".to_owned(),
            false,
        ),
        (false, false) => (
            ProjectSyncStatus::Diverged,
            "The saved project data and tracked project files both diverged from the last exported sync point. Review the mismatch before continuing.".to_owned(),
            false,
        ),
    };

    Ok(ProjectSyncReport {
        status,
        summary,
        modified_files,
        missing_files,
        extra_files,
        can_restore_project_from_last_export,
        can_rewrite_files_from_project: true,
        snapshot_generated_at_utc: Some(snapshot.generated_at_utc),
    })
}

pub fn restore_project_from_sync_snapshot(root: &Path) -> Result<EditorProjectState> {
    let Some(snapshot) = read_sync_snapshot(root)? else {
        anyhow::bail!("no Quartz Forge sync snapshot exists for this project")
    };

    let mut manifest = snapshot.manifest;
    manifest.ensure_default_scene();
    let active_scene_index = manifest.active_scene_index().unwrap_or(0);
    Ok(EditorProjectState {
        manifest,
        active_scene_index,
        dirty: false,
    })
}

pub fn ensure_runtime_scaffold(state: &EditorProjectState, root: &Path) -> Result<()> {
    let src_dir = root.join("src");
    let scenes_dir = src_dir.join("scenes");
    let scripts_dir = src_dir.join("scripts");
    let resources_dir = root.join("resources");
    let assets_dir = root.join("assets");

    std::fs::create_dir_all(&src_dir)
        .with_context(|| format!("failed to create {}", src_dir.display()))?;
    std::fs::create_dir_all(&scenes_dir)
        .with_context(|| format!("failed to create {}", scenes_dir.display()))?;
    std::fs::create_dir_all(&scripts_dir)
        .with_context(|| format!("failed to create {}", scripts_dir.display()))?;
    std::fs::create_dir_all(&resources_dir)
        .with_context(|| format!("failed to create {}", resources_dir.display()))?;
    std::fs::create_dir_all(&assets_dir)
        .with_context(|| format!("failed to create {}", assets_dir.display()))?;

    ensure_gitignore(root)?;
    ensure_cargo_toml(state, root)?;
    ensure_main_rs(state, root)?;
    ensure_lib_rs(state, root)?;

    Ok(())
}

fn save_manifest(manifest: &ProjectManifest, layout: &ProjectLayoutPaths) -> Result<()> {
    let content = serde_json::to_string_pretty(manifest).context("failed to serialize manifest")?;
    std::fs::write(&layout.manifest_path, content)
        .with_context(|| format!("failed to write {}", layout.manifest_path.display()))
}

fn read_sync_snapshot(root: &Path) -> Result<Option<ProjectSyncSnapshot>> {
    let layout = ProjectLayoutPaths::from_root(root);
    if !layout.sync_snapshot_path.exists() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(&layout.sync_snapshot_path)
        .with_context(|| format!("failed to read {}", layout.sync_snapshot_path.display()))?;
    let snapshot: ProjectSyncSnapshot = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", layout.sync_snapshot_path.display()))?;
    Ok(Some(snapshot))
}

fn manifest_hash(manifest: &ProjectManifest) -> Result<u64> {
    let bytes = serde_json::to_vec(manifest).context("failed to serialize manifest for hashing")?;
    Ok(fnv1a_hash(&bytes))
}

fn fnv1a_hash(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;

    let mut hash = OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

fn collect_sync_tracked_files(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    collect_rs_files_recursive(&root.join("src"), root, &mut files)?;
    for file in ["Cargo.toml", ".gitignore"] {
        if root.join(file).exists() {
            files.push(file.to_owned());
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn collect_rs_files_recursive(dir: &Path, root: &Path, out: &mut Vec<String>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("failed to read directory {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files_recursive(&path, root, out)?;
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }

        let rel = path
            .strip_prefix(root)
            .with_context(|| format!("failed to relativize {}", path.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        out.push(rel);
    }

    Ok(())
}

fn ensure_gitignore(root: &Path) -> Result<()> {
    let path = root.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == "/target") {
        return Ok(());
    }

    let mut merged = existing;
    if !merged.is_empty() && !merged.ends_with('\n') {
        merged.push('\n');
    }
    merged.push_str("/target\n");
    std::fs::write(&path, merged)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn ensure_cargo_toml(state: &EditorProjectState, root: &Path) -> Result<()> {
    let path = root.join("Cargo.toml");
    if path.exists() {
        return Ok(());
    }

    let crate_name = slugify_crate_name(&state.manifest.project_name);
    let cargo = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nramp = {{ package = \"ramp2\", path = \"../ramp\" }}\nquartz = {{ path = \"../quartz\" }}\n",
        crate_name
    );
    std::fs::write(&path, cargo)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn ensure_main_rs(state: &EditorProjectState, root: &Path) -> Result<()> {
    let path = root.join("src").join("main.rs");
    let crate_name = slugify_crate_name(&state.manifest.project_name);
    let main_rs = managed_main_rs(&crate_name);

    if path.exists() {
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        if !should_rewrite_managed_main(&existing) || existing == main_rs {
            return Ok(());
        }
    }

    std::fs::write(&path, main_rs)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn ensure_lib_rs(state: &EditorProjectState, root: &Path) -> Result<()> {
    let path = root.join("src").join("lib.rs");
    let scene_module_rel = state
        .manifest
        .scenes
        .get(state.active_scene_index)
        .map(|scene| scene.source_file.clone())
        .unwrap_or_else(|| "src/scripts/main_scene.rs".to_owned());
    let scene_module_path = scene_module_rel.strip_prefix("src/").unwrap_or(&scene_module_rel);

    let canvas_mode = active_scene_canvas_mode_expr(state);
    let lib_rs = managed_lib_rs(&scene_module_path.replace('\\', "/"), canvas_mode);

    if path.exists() {
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        if !should_rewrite_managed_lib(&existing) || existing == lib_rs {
            return Ok(());
        }
    }

    std::fs::write(&path, lib_rs)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn active_scene_canvas_mode_expr(state: &EditorProjectState) -> &'static str {
    match state
        .manifest
        .scenes
        .get(state.active_scene_index)
        .map(|scene| scene.canvas.orientation)
        .unwrap_or(CanvasOrientation::Landscape)
    {
        CanvasOrientation::Landscape => "CanvasMode::Landscape",
        CanvasOrientation::Portrait => "CanvasMode::Portrait",
    }
}

fn managed_main_rs(crate_name: &str) -> String {
    format!(
        "{MANAGED_MAIN_MARKER}\nuse quartz::*;\nuse ramp::Drawable;\nuse {crate_name}::build_app;\n\nramp::run! {{ []; |ctx: &mut Context| {{ build_app(ctx) }} }}\n"
    )
}

fn should_rewrite_managed_main(existing: &str) -> bool {
    existing.contains(MANAGED_MAIN_MARKER)
        || (existing.contains("use quartz::*;")
            && existing.contains("use ramp::Drawable;")
            && existing.contains("::build_app;")
            && existing.contains("ramp::run! { []; |ctx: &mut Context| { build_app(ctx) } }"))
}

fn managed_lib_rs(scene_module_path: &str, canvas_mode: &str) -> String {
    format!(
        "{MANAGED_LIB_MARKER}\nuse quartz::*;\nuse ramp::Drawable;\n\n#[path = \"{scene_module_path}\"]\nmod generated_scene;\n\npub fn build_app(ctx: &mut Context) -> impl Drawable {{\n    let mut canvas = Canvas::new(ctx, {canvas_mode});\n    generated_scene::setup_scene(&mut canvas);\n    generated_scene::register_logic(&mut canvas);\n    generated_scene::register_events(&mut canvas);\n    canvas\n}}\n"
    )
}

fn should_rewrite_managed_lib(existing: &str) -> bool {
    existing.contains(MANAGED_LIB_MARKER)
        || (existing.contains("mod generated_scene;")
            && existing.contains("pub fn build_app(ctx: &mut Context) -> impl Drawable")
            && existing.contains("generated_scene::setup_scene(&mut canvas);")
            && existing.contains("generated_scene::register_logic(&mut canvas);")
            && existing.contains("generated_scene::register_events(&mut canvas);"))
}

fn slugify_crate_name(name: &str) -> String {
    let mut out = String::new();
    let mut prev_sep = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_sep = false;
        } else if !prev_sep {
            out.push('_');
            prev_sep = true;
        }
    }

    let out = out.trim_matches('_').to_owned();
    if out.is_empty() {
        "quartz_game".to_owned()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        managed_lib_rs, managed_main_rs, should_rewrite_managed_lib, should_rewrite_managed_main,
        validate_project_sync, write_sync_snapshot, ProjectSyncStatus,
    };
    use crate::core::project::EditorProjectState;

    fn temp_project_root(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("qf_sync_{name}_{unique}"))
    }

    #[test]
    fn generated_main_marker_is_rewritable() {
        let generated = managed_main_rs("my_game");
        assert!(should_rewrite_managed_main(&generated));
    }

    #[test]
    fn generated_lib_tracks_scene_path_and_canvas_mode() {
        let generated = managed_lib_rs("scripts/menu_scene.rs", "CanvasMode::Portrait");
        assert!(generated.contains("#[path = \"scripts/menu_scene.rs\"]"));
        assert!(generated.contains("Canvas::new(ctx, CanvasMode::Portrait)"));
        assert!(should_rewrite_managed_lib(&generated));
    }

    #[test]
    fn validate_project_sync_reports_missing_snapshot() {
        let root = temp_project_root("missing_snapshot");
        std::fs::create_dir_all(&root).unwrap();
        let state = EditorProjectState::new("sync_test".to_owned());

        let report = validate_project_sync(&state, &root).unwrap();
        assert_eq!(report.status, ProjectSyncStatus::MissingSnapshot);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_snapshot_round_trips_to_in_sync_state() {
        let root = temp_project_root("roundtrip");
        let mut state = EditorProjectState::new("sync_test".to_owned());
        super::save_project(&mut state, &root).unwrap();
        std::fs::write(root.join("src").join("scripts").join("main_scene_scene.rs"), "pub fn setup_scene(_: &mut Canvas) {}\n").unwrap();
        write_sync_snapshot(&state, &root).unwrap();

        let report = validate_project_sync(&state, &root).unwrap();
        assert_eq!(report.status, ProjectSyncStatus::InSync);

        let _ = std::fs::remove_dir_all(&root);
    }
}
