use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, Context, Result};

use crate::core::project::EditorProjectState;
use crate::core::quartz_domain::CustomCodeKind;
use crate::services::{codegen, persistence};

pub fn component_target_path(scene_source_file: &str, configured: &str) -> String {
    let configured = configured.trim();
    if configured.is_empty() {
        scene_source_file.to_owned()
    } else {
        configured.to_owned()
    }
}

pub fn component_module_path_attr(scene_source_file: &str, target_file: &str) -> String {
    let scene_dir = Path::new(scene_source_file)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let target_path = Path::new(target_file);

    let from_components = scene_dir.components().collect::<Vec<_>>();
    let to_components = target_path.components().collect::<Vec<_>>();

    let mut shared_prefix_len = 0usize;
    while shared_prefix_len < from_components.len()
        && shared_prefix_len < to_components.len()
        && from_components[shared_prefix_len] == to_components[shared_prefix_len]
    {
        shared_prefix_len += 1;
    }

    let mut parts = Vec::new();
    for _ in shared_prefix_len..from_components.len() {
        parts.push("..".to_owned());
    }
    for component in to_components.iter().skip(shared_prefix_len) {
        parts.push(component.as_os_str().to_string_lossy().replace('\\', "/"));
    }

    if parts.is_empty() {
        Path::new(target_file)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| target_file.replace('\\', "/"))
    } else {
        parts.join("/")
    }
}

fn custom_code_function_name(prefix: &str, id: &str) -> String {
    let mut out = String::from(prefix);
    for ch in id.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    out
}

fn append_indented_block(out: &mut String, source: &str, indent: &str) {
    for line in source.lines() {
        if line.trim().is_empty() {
            out.push('\n');
        } else {
            out.push_str(indent);
            out.push_str(line);
            out.push('\n');
        }
    }
}

