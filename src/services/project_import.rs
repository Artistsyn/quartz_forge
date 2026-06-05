use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use quote::ToTokens;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::{
    Expr, ExprCall, ExprClosure, ExprLit, ExprMethodCall, ExprParen, ExprPath, ExprStruct,
    File, Item, ItemFn, Lit, Local, Member, Pat, PatIdent, Stmt,
};

use crate::core::project::EditorProjectState;
use crate::core::quartz_domain::{
    CustomCodeBlock, CustomCodeKind, LogicNode, LogicTree, ObjectPhysicsMaterialPreset,
    ObjectPhysicsMaterialSpec, QuartzAction, QuartzEventBinding, QuartzEventKind,
    QuartzExpr, QuartzExprKind, QuartzKeyModifiers,
    QuartzMouseButtonFilter, QuartzObjectBlueprint, QuartzScrollAxisFilter, QuartzTargetRef,
};

#[derive(Debug, Clone, Default)]
pub struct SemanticImportReport {
    pub imported_files: Vec<String>,
    pub imported_object_count: usize,
    pub imported_logic_tree_count: usize,
    pub imported_event_count: usize,
    pub imported_custom_block_count: usize,
    pub fallback_manual_override_files: Vec<String>,
    pub unsupported_files: Vec<String>,
    pub notes: Vec<String>,
}

pub fn import_files_into_state(
    state: &mut EditorProjectState,
    root: &Path,
    files: &[String],
    fallback_manual_overrides: bool,
) -> Result<SemanticImportReport> {
    let mut report = SemanticImportReport::default();

    for file in files {
        let Some(rel_path) = normalize_project_rel_path(root, file) else {
            report.unsupported_files.push(file.clone());
            report.notes.push(format!("Skipped {} because it is not inside the project root.", file));
            continue;
        };

        let path = root.join(&rel_path);
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        match import_single_file(state, &rel_path, &content) {
            Ok(Some((objects, logic_trees, events, blocks))) => {
                report.imported_files.push(rel_path.clone());
                report.imported_object_count += objects;
                report.imported_logic_tree_count += logic_trees;
                report.imported_event_count += events;
                report.imported_custom_block_count += blocks;
            }
            Ok(None) => {
                if fallback_manual_overrides {
                    let _ = state.track_manual_override_for_file(&rel_path, &content);
                    report.fallback_manual_override_files.push(rel_path);
                } else {
                    report.unsupported_files.push(rel_path);
                }
            }
            Err(err) => {
                if fallback_manual_overrides {
                    let _ = state.track_manual_override_for_file(&rel_path, &content);
                    report
                        .notes
                        .push(format!("Fell back to ManualFileOverride for {} after semantic import error: {err}", rel_path));
                    report.fallback_manual_override_files.push(rel_path);
                } else {
                    return Err(err);
                }
            }
        }
    }

    Ok(report)
}

fn import_single_file(
    state: &mut EditorProjectState,
    rel_path: &str,
    content: &str,
) -> Result<Option<(usize, usize, usize, usize)>> {
    if let Some(count) = import_raw_custom_code_file(state, rel_path, content) {
        return Ok(Some((0, 0, 0, count)));
    }

    let ast = syn::parse_file(content)
        .with_context(|| format!("failed to parse Rust file {} for semantic import", rel_path))?;

    let scene_index = match state.preferred_scene_index_for_file(rel_path) {
        Some(idx) => idx,
        None => return Ok(None),
    };

    let imported_objects = import_objects_from_ast(state, scene_index, rel_path, &ast)?;
    let imported_logic_trees = import_logic_trees_from_ast(state, scene_index, rel_path, &ast)?;
    let imported_events = import_events_from_ast(state, scene_index, rel_path, &ast)?;
    let imported_custom_blocks = import_custom_blocks_from_ast(state, scene_index, rel_path, &ast)?;

    if imported_objects == 0
        && imported_logic_trees == 0
        && imported_events == 0
        && imported_custom_blocks == 0
    {
        Ok(None)
    } else {
        remove_manual_override_for_file(state, scene_index, rel_path);
        Ok(Some((
            imported_objects,
            imported_logic_trees,
            imported_events,
            imported_custom_blocks,
        )))
    }
}

fn import_raw_custom_code_file(
    state: &mut EditorProjectState,
    rel_path: &str,
    content: &str,
) -> Option<usize> {
    let scene_index = state.preferred_scene_index_for_file(rel_path)?;
    let scene = state.manifest.scenes.get_mut(scene_index)?;

    let mut updated = 0usize;
    for block in &mut scene.custom_code_blocks {
        if block.output_file != rel_path {
            continue;
        }
        if matches!(
            block.kind,
            CustomCodeKind::Constants | CustomCodeKind::GameStateVars | CustomCodeKind::TypedVars | CustomCodeKind::TopLevel
        ) {
            block.code = content.to_owned();
            updated += 1;
        }
    }

    if updated > 0 {
        state.dirty = true;
        Some(updated)
    } else {
        None
    }
}

fn import_objects_from_ast(
    state: &mut EditorProjectState,
    scene_index: usize,
    rel_path: &str,
    ast: &File,
) -> Result<usize> {
    let mut imported = Vec::new();
    for item in &ast.items {
        let Item::Fn(func) = item else { continue; };
        let name = func.sig.ident.to_string();
        if name == "setup_scene" || name.starts_with("spawn_") {
            imported.extend(parse_objects_from_function(func, rel_path)?);
        }
    }

    if imported.is_empty() {
        return Ok(0);
    }

    if let Some(scene) = state.manifest.scenes.get_mut(scene_index) {
        scene.objects.retain(|obj| obj.output_file != rel_path);
        scene.objects.extend(imported.iter().cloned());
        state.dirty = true;
    }

    Ok(imported.len())
}

fn parse_objects_from_function(func: &ItemFn, rel_path: &str) -> Result<Vec<QuartzObjectBlueprint>> {
    let mut imported = Vec::new();
    let mut built = std::collections::BTreeMap::<String, QuartzObjectBlueprint>::new();

    for stmt in &func.block.stmts {
        match stmt {
            Stmt::Local(local) => {
                if let Some((var_name, blueprint)) = parse_object_local(local, rel_path)? {
                    built.insert(var_name, blueprint);
                }
            }
            Stmt::Expr(expr, _) => {
                parse_object_followup_expr(expr, &mut built)?;
            }
            Stmt::Item(_) | Stmt::Macro(_) => {}
        }
    }

    imported.extend(built.into_values());
    Ok(imported)
}

