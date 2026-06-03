use std::path::Path;

use anyhow::{Context, Result};

use crate::core::layout::ProjectLayoutPaths;
use crate::core::project::{EditorProjectState, ProjectManifest};

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

pub fn save_project(state: &mut EditorProjectState, root: &Path) -> Result<()> {
    let layout = ProjectLayoutPaths::from_root(root);
    layout.ensure_dirs()?;
    state.manifest.touch_saved_time();
    save_manifest(&state.manifest, &layout)?;
    ensure_runtime_scaffold(state, root)?;
    state.dirty = false;
    Ok(())
}

pub fn ensure_runtime_scaffold(state: &EditorProjectState, root: &Path) -> Result<()> {
    let src_dir = root.join("src");
    let scripts_dir = src_dir.join("scripts");
    let resources_dir = root.join("resources");
    let assets_dir = root.join("assets");

    std::fs::create_dir_all(&src_dir)
        .with_context(|| format!("failed to create {}", src_dir.display()))?;
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
    if path.exists() {
        return Ok(());
    }

    let crate_name = slugify_crate_name(&state.manifest.project_name);
    let main_rs = format!(
        "use quartz::*;\nuse ramp::Drawable;\nuse {}::build_app;\n\nramp::run! {{ []; |ctx: &mut Context| {{ build_app(ctx) }} }}\n",
        crate_name
    );
    std::fs::write(&path, main_rs)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn ensure_lib_rs(state: &EditorProjectState, root: &Path) -> Result<()> {
    let path = root.join("src").join("lib.rs");
    if path.exists() {
        return Ok(());
    }

    let scene_module_rel = state
        .manifest
        .scenes
        .get(state.active_scene_index)
        .map(|scene| scene.source_file.clone())
        .unwrap_or_else(|| "src/scripts/main_scene.rs".to_owned());
    let scene_module_path = scene_module_rel.strip_prefix("src/").unwrap_or(&scene_module_rel);

    let lib_rs = format!(
        "use quartz::*;\nuse ramp::Drawable;\n\n#[path = \"{}\"]\nmod generated_scene;\n\npub fn build_app(ctx: &mut Context) -> impl Drawable {{\n    let mut canvas = Canvas::new(ctx, CanvasMode::Landscape);\n    generated_scene::setup_scene(&mut canvas);\n    generated_scene::register_logic(&mut canvas);\n    generated_scene::register_events(&mut canvas);\n    canvas\n}}\n",
        scene_module_path.replace('\\', "/")
    );

    std::fs::write(&path, lib_rs)
        .with_context(|| format!("failed to write {}", path.display()))
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