pub fn build_scene_source(state: &EditorProjectState, scene_index: usize) -> String {
    let Some(scene) = state.manifest.scenes.get(scene_index) else {
        return "// no active scene".to_owned();
    };

    let scene_source_file = scene.source_file.trim();
    let mut external_modules: BTreeMap<String, String> = BTreeMap::new();

    for object in &scene.objects {
        let target = component_target_path(scene_source_file, &object.output_file);
        if target != scene_source_file && !external_modules.contains_key(&target) {
            external_modules.insert(target.clone(), codegen::file_module_name(&target));
        }
    }
    for event in &scene.events {
        let target = component_target_path(scene_source_file, &event.output_file);
        if target != scene_source_file && !external_modules.contains_key(&target) {
            external_modules.insert(target.clone(), codegen::file_module_name(&target));
        }
    }
    for tree in &scene.logic_trees {
        let target = component_target_path(scene_source_file, &tree.output_file);
        if target != scene_source_file && !external_modules.contains_key(&target) {
            external_modules.insert(target.clone(), codegen::file_module_name(&target));
        }
    }
    for block in &scene.custom_code_blocks {
        if block.kind == CustomCodeKind::ManualFileOverride {
            continue;
        }
        let target = component_target_path(scene_source_file, &block.output_file);
        if target != scene_source_file
            && !block.code.trim().is_empty()
            && !external_modules.contains_key(&target)
        {
            external_modules.insert(target.clone(), codegen::file_module_name(&target));
        }
    }

    let mut out = String::new();
    out.push_str("use quartz::prelude::*;\n");
    for (target, module_name) in &external_modules {
        let module_path = component_module_path_attr(scene_source_file, target);
        out.push_str(&format!("#[path = \"{}\"]\nmod {};\n", module_path, module_name));
        out.push_str(&format!("use {}::*;\n", module_name));
    }
    if !external_modules.is_empty() {
        out.push_str("\n");
    }

    for block in &scene.custom_code_blocks {
        if block.code.trim().is_empty() {
            continue;
        }
        let target = component_target_path(scene_source_file, &block.output_file);
        if target != scene_source_file {
            continue;
        }
        if matches!(block.kind, CustomCodeKind::Constants | CustomCodeKind::TopLevel) {
            out.push_str(&format!("// custom code block: {}\n", block.name));
            out.push_str(&block.code);
            out.push_str("\n\n");
        }
    }

    for object in &scene.objects {
        if !object.enabled || !object.spawn_only {
            continue;
        }
        let target = component_target_path(scene_source_file, &object.output_file);
        if target == scene_source_file {
            out.push_str(&codegen::object_function_source(object));
            out.push('\n');
        }
    }

    out.push_str("pub fn setup_scene(canvas: &mut Canvas) {\n");
    for object in &scene.objects {
        if !object.enabled || object.spawn_only {
            continue;
        }
        let target = component_target_path(scene_source_file, &object.output_file);
        if target == scene_source_file {
            out.push_str(&codegen::object_registration_body(object));
        } else {
            out.push_str(&format!("    {}(canvas);\n", codegen::object_function_name(object)));
        }
    }
    for block in &scene.custom_code_blocks {
        if !matches!(block.kind, CustomCodeKind::GameStateVars | CustomCodeKind::TypedVars)
            || block.code.trim().is_empty()
        {
            continue;
        }
        let target = component_target_path(scene_source_file, &block.output_file);
        if target == scene_source_file {
            out.push_str(&format!("    // game var block: {}\n", block.name));
            append_indented_block(&mut out, &block.code, "    ");
        } else {
            out.push_str(&format!(
                "    {}(canvas);\n",
                custom_code_function_name("init_vars_", &block.id)
            ));
        }
    }
    out.push_str("}\n\n");

    out.push_str("pub fn register_logic(canvas: &mut Canvas) {\n");
    for tree in &scene.logic_trees {
        let target = component_target_path(scene_source_file, &tree.output_file);
        if target == scene_source_file {
            out.push_str(&format!("    // Update Script: {}\n", tree.name));
            out.push_str("    canvas.on_update(|canvas| {\n");
            out.push_str(&format!("        canvas.run({});\n", codegen::logic_tree_action_expr(tree)));
            out.push_str("    });\n");
        } else {
            out.push_str(&format!("    {}(canvas);\n", codegen::logic_tree_function_name(tree)));
        }
    }
    for block in &scene.custom_code_blocks {
        if block.kind != CustomCodeKind::UpdateLoops || block.code.trim().is_empty() {
            continue;
        }
        let target = component_target_path(scene_source_file, &block.output_file);
        if target == scene_source_file {
            out.push_str(&format!("    // custom update loop: {}\n", block.name));
            out.push_str("    canvas.on_update(|canvas| {\n");
            append_indented_block(&mut out, &block.code, "        ");
            out.push_str("    });\n");
        } else {
            out.push_str(&format!(
                "    {}(canvas);\n",
                custom_code_function_name("register_update_", &block.id)
            ));
        }
    }
    out.push_str("}\n\n");

    out.push_str("pub fn register_events(canvas: &mut Canvas) {\n");
    for event in &scene.events {
        let target = component_target_path(scene_source_file, &event.output_file);
        if target == scene_source_file {
            out.push_str(&codegen::event_binding_body(event, &scene.logic_trees));
        } else {
            out.push_str(&format!("    {}(canvas);\n", codegen::event_function_name(event)));
        }
    }
    for block in &scene.custom_code_blocks {
        if block.kind != CustomCodeKind::CustomEvents || block.code.trim().is_empty() {
            continue;
        }
        let target = component_target_path(scene_source_file, &block.output_file);
        if target == scene_source_file {
            let event_name = if block.custom_event_name.trim().is_empty() {
                block.name.clone()
            } else {
                block.custom_event_name.trim().to_owned()
            };
            out.push_str(&format!("    // custom event: {}\n", block.name));
            out.push_str(&format!(
                "    canvas.register_custom_event(\"{}\".to_owned(), |canvas| {{\n",
                event_name
            ));
            append_indented_block(&mut out, &block.code, "        ");
            out.push_str("    });\n");
        } else {
            out.push_str(&format!(
                "    {}(canvas);\n",
                custom_code_function_name("register_event_", &block.id)
            ));
        }
    }
    out.push_str("}\n");

    out
}

