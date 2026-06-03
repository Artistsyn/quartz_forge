use std::path::Path;

use anyhow::{Context, Result};

use crate::core::layout::ProjectLayoutPaths;
use crate::core::project::{EditorProjectState, ProjectManifest};

pub fn create_new_project(project_name: String, root: &Path) -> Result<EditorProjectState> {
    let layout = ProjectLayoutPaths::from_root(root);
    layout.ensure_dirs()?;

    let state = EditorProjectState::new(project_name);
    save_manifest(&state.manifest, &layout)?;
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

pub fn save_project(state: &mut EditorProjectState, root: &Path) -> Result<()> {
    let layout = ProjectLayoutPaths::from_root(root);
    layout.ensure_dirs()?;
    state.manifest.touch_saved_time();
    save_manifest(&state.manifest, &layout)?;
    state.dirty = false;
    Ok(())
}

fn save_manifest(manifest: &ProjectManifest, layout: &ProjectLayoutPaths) -> Result<()> {
    let content = serde_json::to_string_pretty(manifest).context("failed to serialize manifest")?;
    std::fs::write(&layout.manifest_path, content)
        .with_context(|| format!("failed to write {}", layout.manifest_path.display()))
}