fn parse_object_local(
    local: &Local,
    rel_path: &str,
) -> Result<Option<(String, QuartzObjectBlueprint)>> {
    let Pat::Ident(PatIdent { ident, .. }) = &local.pat else {
        return Ok(None);
    };
    let Some(init) = &local.init else {
        return Ok(None);
    };
    let Some((object_id, methods)) = extract_builder_chain(&init.expr) else {
        return Ok(None);
    };

    let mut blueprint = QuartzObjectBlueprint::new(object_id.clone(), object_id.clone());
    blueprint.output_file = rel_path.to_owned();
    apply_builder_methods(&mut blueprint, &methods)?;
    Ok(Some((ident.to_string(), blueprint)))
}

fn parse_object_followup_expr(
    expr: &Expr,
    built: &mut std::collections::BTreeMap<String, QuartzObjectBlueprint>,
) -> Result<()> {
    if let Expr::MethodCall(call) = expr {
        let method = call.method.to_string();
        if method == "set_animation" {
            let Some(var_name) = expr_ident_name(&call.receiver) else {
                return Ok(());
            };
            let Some(object) = built.get_mut(&var_name) else {
                return Ok(());
            };
            if let Some((asset_path, width, height, fps)) = parse_load_animation_call(call) {
                object.visual_asset_mode = crate::core::quartz_domain::ObjectVisualAssetMode::AnimatedSprite;
                object.visual_asset_path = asset_path;
                object.visual_asset_fps = fps;
                if object.w == 0.0 { object.w = width; }
                if object.h == 0.0 { object.h = height; }
            }
        }
    }
    Ok(())
}

fn import_logic_trees_from_ast(
    state: &mut EditorProjectState,
    scene_index: usize,
    rel_path: &str,
    ast: &File,
) -> Result<usize> {
    let mut imported = Vec::<LogicTree>::new();
    for item in &ast.items {
        let Item::Fn(func) = item else { continue; };
        let name = func.sig.ident.to_string();
        if name != "register_logic" && !name.starts_with("register_update_") {
            continue;
        }

        let loops = parse_logic_trees_from_function(func, state, rel_path);
        imported.extend(loops);
    }

    if imported.is_empty() {
        return Ok(0);
    }

    if let Some(scene) = state.manifest.scenes.get_mut(scene_index) {
        scene.logic_trees.retain(|tree| tree.output_file != rel_path);
        scene.logic_trees.extend(imported.iter().cloned());
        state.dirty = true;
    }

    Ok(imported.len())
}

fn parse_logic_trees_from_function(
    func: &ItemFn,
    state: &mut EditorProjectState,
    rel_path: &str,
) -> Vec<LogicTree> {
    let mut out = Vec::new();
    for stmt in &func.block.stmts {
        let Stmt::Expr(Expr::MethodCall(call), _) = stmt else { continue; };
        if call.method != "on_update" {
            continue;
        }
        let Some(Expr::Closure(closure)) = call.args.first() else {
            continue;
        };

        let (id, name) = state.manifest.next_logic_tree_identity();
        let mut tree = LogicTree::new(id, format!("{}_imported", name));
        tree.output_file = rel_path.to_owned();
        tree.nodes = parse_logic_nodes_from_closure(closure);
        tree.refresh_references();
        out.push(tree);
    }
    out
}

fn parse_logic_nodes_from_closure(closure: &ExprClosure) -> Vec<LogicNode> {
    let mut nodes = Vec::new();
    if let Expr::Block(block) = &*closure.body {
        for stmt in &block.block.stmts {
            if let Some(action) = parse_action_from_stmt(stmt) {
                nodes.push(LogicNode::Action(action));
            }
        }
    } else {
        nodes.push(LogicNode::Action(QuartzAction::Expr {
            raw: closure.body.to_token_stream().to_string(),
        }));
    }

    if nodes.is_empty() {
        nodes.push(LogicNode::Action(QuartzAction::Expr {
            raw: closure.body.to_token_stream().to_string(),
        }));
    }
    nodes
}

fn import_events_from_ast(
    state: &mut EditorProjectState,
    scene_index: usize,
    rel_path: &str,
    ast: &File,
) -> Result<usize> {
    let mut imported = Vec::<QuartzEventBinding>::new();
    for item in &ast.items {
        let Item::Fn(func) = item else { continue; };
        let name = func.sig.ident.to_string();
        if name != "register_events" {
            continue;
        }
        imported.extend(parse_event_bindings_from_function(func, state, rel_path));
    }

    if imported.is_empty() {
        return Ok(0);
    }

    if let Some(scene) = state.manifest.scenes.get_mut(scene_index) {
        scene.events.retain(|event| event.output_file != rel_path);
        scene.events.extend(imported.iter().cloned());
        state.dirty = true;
    }

    Ok(imported.len())
}

fn parse_event_bindings_from_function(
    func: &ItemFn,
    state: &mut EditorProjectState,
    rel_path: &str,
) -> Vec<QuartzEventBinding> {
    let mut out = Vec::new();
    for stmt in &func.block.stmts {
        let Stmt::Expr(Expr::MethodCall(call), _) = stmt else { continue; };
        if call.method != "add_event" || call.args.len() < 2 {
            continue;
        }
        let Some((kind, action_target, action)) = parse_game_event_expr(&call.args[0]) else {
            continue;
        };
        let listener_target = parse_target_ref(&call.args[1])
            .unwrap_or_else(|| QuartzTargetRef::Name("player".to_owned()));

        let (id, name) = state.manifest.next_event_identity();
        let mut binding = QuartzEventBinding::new(id, format!("{}_imported", name), listener_target.clone());
        binding.output_file = rel_path.to_owned();
        binding.kind = kind;
        binding.listener_target = listener_target;
        binding.action_target = action_target;
        binding.action = action;
        binding.refresh_references();
        out.push(binding);
    }

    out
}