pub fn build_component_module_source(
    state: &EditorProjectState,
    scene_index: usize,
    target_file: &str,
) -> Option<String> {
    let scene = state.manifest.scenes.get(scene_index)?;
    let scene_source_file = scene.source_file.trim();
    let mut out = String::new();
    let mut wrote_any = false;

    out.push_str("use quartz::prelude::*;\n\n");
    for object in &scene.objects {
        if !object.enabled {
            continue;
        }
        let object_target = component_target_path(scene_source_file, &object.output_file);
        if object_target == target_file {
            out.push_str(&codegen::object_function_source(object));
            out.push('\n');
            wrote_any = true;
        }
    }
    for event in &scene.events {
        let event_target = component_target_path(scene_source_file, &event.output_file);
        if event_target == target_file {
            out.push_str(&codegen::event_function_source(event, &scene.logic_trees));
            out.push('\n');
            wrote_any = true;
        }
    }
    for tree in &scene.logic_trees {
        let tree_target = component_target_path(scene_source_file, &tree.output_file);
        if tree_target == target_file {
            out.push_str(&codegen::logic_tree_function_source(tree));
            out.push('\n');
            wrote_any = true;
        }
    }

    for block in &scene.custom_code_blocks {
        if block.code.trim().is_empty() || block.kind == CustomCodeKind::ManualFileOverride {
            continue;
        }
        let block_target = component_target_path(scene_source_file, &block.output_file);
        if block_target != target_file {
            continue;
        }
        match block.kind {
            CustomCodeKind::Constants | CustomCodeKind::TopLevel => {
                out.push_str(&format!("// custom code block: {}\n", block.name));
                out.push_str(&block.code);
                out.push_str("\n\n");
            }
            CustomCodeKind::GameStateVars | CustomCodeKind::TypedVars => {
                out.push_str(&format!(
                    "pub fn {}(canvas: &mut Canvas) {{\n",
                    custom_code_function_name("init_vars_", &block.id)
                ));
                append_indented_block(&mut out, &block.code, "    ");
                out.push_str("}\n\n");
            }
            CustomCodeKind::UpdateLoops => {
                out.push_str(&format!(
                    "pub fn {}(canvas: &mut Canvas) {{\n",
                    custom_code_function_name("register_update_", &block.id)
                ));
                out.push_str("    canvas.on_update(|canvas| {\n");
                append_indented_block(&mut out, &block.code, "        ");
                out.push_str("    });\n}\n\n");
            }
            CustomCodeKind::CustomEvents => {
                let event_name = if block.custom_event_name.trim().is_empty() {
                    block.name.clone()
                } else {
                    block.custom_event_name.trim().to_owned()
                };
                out.push_str(&format!(
                    "pub fn {}(canvas: &mut Canvas) {{\n",
                    custom_code_function_name("register_event_", &block.id)
                ));
                out.push_str(&format!(
                    "    canvas.register_custom_event(\"{}\".to_owned(), |canvas| {{\n",
                    event_name
                ));
                append_indented_block(&mut out, &block.code, "        ");
                out.push_str("    });\n}\n\n");
            }
            CustomCodeKind::ManualFileOverride => {}
        }
        wrote_any = true;
    }

    if wrote_any {
        Some(out)
    } else {
        None
    }
}

pub fn write_generated_files_for_scene(
    state: &EditorProjectState,
    root: &Path,
    scene_index: usize,
) -> Result<()> {
    let scene = state
        .manifest
        .scenes
        .get(scene_index)
        .cloned()
        .ok_or_else(|| anyhow!("missing scene at index {scene_index}"))?;

    let configured_rel = scene.source_file.trim().to_owned();
    let fallback_rel = format!("scripts/{}", codegen::generated_file_name(state));
    let rel_path = if configured_rel.is_empty() { fallback_rel } else { configured_rel };

    let out_path = root.join(&rel_path);
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to prepare output directory {}", parent.display()))?;
    }

    let scene_output_generated = build_scene_source(state, scene_index);
    let scene_override = scene
        .custom_code_blocks
        .iter()
        .find(|b| {
            b.kind == CustomCodeKind::ManualFileOverride
                && b.output_file.trim() == rel_path
                && !b.code.trim().is_empty()
        })
        .map(|b| b.code.clone());
    let scene_output = scene_override.unwrap_or(scene_output_generated);

    std::fs::write(&out_path, scene_output)
        .with_context(|| format!("failed to write generated scene file {}", out_path.display()))?;

    let scene_source_file = scene.source_file.trim().to_owned();
    let mut target_files: Vec<String> = Vec::new();
    for object in &scene.objects {
        let target = component_target_path(&scene_source_file, &object.output_file);
        if target != scene_source_file && !target_files.contains(&target) {
            target_files.push(target);
        }
    }
    for event in &scene.events {
        let target = component_target_path(&scene_source_file, &event.output_file);
        if target != scene_source_file && !target_files.contains(&target) {
            target_files.push(target);
        }
    }
    for tree in &scene.logic_trees {
        let target = component_target_path(&scene_source_file, &tree.output_file);
        if target != scene_source_file && !target_files.contains(&target) {
            target_files.push(target);
        }
    }
    for block in &scene.custom_code_blocks {
        if block.kind == CustomCodeKind::ManualFileOverride {
            let manual_target = block.output_file.trim();
            if !manual_target.is_empty()
                && manual_target != scene_source_file
                && !target_files.contains(&manual_target.to_owned())
            {
                target_files.push(manual_target.to_owned());
            }
            continue;
        }
        let target = component_target_path(&scene_source_file, &block.output_file);
        if target != scene_source_file && !target_files.contains(&target) {
            target_files.push(target);
        }
    }

    for target_file in target_files {
        let module_override = scene
            .custom_code_blocks
            .iter()
            .find(|b| {
                b.kind == CustomCodeKind::ManualFileOverride
                    && b.output_file.trim() == target_file
                    && !b.code.trim().is_empty()
            })
            .map(|b| b.code.clone());

        if let Some(module_source) =
            module_override.or_else(|| build_component_module_source(state, scene_index, &target_file))
        {
            let module_path = root.join(&target_file);
            if let Some(parent) = module_path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("failed to prepare component directory {}", parent.display())
                })?;
            }
            std::fs::write(&module_path, module_source)
                .with_context(|| format!("failed to write component file {}", module_path.display()))?;
        }
    }

    Ok(())
}