fn import_custom_blocks_from_ast(
    state: &mut EditorProjectState,
    scene_index: usize,
    rel_path: &str,
    ast: &File,
) -> Result<usize> {
    let mut updated = 0usize;

    if let Some(constants_code) = collect_constants_code(ast) {
        upsert_named_custom_block(
            state,
            scene_index,
            rel_path,
            "constants",
            "constants",
            CustomCodeKind::Constants,
            constants_code,
            None,
        );
        updated += 1;
    }

    for item in &ast.items {
        let Item::Fn(func) = item else { continue; };
        let name = func.sig.ident.to_string();

        if name == "setup_scene" {
            if let Some(code) = extract_setup_scene_game_vars(func) {
                upsert_named_custom_block(
                    state,
                    scene_index,
                    rel_path,
                    "game_state",
                    "game_state",
                    CustomCodeKind::GameStateVars,
                    code,
                    None,
                );
                updated += 1;
            }
            continue;
        }

        if let Some(id) = name.strip_prefix("init_vars_") {
            if let Some(code) = function_body_code(func) {
                upsert_custom_block(
                    state,
                    scene_index,
                    rel_path,
                    id,
                    CustomCodeKind::GameStateVars,
                    code,
                    None,
                );
                updated += 1;
            }
            continue;
        }

        if let Some(id) = name.strip_prefix("register_update_") {
            if let Some(code) = extract_on_update_body(func) {
                upsert_custom_block(
                    state,
                    scene_index,
                    rel_path,
                    id,
                    CustomCodeKind::UpdateLoops,
                    code,
                    None,
                );
                updated += 1;
            }
            continue;
        }

        if let Some(id) = name.strip_prefix("register_event_") {
            if let Some((event_name, code)) = extract_custom_event_body(func) {
                upsert_custom_block(
                    state,
                    scene_index,
                    rel_path,
                    id,
                    CustomCodeKind::CustomEvents,
                    code,
                    Some(event_name),
                );
                updated += 1;
            }
            continue;
        }

        if name == "register_events" {
            let imported = import_custom_events_from_register_events(state, scene_index, rel_path, func);
            updated += imported;
            continue;
        }
    }

    Ok(updated)
}

fn collect_constants_code(ast: &File) -> Option<String> {
    let lines = ast
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Const(_) | Item::Static(_) => Some(item.to_token_stream().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn extract_setup_scene_game_vars(func: &ItemFn) -> Option<String> {
    let lines = func
        .block
        .stmts
        .iter()
        .filter_map(|stmt| {
            let Stmt::Expr(Expr::MethodCall(call), _) = stmt else { return None; };
            if call.method == "set_var" || call.method == "mod_var" {
                Some(stmt.to_token_stream().to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn import_custom_events_from_register_events(
    state: &mut EditorProjectState,
    scene_index: usize,
    rel_path: &str,
    func: &ItemFn,
) -> usize {
    let mut updated = 0usize;
    for stmt in &func.block.stmts {
        let Stmt::Expr(Expr::MethodCall(call), _) = stmt else { continue; };
        if call.method != "register_custom_event" || call.args.len() < 2 {
            continue;
        }
        let Some(event_name) = extract_to_owned_string(&call.args[0]) else {
            continue;
        };
        let Some(Expr::Closure(closure)) = call.args.get(1) else {
            continue;
        };
        let Some(code) = closure_body_code(closure) else {
            continue;
        };
        let block_id = format!("custom_event_{}", sanitize_identifier(&event_name));
        upsert_named_custom_block(
            state,
            scene_index,
            rel_path,
            &block_id,
            &event_name,
            CustomCodeKind::CustomEvents,
            code,
            Some(event_name.clone()),
        );
        updated += 1;
    }
    updated
}

fn upsert_custom_block(
    state: &mut EditorProjectState,
    scene_index: usize,
    rel_path: &str,
    block_id_suffix: &str,
    fallback_kind: CustomCodeKind,
    code: String,
    custom_event_name: Option<String>,
) {
    if let Some(scene) = state.manifest.scenes.get_mut(scene_index) {
        if let Some(block) = scene.custom_code_blocks.iter_mut().find(|block| {
            block.output_file == rel_path
                && (block.id == block_id_suffix
                    || block.id == format!("code_{}", block_id_suffix)
                    || block.id.ends_with(block_id_suffix))
        }) {
            block.code = code;
            if let Some(name) = custom_event_name {
                block.custom_event_name = name;
            }
            state.dirty = true;
            return;
        }
    } else {
        return;
    }

    let (id, name) = state.manifest.next_custom_code_identity(fallback_kind);
    let mut block = CustomCodeBlock::new(id, name, fallback_kind, rel_path.to_owned());
    block.code = code;
    if let Some(name) = custom_event_name {
        block.custom_event_name = name;
    }
    if let Some(scene) = state.manifest.scenes.get_mut(scene_index) {
        scene.custom_code_blocks.push(block);
        state.dirty = true;
    }
}

fn upsert_named_custom_block(
    state: &mut EditorProjectState,
    scene_index: usize,
    rel_path: &str,
    block_id: &str,
    block_name: &str,
    kind: CustomCodeKind,
    code: String,
    custom_event_name: Option<String>,
) {
    if let Some(scene) = state.manifest.scenes.get_mut(scene_index) {
        if let Some(block) = scene
            .custom_code_blocks
            .iter_mut()
            .find(|block| block.output_file == rel_path && block.kind == kind && block.id == block_id)
        {
            block.name = block_name.to_owned();
            block.code = code;
            if let Some(name) = custom_event_name {
                block.custom_event_name = name;
            }
            state.dirty = true;
            return;
        }
    } else {
        return;
    }

    let mut block = CustomCodeBlock::new(
        block_id.to_owned(),
        block_name.to_owned(),
        kind,
        rel_path.to_owned(),
    );
    block.code = code;
    if let Some(name) = custom_event_name {
        block.custom_event_name = name;
    }
    if let Some(scene) = state.manifest.scenes.get_mut(scene_index) {
        scene.custom_code_blocks.push(block);
        state.dirty = true;
    }
}

fn function_body_code(func: &ItemFn) -> Option<String> {
    Some(block_stmts_to_code(&func.block.stmts))
}

fn extract_on_update_body(func: &ItemFn) -> Option<String> {
    for stmt in &func.block.stmts {
        let Stmt::Expr(Expr::MethodCall(call), _) = stmt else { continue; };
        if call.method != "on_update" { continue; }
        let Some(Expr::Closure(closure)) = call.args.first() else { continue; };
        return closure_body_code(closure);
    }
    None
}

fn extract_custom_event_body(func: &ItemFn) -> Option<(String, String)> {
    for stmt in &func.block.stmts {
        let Stmt::Expr(Expr::MethodCall(call), _) = stmt else { continue; };
        if call.method != "register_custom_event" || call.args.len() < 2 { continue; }
        let event_name = extract_to_owned_string(&call.args[0])?;
        let closure = match &call.args[1] {
            Expr::Closure(closure) => closure,
            _ => continue,
        };
        let code = closure_body_code(closure)?;
        return Some((event_name, code));
    }
    None
}

fn closure_body_code(closure: &ExprClosure) -> Option<String> {
    match &*closure.body {
        Expr::Block(block) => Some(block_stmts_to_code(&block.block.stmts)),
        other => Some(other.to_token_stream().to_string()),
    }
}

fn block_stmts_to_code(stmts: &[Stmt]) -> String {
    stmts.iter()
        .map(|stmt| stmt.to_token_stream().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_builder_chain(expr: &Expr) -> Option<(String, Vec<(String, Vec<Expr>)>)> {
    let mut methods = Vec::new();
    let mut current = expr;
    loop {
        match current {
            Expr::MethodCall(call) => {
                methods.push((call.method.to_string(), call.args.iter().cloned().collect()));
                current = &call.receiver;
            }
            Expr::Call(ExprCall { func, args, .. }) => {
                let Expr::Path(ExprPath { path, .. }) = &**func else { return None; };
                let segments = path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>();
                if segments != ["GameObject", "build"] {
                    return None;
                }
                let object_id = args.first().and_then(extract_string_literal)?;
                methods.reverse();
                return Some((object_id, methods));
            }
            Expr::Paren(ExprParen { expr, .. }) => current = expr,
            _ => return None,
        }
    }
}

fn apply_builder_methods(
    object: &mut QuartzObjectBlueprint,
    methods: &[(String, Vec<Expr>)],
) -> Result<()> {
    for (method, args) in methods {
        match method.as_str() {
            "size" if args.len() == 2 => {
                object.w = expr_to_f32(&args[0])?;
                object.h = expr_to_f32(&args[1])?;
            }
            "position" if args.len() == 2 => {
                object.x = expr_to_f32(&args[0])?;
                object.y = expr_to_f32(&args[1])?;
            }
            "layer" if args.len() == 1 => object.layer = expr_to_i32(&args[0])?,
            "momentum" if args.len() == 2 => {
                object.advanced.momentum_x = expr_to_f32(&args[0])?;
                object.advanced.momentum_y = expr_to_f32(&args[1])?;
            }
            "resistance" if args.len() == 2 => {
                object.advanced.resistance_x = expr_to_f32(&args[0])?;
                object.advanced.resistance_y = expr_to_f32(&args[1])?;
            }
            "gravity" if args.len() == 1 => object.advanced.gravity = expr_to_f32(&args[0])?,
            "rotation" if args.len() == 1 => object.advanced.rotation_deg = expr_to_f32(&args[0])?,
            "pivot" if args.len() == 2 => {
                object.advanced.pivot_x = expr_to_f32(&args[0])?;
                object.advanced.pivot_y = expr_to_f32(&args[1])?;
            }
            "material" if args.len() == 1 => object.advanced.material = expr_to_material(&args[0])?,
            "collision_layer" if args.len() == 1 => object.advanced.collision_layer = expr_to_u32(&args[0])?,
            "collision_mask" if args.len() == 1 => object.advanced.collision_mask = expr_to_u32(&args[0])?,
            "image" if args.len() == 1 => {
                if let Some(parsed) = parse_load_image_expr(&args[0]) {
                    object.visual_asset_mode = crate::core::quartz_domain::ObjectVisualAssetMode::StaticImage;
                    object.visual_asset_path = parsed.asset_path;
                    object.visual_asset_use_canvas_cache = parsed.use_canvas_cache;
                    object.visual_asset_cache_key = parsed.cache_key.unwrap_or_default();
                    object.visual_asset_size_aware_cache = parsed.size_aware_cache;
                }
            }
            "screen_space" => object.advanced.set_camera_space_pinned(true),
            "ignore_zoom" => object.advanced.ignore_zoom = true,
            "tag" if args.len() == 1 => {
                if let Some(tag) = extract_string_literal(&args[0]) {
                    object.tags.push(tag);
                }
            }
            "slope_auto_rotation" if args.len() == 2 => {
                object.advanced.slope_enabled = true;
                object.advanced.slope_auto_rotation = true;
                object.advanced.slope_left_offset = expr_to_f32(&args[0])?;
                object.advanced.slope_right_offset = expr_to_f32(&args[1])?;
            }
            "slope" if args.len() == 2 => {
                object.advanced.slope_enabled = true;
                object.advanced.slope_left_offset = expr_to_f32(&args[0])?;
                object.advanced.slope_right_offset = expr_to_f32(&args[1])?;
            }
            "one_way" => object.advanced.one_way = true,
            "surface_velocity" if args.len() == 1 => {
                object.advanced.surface_velocity_enabled = true;
                object.advanced.surface_velocity_x = expr_to_f32(&args[0])?;
            }
            "surface" if args.len() == 2 => {
                object.advanced.surface_normal_enabled = true;
                object.advanced.surface_normal_x = expr_to_f32(&args[0])?;
                object.advanced.surface_normal_y = expr_to_f32(&args[1])?;
            }
            "align_to_slope" => object.advanced.align_to_slope = true,
            "align_to_slope_speed" if args.len() == 1 => object.advanced.align_to_slope_speed = expr_to_f32(&args[0])?,
            "planet" if args.len() == 1 => {
                object.advanced.planet_enabled = true;
                object.advanced.planet_radius = expr_to_f32(&args[0])?;
            }
            "gravity_target" if args.len() == 1 => {
                object.advanced.gravity_target_enabled = true;
                object.advanced.gravity_target_tag = extract_string_literal(&args[0]).unwrap_or_default();
            }
            "gravity_strength" if args.len() == 1 => object.advanced.gravity_strength = expr_to_f32(&args[0])?,
            "gravity_influence_mult" if args.len() == 1 => object.advanced.gravity_influence_mult = expr_to_f32(&args[0])?,
            "gravity_falloff" if args.len() == 1 => {
                if let Some(value) = path_last_ident(&args[0]) {
                    object.advanced.gravity_falloff = match value.as_str() {
                        "Constant" => crate::core::quartz_domain::QuartzGravityFalloff::Constant,
                        "InverseSquare" => crate::core::quartz_domain::QuartzGravityFalloff::InverseSquare,
                        _ => crate::core::quartz_domain::QuartzGravityFalloff::Linear,
                    };
                }
            }
            "gravity_all_sources" => object.advanced.gravity_all_sources = true,
            "gravity_identity" if args.len() == 1 => {
                object.advanced.gravity_identity_enabled = true;
                object.advanced.gravity_identity = extract_string_literal(&args[0]).unwrap_or_default();
            }
            "auto_align" => object.advanced.auto_align = true,
            "auto_align_speed" if args.len() == 1 => object.advanced.auto_align_speed = expr_to_f32(&args[0])?,
            "build" => {}
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct ParsedStaticImage {
    asset_path: String,
    use_canvas_cache: bool,
    cache_key: Option<String>,
    size_aware_cache: bool,
}

fn parse_load_image_expr(expr: &Expr) -> Option<ParsedStaticImage> {
    match expr {
        Expr::Call(ExprCall { func, args, .. }) => {
            if path_last_ident(func)? == "load_image" {
                let asset_path = args.first().and_then(parse_include_bytes_expr)?;
                return Some(ParsedStaticImage {
                    asset_path,
                    use_canvas_cache: false,
                    cache_key: None,
                    size_aware_cache: true,
                });
            }

            let Expr::MethodCall(call) = &**func else { return None; };
            let method_args = args.iter().cloned().collect::<Vec<_>>();
            parse_cached_image_method_call(call, &method_args)
        }
        Expr::MethodCall(call) => {
            let args = call.args.iter().cloned().collect::<Vec<_>>();
            parse_cached_image_method_call(call, &args)
        }
        _ => None,
    }
}

fn parse_cached_image_method_call(call: &ExprMethodCall, args: &[Expr]) -> Option<ParsedStaticImage> {
    let method = call.method.to_string();
    if method == "load_image_cached" && args.len() >= 2 {
        let cache_key = extract_string_literal(&args[0]);
        let asset_path = parse_include_bytes_expr(&args[1])?;
        return Some(ParsedStaticImage {
            asset_path,
            use_canvas_cache: true,
            cache_key,
            size_aware_cache: false,
        });
    }

    if method == "load_image_sized_cached" && args.len() >= 4 {
        let cache_key = extract_string_literal(&args[0]);
        let asset_path = parse_include_bytes_expr(&args[1])?;
        return Some(ParsedStaticImage {
            asset_path,
            use_canvas_cache: true,
            cache_key,
            size_aware_cache: true,
        });
    }

    None
}

fn parse_load_animation_call(call: &ExprMethodCall) -> Option<(String, f32, f32, f32)> {
    let Expr::Call(ExprCall { func, args, .. }) = call.args.first()? else { return None; };
    if path_last_ident(func)? != "load_animation" || args.len() < 3 {
        return None;
    }
    let path = parse_include_bytes_expr(&args[0])?;
    let (w, h) = match &args[1] {
        Expr::Tuple(tuple) if tuple.elems.len() == 2 => {
            (expr_to_f32(&tuple.elems[0]).ok()?, expr_to_f32(&tuple.elems[1]).ok()?)
        }
        _ => return None,
    };
    let fps = expr_to_f32(&args[2]).ok()?;
    Some((path, w, h, fps))
}

fn parse_action_from_stmt(stmt: &Stmt) -> Option<QuartzAction> {
    let Stmt::Expr(expr, _) = stmt else { return None; };
    if let Expr::MethodCall(call) = expr {
        if call.method == "run" {
            return call.args.first().map(parse_action_expr);
        }
    }
    Some(QuartzAction::Expr {
        raw: expr.to_token_stream().to_string(),
    })
}

fn parse_game_event_expr(expr: &Expr) -> Option<(QuartzEventKind, QuartzTargetRef, Option<QuartzAction>)> {
    let Expr::Struct(ExprStruct { path, fields, .. }) = expr else { return None; };
    let variant = path.segments.last()?.ident.to_string();

    let mut key: Option<String> = None;
    let mut modifiers = QuartzKeyModifiers::default();
    let mut action_target: Option<QuartzTargetRef> = None;
    let mut action: Option<QuartzAction> = None;
    let mut custom_name: Option<String> = None;
    let mut mouse_button = QuartzMouseButtonFilter::Any;
    let mut scroll_axis = QuartzScrollAxisFilter::Any;

    for field in fields {
        let Member::Named(member) = &field.member else { continue; };
        match member.to_string().as_str() {
            "key" => key = parse_key_literal(&field.expr),
            "target" => action_target = parse_target_ref(&field.expr),
            "action" => action = Some(parse_action_expr(&field.expr)),
            "name" => custom_name = extract_to_owned_string(&field.expr),
            "modifiers" => {
                if let Some(parsed) = parse_modifiers_expr(&field.expr) {
                    modifiers = parsed;
                }
            }
            "button" => {
                mouse_button = parse_mouse_button_filter(&field.expr);
            }
            "axis" => {
                scroll_axis = parse_scroll_axis_filter(&field.expr);
            }
            _ => {}
        }
    }

    let target = action_target.unwrap_or_else(|| QuartzTargetRef::Name("player".to_owned()));
    let kind = match variant.as_str() {
        "Collision" => QuartzEventKind::Collision,
        "BoundaryCollision" => QuartzEventKind::BoundaryCollision,
        "KeyPress" => QuartzEventKind::KeyPress {
            key: key.unwrap_or_else(|| "Space".to_owned()),
            modifiers,
        },
        "KeyRelease" => QuartzEventKind::KeyRelease {
            key: key.unwrap_or_else(|| "Space".to_owned()),
            modifiers,
        },
        "KeyHold" => QuartzEventKind::KeyHold {
            key: key.unwrap_or_else(|| "Space".to_owned()),
            modifiers,
        },
        "Tick" => QuartzEventKind::Tick,
        "Custom" => QuartzEventKind::Custom {
            name: custom_name.unwrap_or_else(|| "custom_event".to_owned()),
        },
        "MousePress" => QuartzEventKind::MousePress {
            button: mouse_button,
        },
        "MouseRelease" => QuartzEventKind::MouseRelease {
            button: mouse_button,
        },
        "MouseEnter" => QuartzEventKind::MouseEnter,
        "MouseLeave" => QuartzEventKind::MouseLeave,
        "MouseOver" => QuartzEventKind::MouseOver,
        "MouseScroll" => QuartzEventKind::MouseScroll { axis: scroll_axis },
        "MouseMove" => QuartzEventKind::MouseMove,
        _ => return None,
    };

    Some((kind, target, action))
}

fn parse_action_expr(expr: &Expr) -> QuartzAction {
    let Expr::Struct(ExprStruct { path, fields, .. }) = expr else {
        return QuartzAction::Expr {
            raw: expr.to_token_stream().to_string(),
        };
    };
    let variant = path.segments.last().map(|seg| seg.ident.to_string());
    match variant.as_deref() {
        Some("Custom") => {
            for field in fields {
                if let Member::Named(member) = &field.member {
                    if member == "name" {
                        if let Some(name) = extract_to_owned_string(&field.expr) {
                            return QuartzAction::Custom { name };
                        }
                    }
                }
            }
            QuartzAction::Expr {
                raw: expr.to_token_stream().to_string(),
            }
        }
        Some("SetVar") => {
            let mut name = String::new();
            let mut value = QuartzExpr { kind: QuartzExprKind::F32, raw: "0.0".to_owned() };
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                if member == "name" {
                    name = extract_to_owned_string(&field.expr).unwrap_or_default();
                } else if member == "value" {
                    value = parse_expr_value(&field.expr);
                }
            }
            QuartzAction::SetVar { name, value }
        }
        Some("ModVar") => {
            let mut name = String::new();
            let mut op = crate::core::quartz_domain::QuartzMathOp::Add;
            let mut operand = QuartzExpr { kind: QuartzExprKind::F32, raw: "0.0".to_owned() };
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "op" => {
                        op = match path_last_ident(&field.expr).as_deref() {
                            Some("Sub") => crate::core::quartz_domain::QuartzMathOp::Sub,
                            Some("Mul") => crate::core::quartz_domain::QuartzMathOp::Mul,
                            Some("Div") => crate::core::quartz_domain::QuartzMathOp::Div,
                            _ => crate::core::quartz_domain::QuartzMathOp::Add,
                        }
                    }
                    "operand" => operand = parse_expr_value(&field.expr),
                    _ => {}
                }
            }
            QuartzAction::ModVar { name, op, operand }
        }
        _ => QuartzAction::Expr {
            raw: expr.to_token_stream().to_string(),
        },
    }
}

fn parse_expr_value(expr: &Expr) -> QuartzExpr {
    match expr {
        Expr::Lit(ExprLit { lit: Lit::Int(v), .. }) => QuartzExpr {
            kind: QuartzExprKind::I32,
            raw: v.base10_digits().to_owned(),
        },
        Expr::Lit(ExprLit { lit: Lit::Float(v), .. }) => QuartzExpr {
            kind: QuartzExprKind::F32,
            raw: v.base10_digits().to_owned(),
        },
        Expr::Lit(ExprLit { lit: Lit::Bool(v), .. }) => QuartzExpr {
            kind: QuartzExprKind::Bool,
            raw: v.value.to_string(),
        },
        Expr::Lit(ExprLit { lit: Lit::Str(v), .. }) => QuartzExpr {
            kind: QuartzExprKind::Str,
            raw: v.value(),
        },
        Expr::Call(call) if path_last_ident(&call.func).as_deref() == Some("var") => {
            let variable = call
                .args
                .first()
                .and_then(extract_to_owned_string)
                .unwrap_or_default();
            QuartzExpr {
                kind: QuartzExprKind::Var,
                raw: variable,
            }
        }
        _ => QuartzExpr {
            kind: QuartzExprKind::Var,
            raw: expr.to_token_stream().to_string(),
        },
    }
}

fn parse_target_ref(expr: &Expr) -> Option<QuartzTargetRef> {
    let Expr::Call(ExprCall { func, args, .. }) = expr else {
        return None;
    };
    let variant = path_last_ident(func)?;
    let value = args.first().and_then(extract_to_owned_string)?;
    match variant.as_str() {
        "name" => Some(QuartzTargetRef::Name(value)),
        "tag" => Some(QuartzTargetRef::Tag(value)),
        "id" => Some(QuartzTargetRef::Id(value)),
        _ => None,
    }
}

fn parse_key_literal(expr: &Expr) -> Option<String> {
    if let Expr::Call(ExprCall { func, args, .. }) = expr {
        if path_last_ident(func).as_deref() == Some("Character") {
            return args.first().and_then(extract_to_owned_string);
        }
    }
    path_last_ident(expr)
}

fn parse_modifiers_expr(expr: &Expr) -> Option<QuartzKeyModifiers> {
    if path_last_ident(expr).as_deref() == Some("None") {
        return Some(QuartzKeyModifiers::default());
    }
    let Expr::Call(ExprCall { func, args, .. }) = expr else {
        return None;
    };
    if path_last_ident(func).as_deref() != Some("Some") {
        return None;
    }
    let Expr::Struct(ExprStruct { fields, .. }) = args.first()? else {
        return None;
    };
    let mut out = QuartzKeyModifiers::default();
    for field in fields {
        let Member::Named(member) = &field.member else { continue; };
        let value = matches!(&field.expr, Expr::Lit(ExprLit { lit: Lit::Bool(v), .. }) if v.value);
        match member.to_string().as_str() {
            "shift" => out.shift = value,
            "control" | "ctrl" => out.control = value,
            "alt" => out.alt = value,
            "meta" | "logo" => out.meta = value,
            _ => {}
        }
    }
    Some(out)
}

fn parse_mouse_button_filter(expr: &Expr) -> QuartzMouseButtonFilter {
    match path_last_ident(expr).as_deref() {
        Some("Left") => QuartzMouseButtonFilter::Left,
        Some("Right") => QuartzMouseButtonFilter::Right,
        Some("Middle") => QuartzMouseButtonFilter::Middle,
        _ => QuartzMouseButtonFilter::Any,
    }
}

fn parse_scroll_axis_filter(expr: &Expr) -> QuartzScrollAxisFilter {
    match path_last_ident(expr).as_deref() {
        Some("X") => QuartzScrollAxisFilter::X,
        Some("Y") => QuartzScrollAxisFilter::Y,
        _ => QuartzScrollAxisFilter::Any,
    }
}

fn sanitize_identifier(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "entry".to_owned()
    } else {
        out
    }
}

fn parse_include_bytes_expr(expr: &Expr) -> Option<String> {
    let Expr::Macro(mac) = expr else { return None; };
    if !mac.mac.path.is_ident("include_bytes") {
        return None;
    }

    if let Ok(lit) = syn::parse2::<syn::LitStr>(mac.mac.tokens.clone()) {
        return Some(lit.value());
    }

    let inner = syn::parse2::<Expr>(mac.mac.tokens.clone()).ok()?;
    parse_manifest_concat_expr(&inner)
}

fn parse_manifest_concat_expr(expr: &Expr) -> Option<String> {
    let Expr::Macro(mac) = expr else { return None; };
    if !mac.mac.path.is_ident("concat") {
        return None;
    }

    let args = Punctuated::<Expr, syn::Token![,]>::parse_terminated
        .parse2(mac.mac.tokens.clone())
        .ok()?;

    let mut joined = String::new();
    for arg in args.iter() {
        if let Some(part) = extract_string_literal(arg) {
            joined.push_str(&part);
        }
    }

    if joined.is_empty() {
        return None;
    }

    Some(joined.trim_start_matches('/').to_owned())
}

fn expr_ident_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(path) if path.path.segments.len() == 1 => Some(path.path.segments[0].ident.to_string()),
        Expr::Paren(paren) => expr_ident_name(&paren.expr),
        _ => None,
    }
}

fn extract_to_owned_string(expr: &Expr) -> Option<String> {
    if let Expr::MethodCall(call) = expr {
        if call.method == "to_owned" {
            return extract_string_literal(&call.receiver);
        }
    }
    extract_string_literal(expr)
}

fn extract_string_literal(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Lit(ExprLit { lit: Lit::Str(value), .. }) => Some(value.value()),
        Expr::Paren(paren) => extract_string_literal(&paren.expr),
        _ => None,
    }
}

fn expr_to_f32(expr: &Expr) -> Result<f32> {
    match expr {
        Expr::Lit(ExprLit { lit: Lit::Float(value), .. }) => Ok(value.base10_parse::<f32>()?),
        Expr::Lit(ExprLit { lit: Lit::Int(value), .. }) => Ok(value.base10_parse::<f32>()?),
        Expr::Unary(unary) if unary.op.to_token_stream().to_string() == "-" => Ok(-expr_to_f32(&unary.expr)?),
        Expr::Paren(paren) => expr_to_f32(&paren.expr),
        _ => anyhow::bail!("unsupported numeric expression: {}", expr.to_token_stream()),
    }
}

fn expr_to_i32(expr: &Expr) -> Result<i32> {
    Ok(expr_to_f32(expr)? as i32)
}

fn expr_to_u32(expr: &Expr) -> Result<u32> {
    Ok(expr_to_f32(expr)? as u32)
}

fn expr_to_material(expr: &Expr) -> Result<ObjectPhysicsMaterialSpec> {
    if let Some(last) = path_last_ident(expr) {
        let preset = match last.as_str() {
            "default" => Some(ObjectPhysicsMaterialPreset::Default),
            "rubber" => Some(ObjectPhysicsMaterialPreset::Rubber),
            "ice" => Some(ObjectPhysicsMaterialPreset::Ice),
            "metal" => Some(ObjectPhysicsMaterialPreset::Metal),
            "wood" => Some(ObjectPhysicsMaterialPreset::Wood),
            "stone" => Some(ObjectPhysicsMaterialPreset::Stone),
            "bouncy" => Some(ObjectPhysicsMaterialPreset::Bouncy),
            "sticky" => Some(ObjectPhysicsMaterialPreset::Sticky),
            "glass" => Some(ObjectPhysicsMaterialPreset::Glass),
            "feather" => Some(ObjectPhysicsMaterialPreset::Feather),
            _ => None,
        };
        if let Some(preset) = preset {
            return Ok(ObjectPhysicsMaterialSpec::resolved_defaults(preset));
        }
    }

    let Expr::Struct(ExprStruct { fields, .. }) = expr else {
        anyhow::bail!("unsupported material expression: {}", expr.to_token_stream())
    };
    let mut material = ObjectPhysicsMaterialSpec::resolved_defaults(ObjectPhysicsMaterialPreset::Custom);
    material.preset = ObjectPhysicsMaterialPreset::Custom;
    for field in fields {
        let Member::Named(name) = &field.member else { continue; };
        match name.to_string().as_str() {
            "elasticity" => material.elasticity = expr_to_f32(&field.expr)?,
            "friction" => material.friction = expr_to_f32(&field.expr)?,
            "density" => material.density = expr_to_f32(&field.expr)?,
            _ => {}
        }
    }
    Ok(material)
}

fn path_last_ident(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Call(call) => path_last_ident(&call.func),
        Expr::Path(path) => path.path.segments.last().map(|seg| seg.ident.to_string()),
        Expr::Paren(paren) => path_last_ident(&paren.expr),
        _ => None,
    }
}

fn remove_manual_override_for_file(state: &mut EditorProjectState, scene_index: usize, rel_path: &str) {
    if let Some(scene) = state.manifest.scenes.get_mut(scene_index) {
        let before = scene.custom_code_blocks.len();
        scene.custom_code_blocks.retain(|block| {
            !(block.kind == CustomCodeKind::ManualFileOverride && block.output_file == rel_path)
        });
        if scene.custom_code_blocks.len() != before {
            state.dirty = true;
        }
    }
}

fn normalize_project_rel_path(root: &Path, file: &str) -> Option<String> {
    let candidate = PathBuf::from(file);
    let path = if candidate.is_absolute() { candidate } else { root.join(candidate) };
    path.strip_prefix(root)
        .ok()
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::import_files_into_state;
    use crate::core::project::EditorProjectState;
    use crate::core::quartz_domain::CustomCodeKind;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("qf_import_{name}_{unique}"))
    }

    #[test]
    fn semantic_import_reads_object_component_file() {
        let root = temp_root("object_component");
        std::fs::create_dir_all(root.join("src/scripts")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scripts/main_scene.rs".to_owned();
        let object_file = "src/scripts/objects.rs";
        std::fs::write(
            root.join(object_file),
            r#"use quartz::prelude::*;

pub fn spawn_obj_0001(canvas: &mut Canvas) {
    let mut obj_0001 = GameObject::build("obj_0001")
        .size(64.0, 32.0)
        .position(10.0, 20.0)
        .layer(2)
        .momentum(1.0, 2.0)
        .resistance(3.0, 4.0)
        .gravity(5.0)
        .rotation(6.0)
        .pivot(0.25, 0.75)
        .material(PhysicsMaterial::rubber())
        .collision_layer(7)
        .collision_mask(8)
        .screen_space()
        .tag("enemy")
        .build(canvas);
    canvas.add_game_object("obj_0001".to_owned(), obj_0001);
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[object_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_object_count, 1);
        assert!(report.fallback_manual_override_files.is_empty());
        let object = &state.manifest.scenes[0].objects[0];
        assert_eq!(object.id, "obj_0001");
        assert_eq!(object.output_file, object_file);
        assert!(object.advanced.screen_space);
        assert_eq!(object.tags, vec!["enemy".to_owned()]);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unsupported_file_falls_back_to_manual_override() {
        let root = temp_root("fallback");
        std::fs::create_dir_all(root.join("src/scripts")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        let rel = "src/scripts/main_scene.rs";
        state.manifest.scenes[0].source_file = rel.to_owned();
        std::fs::write(root.join(rel), "pub fn register_events(canvas: &mut Canvas) { canvas.add_event(GameEvent::Tick, Target::name(\"player\")); }").unwrap();

        let report = import_files_into_state(&mut state, &root, &[rel.to_owned()], true).unwrap();
        assert!(report.imported_files.is_empty());
        assert_eq!(report.fallback_manual_override_files, vec![rel.to_owned()]);
        assert!(state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .any(|block| block.kind == CustomCodeKind::ManualFileOverride && block.output_file == rel));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_reads_cached_static_image_configuration() {
        let root = temp_root("cached_static_image");
        std::fs::create_dir_all(root.join("src/scripts")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scripts/main_scene.rs".to_owned();
        let object_file = "src/scripts/objects.rs";
        std::fs::write(
            root.join(object_file),
            r#"use quartz::prelude::*;

pub fn spawn_obj_cache(canvas: &mut Canvas) {
    let obj_cache = GameObject::build("obj_cache")
        .size(96.0, 48.0)
        .position(12.0, 24.0)
        .image(canvas.load_image_sized_cached("ui/panel", include_bytes!("assets/ui/panel.png"), 96.0, 48.0))
        .build(canvas);
    canvas.add_game_object("obj_cache".to_owned(), obj_cache);
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[object_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_object_count, 1);
        let object = &state.manifest.scenes[0].objects[0];
        assert_eq!(object.visual_asset_path, "assets/ui/panel.png");
        assert!(object.visual_asset_use_canvas_cache);
        assert_eq!(object.visual_asset_cache_key, "ui/panel");
        assert!(object.visual_asset_size_aware_cache);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_reads_multiple_spawn_objects_from_single_file() {
        let root = temp_root("multi_spawn_objects");
        std::fs::create_dir_all(root.join("src/scripts")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scripts/main_scene.rs".to_owned();
        let object_file = "src/scripts/objects.rs";
        std::fs::write(
            root.join(object_file),
            r#"use quartz::prelude::*;

pub fn spawn_obj_a(canvas: &mut Canvas) {
    let obj_a = GameObject::build("obj_a")
        .size(20.0, 20.0)
        .position(1.0, 2.0)
        .build(canvas);
    canvas.add_game_object("obj_a".to_owned(), obj_a);
}

pub fn spawn_obj_b(canvas: &mut Canvas) {
    let obj_b = GameObject::build("obj_b")
        .size(30.0, 40.0)
        .position(3.0, 4.0)
        .build(canvas);
    canvas.add_game_object("obj_b".to_owned(), obj_b);
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[object_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_object_count, 2);
        assert_eq!(state.manifest.scenes[0].objects.len(), 2);
        assert!(state.manifest.scenes[0].objects.iter().any(|obj| obj.id == "obj_a"));
        assert!(state.manifest.scenes[0].objects.iter().any(|obj| obj.id == "obj_b"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_reads_register_logic_update_loops() {
        let root = temp_root("register_logic");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn register_logic(canvas: &mut Canvas) {
    canvas.on_update(|canvas| {
        canvas.run(Action::Custom { name: "tick_a".to_owned() });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::Custom { name: "tick_b".to_owned() });
    });
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_logic_tree_count, 2);
        assert_eq!(state.manifest.scenes[0].logic_trees.len(), 2);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_reads_register_events_and_constants() {
        let root = temp_root("register_events_constants");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

const PLAYER_SPEED: f32 = 240.0;

pub fn setup_scene(canvas: &mut Canvas) {
    canvas.set_var("score", Value::I32(0));
}

pub fn register_events(canvas: &mut Canvas) {
    canvas.add_event(
        GameEvent::Tick {
            action: Action::SetVar { name: "score".to_owned(), value: Expr::i32(1) },
            target: Target::name("player"),
        },
        Target::name("player"),
    );
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_event_count, 1);
        assert_eq!(state.manifest.scenes[0].events.len(), 1);
        assert!(state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .any(|b| b.kind == CustomCodeKind::Constants && b.code.contains("PLAYER_SPEED")));
        assert!(state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .any(|b| b.kind == CustomCodeKind::GameStateVars && b.code.contains("set_var")));

        let _ = std::fs::remove_dir_all(&root);
    }
}