pub fn write_all_generated_files_from_state(state: &EditorProjectState, root: &Path) -> Result<()> {
    persistence::ensure_runtime_scaffold(state, root)?;
    for scene_index in 0..state.manifest.scenes.len() {
        write_generated_files_for_scene(state, root, scene_index)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::core::project::EditorProjectState;
    use crate::core::quartz_domain::{ObjectVisualAssetMode, QuartzObjectBlueprint};
    use crate::services::project_import;

    use super::{build_scene_source, write_all_generated_files_from_state};

    fn temp_root(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("qf_codegen_import_{name}_{unique}"))
    }

    fn collect_rs_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(root, &path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }

    #[test]
    fn build_scene_source_emits_cache_aware_image_loader() {
        let mut state = EditorProjectState::new("sync_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scripts/main_scene.rs".to_owned();

        let mut obj = QuartzObjectBlueprint::new("obj_cache".to_owned(), "obj_cache".to_owned());
        obj.visual_asset_mode = ObjectVisualAssetMode::StaticImage;
        obj.visual_asset_path = "assets/ui/panel.png".to_owned();
        obj.visual_asset_use_canvas_cache = true;
        obj.visual_asset_cache_key = "ui/panel".to_owned();
        obj.visual_asset_size_aware_cache = true;
        state.manifest.scenes[0].objects.push(obj);

        let source = build_scene_source(&state, 0);
        assert!(source.contains("canvas.load_image_sized_cached(\"ui/panel\""));
        assert!(source.contains("include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/assets/ui/panel.png\"))"));
    }

    #[test]
    fn generated_scene_roundtrip_preserves_cache_fields() {
        let root = temp_root("cache_roundtrip");
        let mut state = EditorProjectState::new("sync_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scripts/main_scene.rs".to_owned();

        let mut obj = QuartzObjectBlueprint::new("obj_cache".to_owned(), "obj_cache".to_owned());
        obj.visual_asset_mode = ObjectVisualAssetMode::StaticImage;
        obj.visual_asset_path = "assets/ui/panel.png".to_owned();
        obj.visual_asset_use_canvas_cache = true;
        obj.visual_asset_cache_key = "ui/panel".to_owned();
        obj.visual_asset_size_aware_cache = true;
        state.manifest.scenes[0].objects.push(obj);

        write_all_generated_files_from_state(&state, &root).unwrap();

        let mut files = Vec::new();
        collect_rs_files(&root, &root.join("src"), &mut files);

        let mut imported = EditorProjectState::new("sync_test".to_owned());
        imported.manifest.scenes[0].source_file = "src/scripts/main_scene.rs".to_owned();
        let report = project_import::import_files_into_state(&mut imported, &root, &files, true).unwrap();

        assert_eq!(report.imported_object_count, 1);
        let imported_obj = imported.manifest.scenes[0]
            .objects
            .iter()
            .find(|o| o.id == "obj_cache")
            .unwrap();
        assert_eq!(imported_obj.visual_asset_path, "assets/ui/panel.png");
        assert!(imported_obj.visual_asset_use_canvas_cache);
        assert_eq!(imported_obj.visual_asset_cache_key, "ui/panel");
        assert!(imported_obj.visual_asset_size_aware_cache);

        let _ = std::fs::remove_dir_all(&root);
    }
}