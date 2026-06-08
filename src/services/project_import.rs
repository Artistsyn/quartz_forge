use std::cell::RefCell;
use std::collections::BTreeMap;
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
    CrystallineConfigProfile, CrystallineQuality, CustomCodeBlock, CustomCodeKind, LogicNode,
    LogicTree, ObjectPhysicsMaterialPreset, ObjectPhysicsMaterialSpec, QuartzAction,
    QuartzCondition, QuartzEventBinding, QuartzEventKind, QuartzExpr, QuartzExprKind,
    QuartzKeyModifiers, QuartzLocationRef, QuartzObjectCollisionMode,
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

impl SemanticImportReport {
    pub fn has_project_state_changes(&self) -> bool {
        !self.imported_files.is_empty() || !self.fallback_manual_override_files.is_empty()
    }
}

thread_local! {
    static IMPORT_NUMERIC_CONSTS: RefCell<BTreeMap<String, f32>> = RefCell::new(BTreeMap::new());
}

fn with_import_numeric_consts<T>(ast: &File, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let numeric_consts = collect_f32_constants_from_ast(ast);
    let previous = IMPORT_NUMERIC_CONSTS.with(|slot| {
        std::mem::replace(&mut *slot.borrow_mut(), numeric_consts)
    });

    let result = f();

    IMPORT_NUMERIC_CONSTS.with(|slot| {
        *slot.borrow_mut() = previous;
    });

    result
}

pub fn require_import_ticket_negative_coverage(has_negative_test: bool) -> Result<()> {
    if has_negative_test {
        Ok(())
    } else {
        anyhow::bail!("ticket completion gate failed; missing coverage: negative_tests")
    }
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

        if rel_path == "target" || rel_path.starts_with("target/") {
            report.notes.push(format!(
                "Skipped {} because target artifacts are not semantic-import sources.",
                rel_path
            ));
            continue;
        }

        let path = root.join(&rel_path);
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        let is_scaffold = is_scaffold_entry_file(&rel_path);

        match import_single_file(state, &rel_path, &content) {
            Ok(Some((objects, logic_trees, events, blocks))) => {
                report.imported_files.push(rel_path.clone());
                report.imported_object_count += objects;
                report.imported_logic_tree_count += logic_trees;
                report.imported_event_count += events;
                report.imported_custom_block_count += blocks;
            }
            Ok(None) => {
                if is_scaffold {
                    // Scaffold files (lib.rs, main.rs) are intentionally skipped.
                    // Purge any stale ManualFileOverride blocks from a previous import.
                    purge_manual_override_blocks_for_file(state, &rel_path);
                } else if fallback_manual_overrides {
                    let _ = state.track_manual_override_for_file(&rel_path, &content);
                    report.fallback_manual_override_files.push(rel_path);
                } else {
                    report.unsupported_files.push(rel_path);
                }
            }
            Err(err) => {
                if is_scaffold {
                    purge_manual_override_blocks_for_file(state, &rel_path);
                } else if fallback_manual_overrides {
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
    let ast = match syn::parse_file(content) {
        Ok(ast) => ast,
        Err(err) => {
            if is_scaffold_entry_file(rel_path) {
                return Ok(None);
            }
            if let Some(count) = import_raw_custom_code_file(state, rel_path, content) {
                return Ok(Some((0, 0, 0, count)));
            }
            return Err(err)
                .with_context(|| format!("failed to parse Rust file {} for semantic import", rel_path));
        }
    };

    with_import_numeric_consts(&ast, || {
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
            if is_scaffold_entry_file(rel_path) {
                return Ok(None);
            }
            if let Some(count) = import_raw_custom_code_file(state, rel_path, content) {
                return Ok(Some((0, 0, 0, count)));
            }
            return Ok(None);
        }

        remove_manual_override_for_file(state, scene_index, rel_path);
        Ok(Some((
            imported_objects,
            imported_logic_trees,
            imported_events,
            imported_custom_blocks,
        )))
    })
}

fn import_raw_custom_code_file(
    state: &mut EditorProjectState,
    rel_path: &str,
    content: &str,
) -> Option<usize> {
    let rel_path_norm = normalize_rel_like(rel_path);
    if state
        .manifest
        .scenes
        .iter()
        .any(|scene| normalize_rel_like(&scene.source_file) == rel_path_norm)
    {
        return None;
    }

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
    let u32_consts = collect_u32_constants_from_ast(ast);
    let numeric_consts = collect_f32_constants_from_ast(ast);
    for item in &ast.items {
        let Item::Fn(func) = item else { continue; };
        imported.extend(parse_objects_from_function(func, rel_path, &u32_consts, &numeric_consts)?);
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

fn parse_objects_from_function(
    func: &ItemFn,
    rel_path: &str,
    u32_consts: &std::collections::BTreeMap<String, u32>,
    numeric_consts: &std::collections::BTreeMap<String, f32>,
) -> Result<Vec<QuartzObjectBlueprint>> {
    if !function_is_object_template_candidate(func) {
        return Ok(Vec::new());
    }

    let mut imported = Vec::new();
    let mut built = std::collections::BTreeMap::<String, QuartzObjectBlueprint>::new();
    let function_name = func.sig.ident.to_string();

    for stmt in &func.block.stmts {
        match stmt {
            Stmt::Local(local) => {
                if let Some((var_name, blueprint)) =
                    parse_object_local(local, rel_path, &function_name, u32_consts, numeric_consts)?
                {
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
    function_name: &str,
    u32_consts: &std::collections::BTreeMap<String, u32>,
    numeric_consts: &std::collections::BTreeMap<String, f32>,
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
    let object_id = object_id.unwrap_or_else(|| format!("{}_{}", function_name, ident));

    let mut blueprint = QuartzObjectBlueprint::new(object_id.clone(), object_id.clone());
    blueprint.output_file = rel_path.to_owned();
    if function_name != "setup_scene" {
        blueprint.spawn_only = true;
    }
    apply_builder_methods(&mut blueprint, &methods, u32_consts, numeric_consts)?;
    Ok(Some((ident.to_string(), blueprint)))
}

fn parse_object_followup_expr(
    expr: &Expr,
    built: &mut std::collections::BTreeMap<String, QuartzObjectBlueprint>,
) -> Result<()> {
    if let Expr::Assign(assign) = expr {
        if let Expr::Field(field) = &*assign.left {
            if let (Some(var_name), Member::Named(member)) = (expr_ident_name(&field.base), &field.member)
                && member == "is_platform"
                && let Some(object) = built.get_mut(&var_name)
            {
                object.advanced.is_platform = expr_to_bool(&assign.right).unwrap_or(false);
            }
        }
    }

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
        } else if method == "set_drawable" {
            let Some(var_name) = expr_ident_name(&call.receiver) else {
                return Ok(());
            };
            let Some(object) = built.get_mut(&var_name) else {
                return Ok(());
            };
            if let Some(rgb) = parse_set_drawable_solid_circle_rgb(call) {
                object.color_rgb = rgb;
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
        if name != "register_logic" {
            continue;
        }

        let loops = parse_logic_trees_from_function(func, state, rel_path);
        imported.extend(loops);
    }

    if let Some(scene) = state.manifest.scenes.get_mut(scene_index) {
        let before = scene.logic_trees.len();
        scene.logic_trees.retain(|tree| tree.output_file != rel_path);
        scene.logic_trees.extend(imported.iter().cloned());
        if scene.logic_trees.len() != before || !imported.is_empty() {
            state.dirty = true;
        }
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
        if !is_canvas_on_update_call(call) {
            continue;
        }
        let Some(Expr::Closure(closure)) = call.args.first() else {
            continue;
        };
        if !closure_is_logic_tree_shape(closure) {
            continue;
        }

        let (id, name) = state.manifest.next_logic_tree_identity();
        let mut tree = LogicTree::new(id, format!("{}_imported", name));
        tree.output_file = rel_path.to_owned();
        tree.nodes = parse_logic_nodes_from_closure(closure);
        tree.refresh_references();
        out.push(tree);
    }
    out
}

fn closure_is_logic_tree_shape(closure: &ExprClosure) -> bool {
    let Expr::Block(block) = &*closure.body else {
        return false;
    };

    !block.block.stmts.is_empty()
        && block.block.stmts.iter().all(stmt_is_canvas_run_action)
}

fn stmt_is_canvas_run_action(stmt: &Stmt) -> bool {
    let Stmt::Expr(Expr::MethodCall(call), _) = stmt else {
        return false;
    };
    call.method == "run"
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

    if let Some(scene) = state.manifest.scenes.get_mut(scene_index) {
        let before = scene.events.len();
        scene.events.retain(|event| event.output_file != rel_path);
        scene.events.extend(imported.iter().cloned());
        if scene.events.len() != before || !imported.is_empty() {
            state.dirty = true;
        }
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
    let mut top_level_fallback_items = Vec::<String>::new();
    let skip_scaffold_fallback = is_scaffold_entry_file(rel_path);
    remove_imported_top_level_fallback_blocks(state, scene_index, rel_path);

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
            import_scene_canvas_from_setup_scene(state, scene_index, func);
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
            if let Some(code) = extract_setup_scene_runtime_statements(func) {
                upsert_named_custom_block(
                    state,
                    scene_index,
                    rel_path,
                    "setup_runtime",
                    "setup_runtime",
                    CustomCodeKind::TypedVars,
                    code,
                    None,
                );
                updated += 1;
            }
            continue;
        }

        if function_contains_object_builder(func) {
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

        if name == "register_logic" {
            let codes = extract_non_logic_tree_on_update_bodies(func);
            updated += sync_imported_update_loop_blocks(
                state,
                scene_index,
                rel_path,
                &name,
                &codes,
            );
            continue;
        }

        if let Some(id) = name.strip_prefix("register_update_") {
            let codes = extract_on_update_bodies(func);
            updated += sync_imported_update_loop_blocks(
                state,
                scene_index,
                rel_path,
                id,
                &codes,
            );
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

        let codes = extract_on_update_bodies(func);
        if !codes.is_empty() {
            updated += sync_imported_update_loop_blocks(
                state,
                scene_index,
                rel_path,
                &name,
                &codes,
            );
            continue;
        }

        if !skip_scaffold_fallback {
            top_level_fallback_items.push(item.to_token_stream().to_string());
        }
    }

    if !top_level_fallback_items.is_empty() {
        upsert_named_custom_block(
            state,
            scene_index,
            rel_path,
            "top_level_imported",
            "top_level_imported",
            CustomCodeKind::TopLevel,
            top_level_fallback_items.join("\n\n"),
            None,
        );
        updated += 1;
    }

    Ok(updated)
}

fn function_contains_object_builder(func: &ItemFn) -> bool {
    if !function_is_object_template_candidate(func) {
        return false;
    }

    func.block.stmts.iter().any(|stmt| {
        let Stmt::Local(local) = stmt else { return false; };
        let Some(init) = &local.init else { return false; };
        extract_builder_chain(&init.expr).is_some()
    })
}

fn function_is_object_template_candidate(func: &ItemFn) -> bool {
    let function_name = func.sig.ident.to_string();
    if function_name == "setup_scene" {
        return true;
    }

    match &func.sig.output {
        syn::ReturnType::Type(_, ty) => type_ends_with_ident(ty.as_ref(), "GameObject"),
        syn::ReturnType::Default => false,
    }
}

fn type_ends_with_ident(ty: &syn::Type, ident: &str) -> bool {
    match ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident == ident)
            .unwrap_or(false),
        syn::Type::Reference(reference) => type_ends_with_ident(reference.elem.as_ref(), ident),
        _ => false,
    }
}

fn is_scaffold_entry_file(rel_path: &str) -> bool {
    let rel = normalize_rel_like(rel_path);
    rel == "src/lib.rs"
        || rel == "src/main.rs"
        || rel.ends_with("/src/lib.rs")
        || rel.ends_with("/src/main.rs")
}

fn remove_imported_top_level_fallback_blocks(
    state: &mut EditorProjectState,
    scene_index: usize,
    rel_path: &str,
) {
    let Some(scene) = state.manifest.scenes.get_mut(scene_index) else {
        return;
    };

    let before = scene.custom_code_blocks.len();
    scene.custom_code_blocks.retain(|block| {
        !(block.kind == CustomCodeKind::TopLevel
            && block.output_file == rel_path
            && block.id == "top_level_imported")
    });
    if scene.custom_code_blocks.len() != before {
        state.dirty = true;
    }
}

/// Remove ALL ManualFileOverride blocks whose output_file matches rel_path, across all scenes.
fn purge_manual_override_blocks_for_file(state: &mut EditorProjectState, rel_path: &str) {
    for scene in &mut state.manifest.scenes {
        let before = scene.custom_code_blocks.len();
        scene.custom_code_blocks.retain(|block| {
            !(block.kind == CustomCodeKind::ManualFileOverride
                && block.output_file == rel_path)
        });
        if scene.custom_code_blocks.len() != before {
            state.dirty = true;
        }
    }
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

fn import_scene_canvas_from_setup_scene(
    state: &mut EditorProjectState,
    scene_index: usize,
    func: &ItemFn,
) {
    let Some(scene) = state.manifest.scenes.get_mut(scene_index) else {
        return;
    };

    let mut changed = false;
    for stmt in &func.block.stmts {
        let Stmt::Expr(Expr::MethodCall(call), _) = stmt else { continue; };
        let Some(receiver_name) = expr_ident_name(&call.receiver) else { continue; };
        if receiver_name != "canvas" {
            continue;
        }

        let method = call.method.to_string();
        match method.as_str() {
            "enable_crystalline" => {
                if !scene.canvas.crystalline_enabled {
                    scene.canvas.crystalline_enabled = true;
                    changed = true;
                }
            }
            "enable_crystalline_with" if call.args.len() == 1 => {
                if !scene.canvas.crystalline_enabled {
                    scene.canvas.crystalline_enabled = true;
                    changed = true;
                }
                if let Some(profile) = parse_crystalline_profile_expr(&call.args[0])
                    && profile != scene.canvas.crystalline_profile
                {
                    scene.canvas.crystalline_profile = profile;
                    changed = true;
                }
            }
            "set_physics_quality" if call.args.len() == 1 => {
                if let Some(quality) = parse_crystalline_quality_expr(&call.args[0])
                    && quality != scene.canvas.crystalline_quality
                {
                    scene.canvas.crystalline_quality = quality;
                    changed = true;
                }
            }
            "run" if call.args.len() == 1 => {
                match path_last_ident(&call.args[0]).as_deref() {
                    Some("EnableCrystalline") => {
                        if !scene.canvas.crystalline_enabled {
                            scene.canvas.crystalline_enabled = true;
                            changed = true;
                        }
                    }
                    Some("DisableCrystalline") => {
                        if scene.canvas.crystalline_enabled {
                            scene.canvas.crystalline_enabled = false;
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    if changed {
        state.dirty = true;
    }
}

fn parse_crystalline_profile_expr(expr: &Expr) -> Option<CrystallineConfigProfile> {
    let ident = path_last_ident(expr)?;
    match ident.as_str() {
        "platformer" | "Platformer" => Some(CrystallineConfigProfile::Platformer),
        "floaty" | "Floaty" => Some(CrystallineConfigProfile::Floaty),
        "realistic" | "Realistic" => Some(CrystallineConfigProfile::Realistic),
        "arcade" | "Arcade" => Some(CrystallineConfigProfile::Arcade),
        _ => None,
    }
}

fn parse_crystalline_quality_expr(expr: &Expr) -> Option<CrystallineQuality> {
    let ident = path_last_ident(expr)?;
    match ident.as_str() {
        "Low" => Some(CrystallineQuality::Low),
        "Medium" => Some(CrystallineQuality::Medium),
        "High" => Some(CrystallineQuality::High),
        _ => None,
    }
}

fn import_custom_events_from_register_events(
    state: &mut EditorProjectState,
    scene_index: usize,
    rel_path: &str,
    func: &ItemFn,
) -> usize {
    let Some(scene) = state.manifest.scenes.get_mut(scene_index) else {
        return 0;
    };

    let before = scene.custom_code_blocks.len();
    scene.custom_code_blocks.retain(|block| {
        !(block.kind == CustomCodeKind::CustomEvents
            && block.output_file == rel_path
            && block.id.starts_with("custom_event_"))
    });
    if scene.custom_code_blocks.len() != before {
        state.dirty = true;
    }

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

fn extract_on_update_bodies(func: &ItemFn) -> Vec<String> {
    let mut out = Vec::new();
    for stmt in &func.block.stmts {
        let Stmt::Expr(Expr::MethodCall(call), _) = stmt else { continue; };
        if !is_canvas_on_update_call(call) { continue; }
        let Some(Expr::Closure(closure)) = call.args.first() else { continue; };
        if let Some(code) = closure_body_code(closure) {
            out.push(code);
        }
    }
    out
}

fn extract_non_logic_tree_on_update_bodies(func: &ItemFn) -> Vec<String> {
    let mut out = Vec::new();
    for stmt in &func.block.stmts {
        let Stmt::Expr(Expr::MethodCall(call), _) = stmt else { continue; };
        if !is_canvas_on_update_call(call) { continue; }
        let Some(Expr::Closure(closure)) = call.args.first() else { continue; };
        if closure_is_logic_tree_shape(closure) {
            continue;
        }
        if let Some(code) = closure_body_code(closure) {
            out.push(code);
        }
    }
    out
}

fn sync_imported_update_loop_blocks(
    state: &mut EditorProjectState,
    scene_index: usize,
    rel_path: &str,
    source_name: &str,
    codes: &[String],
) -> usize {
    let prefix = format!("imported_update_loop_{}_", sanitize_identifier(source_name));
    let names = if codes.len() == 1 {
        vec![source_name.to_owned()]
    } else {
        (1..=codes.len())
            .map(|idx| format!("{}_{}", source_name, idx))
            .collect::<Vec<_>>()
    };

    let Some(scene) = state.manifest.scenes.get_mut(scene_index) else {
        return 0;
    };

    let before = scene.custom_code_blocks.len();
    scene.custom_code_blocks.retain(|block| {
        !(block.kind == CustomCodeKind::UpdateLoops
            && block.output_file == rel_path
            && block.id.starts_with(&prefix))
    });

    for (idx, code) in codes.iter().enumerate() {
        let mut block = CustomCodeBlock::new(
            format!("{}{}", prefix, idx + 1),
            names[idx].clone(),
            CustomCodeKind::UpdateLoops,
            rel_path.to_owned(),
        );
        block.code = code.clone();
        scene.custom_code_blocks.push(block);
    }

    if scene.custom_code_blocks.len() != before || !codes.is_empty() {
        state.dirty = true;
    }

    codes.len()
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

fn extract_builder_chain(expr: &Expr) -> Option<(Option<String>, Vec<(String, Vec<Expr>)>)> {
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
                let object_id = args.first().and_then(extract_string_literal);
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
    u32_consts: &std::collections::BTreeMap<String, u32>,
    numeric_consts: &std::collections::BTreeMap<String, f32>,
) -> Result<()> {
    for (method, args) in methods {
        match method.as_str() {
            "size" if args.len() == 2 => {
                if let Ok(value) = expr_to_f32_with_consts(&args[0], numeric_consts) {
                    object.w = value;
                }
                if let Ok(value) = expr_to_f32_with_consts(&args[1], numeric_consts) {
                    object.h = value;
                }
            }
            "position" if args.len() == 2 => {
                if let Ok(value) = expr_to_f32_with_consts(&args[0], numeric_consts) {
                    object.x = value;
                }
                if let Ok(value) = expr_to_f32_with_consts(&args[1], numeric_consts) {
                    object.y = value;
                }
            }
            "layer" if args.len() == 1 => object.layer = expr_to_i32(&args[0])?,
            "momentum" if args.len() == 2 => {
                if let Ok(value) = expr_to_f32_with_consts(&args[0], numeric_consts) {
                    object.advanced.momentum_x = value;
                }
                if let Ok(value) = expr_to_f32_with_consts(&args[1], numeric_consts) {
                    object.advanced.momentum_y = value;
                }
            }
            "resistance" if args.len() == 2 => {
                if let Ok(value) = expr_to_f32_with_consts(&args[0], numeric_consts) {
                    object.advanced.resistance_x = value;
                }
                if let Ok(value) = expr_to_f32_with_consts(&args[1], numeric_consts) {
                    object.advanced.resistance_y = value;
                }
            }
            "gravity" if args.len() == 1 => {
                if let Ok(value) = expr_to_f32_with_consts(&args[0], numeric_consts) {
                    object.advanced.gravity = value;
                }
            }
            "rotation" if args.len() == 1 => {
                if let Ok(value) = expr_to_f32_with_consts(&args[0], numeric_consts) {
                    object.advanced.rotation_deg = value;
                }
            }
            "rotation_resistance" if args.len() == 1 => {
                object.advanced.rotation_resistance = expr_to_f32_with_consts(&args[0], numeric_consts)?;
            }
            "rotate_around_center" => {
                object.advanced.pivot_x = 0.5;
                object.advanced.pivot_y = 0.5;
            }
            "pivot" if args.len() == 2 => {
                object.advanced.pivot_x = expr_to_f32_with_consts(&args[0], numeric_consts)?;
                object.advanced.pivot_y = expr_to_f32_with_consts(&args[1], numeric_consts)?;
            }
            "center_at" if args.len() == 2 => {
                let cx = expr_to_f32_with_consts(&args[0], numeric_consts)?;
                let cy = expr_to_f32_with_consts(&args[1], numeric_consts)?;
                object.x = cx - (object.w * 0.5);
                object.y = cy - (object.h * 0.5);
            }
            "material" if args.len() == 1 => object.advanced.material = expr_to_material(&args[0])?,
            "bouncy" => {
                object.advanced.material = ObjectPhysicsMaterialSpec::resolved_defaults(
                    ObjectPhysicsMaterialPreset::Bouncy,
                );
            }
            "slippery" => {
                object.advanced.material =
                    ObjectPhysicsMaterialSpec::resolved_defaults(ObjectPhysicsMaterialPreset::Ice);
            }
            "heavy" => {
                object.advanced.material =
                    ObjectPhysicsMaterialSpec::resolved_defaults(ObjectPhysicsMaterialPreset::Metal);
            }
            "light" => {
                object.advanced.material = ObjectPhysicsMaterialSpec::resolved_defaults(
                    ObjectPhysicsMaterialPreset::Feather,
                );
            }
            "rubber" => {
                object.advanced.material = ObjectPhysicsMaterialSpec::resolved_defaults(
                    ObjectPhysicsMaterialPreset::Rubber,
                );
            }
            "static_object" => {
                object.advanced.gravity = 0.0;
                object.advanced.resistance_x = 0.0;
                object.advanced.resistance_y = 0.0;
            }
            "collision_layer" if args.len() == 1 => {
                if let Ok(value) = expr_to_u32_with_consts(&args[0], u32_consts) {
                    object.advanced.collision_layer = value;
                }
            }
            "collision_mask" if args.len() == 1 => {
                if let Ok(value) = expr_to_u32_with_consts(&args[0], u32_consts) {
                    object.advanced.collision_mask = value;
                }
            }
            "player_layer" => {
                object.advanced.collision_layer = 1 << 1;
                object.advanced.collision_mask = u32::MAX;
            }
            "enemy_layer" => {
                object.advanced.collision_layer = 1 << 2;
                object.advanced.collision_mask = (1 << 1) | (1 << 3) | (1 << 6);
            }
            "projectile_layer" => {
                object.advanced.collision_layer = 1 << 3;
                object.advanced.collision_mask = (1 << 2) | (1 << 6);
            }
            "collision_mode" if args.len() == 1 => {
                if let Some((mode, radius)) = parse_collision_mode_expr(&args[0]) {
                    object.advanced.collision_mode = mode;
                    if let Some(radius) = radius {
                        object.advanced.collision_circle_radius = radius;
                    }
                    object.advanced.is_platform = !matches!(mode, QuartzObjectCollisionMode::NonPlatform);
                }
            }
            "clip" => object.advanced.clip_enabled = true,
            "clip_origin" if args.len() == 2 => {
                object.advanced.clip_enabled = true;
                object.advanced.clip_origin_enabled = true;
                object.advanced.clip_origin_x = expr_to_f32(&args[0])?;
                object.advanced.clip_origin_y = expr_to_f32(&args[1])?;
            }
            "clip_size" if args.len() == 2 => {
                object.advanced.clip_enabled = true;
                object.advanced.clip_size_enabled = true;
                object.advanced.clip_size_w = expr_to_f32_with_consts(&args[0], numeric_consts)?;
                object.advanced.clip_size_h = expr_to_f32_with_consts(&args[1], numeric_consts)?;
            }
            "platform" | "floor" | "ceiling" | "wall_left" | "wall_right" => {
                object.advanced.is_platform = true;
                object.advanced.collision_mode = QuartzObjectCollisionMode::Surface;
            }
            "no_collision" => {
                object.advanced.is_platform = false;
                object.advanced.collision_mode = QuartzObjectCollisionMode::NonPlatform;
                object.advanced.collision_layer = 0;
                object.advanced.collision_mask = 0;
            }
            "solid" => {
                object.advanced.is_platform = true;
                object.advanced.collision_mode = QuartzObjectCollisionMode::SolidRectangle;
                object.template = crate::core::quartz_domain::ObjectTemplate::Rectangle;
            }
            "solid_circle" if args.len() == 1 => {
                object.advanced.is_platform = true;
                object.advanced.collision_mode = QuartzObjectCollisionMode::SolidCircle;
                object.advanced.collision_circle_radius = expr_to_f32_with_consts(&args[0], numeric_consts)?;
                object.template = crate::core::quartz_domain::ObjectTemplate::Circle;
            }
            "image" if args.len() == 1 => {
                if let Some(parsed) = parse_load_image_expr(&args[0]) {
                    object.visual_asset_mode = crate::core::quartz_domain::ObjectVisualAssetMode::StaticImage;
                    object.visual_asset_path = parsed.asset_path;
                    object.visual_asset_use_canvas_cache = parsed.use_canvas_cache;
                    object.visual_asset_cache_key = parsed.cache_key.unwrap_or_default();
                    object.visual_asset_size_aware_cache = parsed.size_aware_cache;
                }
            }
            "tint" if args.len() == 1 => {
                if let Some(rgba) = expr_to_rgba_u8(&args[0]) {
                    object.advanced.tint_enabled = true;
                    object.advanced.tint_rgba = rgba;
                }
            }
            "glow" if args.len() == 1 => {
                if let Some((rgba, width)) = parse_glow_config_expr(&args[0]) {
                    object.advanced.glow_enabled = true;
                    object.advanced.glow_rgba = rgba;
                    object.advanced.glow_width = width;
                }
            }
            "highlight" if args.len() == 1 => {
                if let Some((tint, glow)) = parse_highlight_effect_expr(&args[0]) {
                    if let Some(rgba) = tint {
                        object.advanced.tint_enabled = true;
                        object.advanced.tint_rgba = rgba;
                    }
                    if let Some((rgba, width)) = glow {
                        object.advanced.glow_enabled = true;
                        object.advanced.glow_rgba = rgba;
                        object.advanced.glow_width = width;
                    }
                }
            }
            "screen_space" => object.advanced.set_camera_space_pinned(true),
            "ignore_zoom" => object.advanced.ignore_zoom = true,
            "pin" if args.len() == 2 => {
                object.advanced.screen_pin_enabled = true;
                object.advanced.screen_space = false;
                object.advanced.ignore_zoom = true;
                object.advanced.screen_pin_anchor_x = expr_to_f32_with_consts(&args[0], numeric_consts)?;
                object.advanced.screen_pin_anchor_y = expr_to_f32_with_consts(&args[1], numeric_consts)?;
            }
            "pin_offset" if args.len() == 2 => {
                object.advanced.screen_pin_enabled = true;
                object.advanced.screen_space = false;
                object.advanced.ignore_zoom = true;
                object.advanced.screen_pin_offset_x = expr_to_f32_with_consts(&args[0], numeric_consts)?;
                object.advanced.screen_pin_offset_y = expr_to_f32_with_consts(&args[1], numeric_consts)?;
            }
            "pin_top_left" if args.len() == 2 => {
                object.advanced.screen_pin_enabled = true;
                object.advanced.screen_space = false;
                object.advanced.ignore_zoom = true;
                object.advanced.screen_pin_anchor_x = 0.0;
                object.advanced.screen_pin_anchor_y = 0.0;
                object.advanced.screen_pin_offset_x = expr_to_f32_with_consts(&args[0], numeric_consts)?;
                object.advanced.screen_pin_offset_y = expr_to_f32_with_consts(&args[1], numeric_consts)?;
            }
            "pin_top_right" if args.len() == 2 => {
                object.advanced.screen_pin_enabled = true;
                object.advanced.screen_space = false;
                object.advanced.ignore_zoom = true;
                object.advanced.screen_pin_anchor_x = 1.0;
                object.advanced.screen_pin_anchor_y = 0.0;
                object.advanced.screen_pin_offset_x = expr_to_f32_with_consts(&args[0], numeric_consts)?;
                object.advanced.screen_pin_offset_y = expr_to_f32_with_consts(&args[1], numeric_consts)?;
            }
            "pin_top_center" if args.len() == 1 => {
                object.advanced.screen_pin_enabled = true;
                object.advanced.screen_space = false;
                object.advanced.ignore_zoom = true;
                object.advanced.screen_pin_anchor_x = 0.5;
                object.advanced.screen_pin_anchor_y = 0.0;
                object.advanced.screen_pin_offset_x = 0.0;
                object.advanced.screen_pin_offset_y = expr_to_f32_with_consts(&args[0], numeric_consts)?;
            }
            "pin_bottom_left" if args.len() == 2 => {
                object.advanced.screen_pin_enabled = true;
                object.advanced.screen_space = false;
                object.advanced.ignore_zoom = true;
                object.advanced.screen_pin_anchor_x = 0.0;
                object.advanced.screen_pin_anchor_y = 1.0;
                object.advanced.screen_pin_offset_x = expr_to_f32_with_consts(&args[0], numeric_consts)?;
                object.advanced.screen_pin_offset_y = expr_to_f32_with_consts(&args[1], numeric_consts)?;
            }
            "pin_bottom_right" if args.len() == 2 => {
                object.advanced.screen_pin_enabled = true;
                object.advanced.screen_space = false;
                object.advanced.ignore_zoom = true;
                object.advanced.screen_pin_anchor_x = 1.0;
                object.advanced.screen_pin_anchor_y = 1.0;
                object.advanced.screen_pin_offset_x = expr_to_f32_with_consts(&args[0], numeric_consts)?;
                object.advanced.screen_pin_offset_y = expr_to_f32_with_consts(&args[1], numeric_consts)?;
            }
            "pin_bottom_center" if args.len() == 1 => {
                object.advanced.screen_pin_enabled = true;
                object.advanced.screen_space = false;
                object.advanced.ignore_zoom = true;
                object.advanced.screen_pin_anchor_x = 0.5;
                object.advanced.screen_pin_anchor_y = 1.0;
                object.advanced.screen_pin_offset_x = 0.0;
                object.advanced.screen_pin_offset_y = expr_to_f32_with_consts(&args[0], numeric_consts)?;
            }
            "pin_center" => {
                object.advanced.screen_pin_enabled = true;
                object.advanced.screen_space = false;
                object.advanced.ignore_zoom = true;
                object.advanced.screen_pin_anchor_x = 0.5;
                object.advanced.screen_pin_anchor_y = 0.5;
                object.advanced.screen_pin_offset_x = 0.0;
                object.advanced.screen_pin_offset_y = 0.0;
            }
            "fill_screen" => {
                object.advanced.screen_pin_enabled = true;
                object.advanced.screen_space = false;
                object.advanced.ignore_zoom = true;
                object.advanced.screen_pin_anchor_x = 0.0;
                object.advanced.screen_pin_anchor_y = 0.0;
                object.advanced.screen_pin_offset_x = 0.0;
                object.advanced.screen_pin_offset_y = 0.0;
            }
            "tag" if args.len() == 1 => {
                if let Some(tag) = extract_string_literal(&args[0]) {
                    object.tags.push(tag);
                }
            }
            "slope_auto_rotation" if args.len() == 2 => {
                object.advanced.slope_enabled = true;
                object.advanced.slope_auto_rotation = true;
                object.advanced.slope_left_offset = expr_to_f32_with_consts(&args[0], numeric_consts)?;
                object.advanced.slope_right_offset = expr_to_f32_with_consts(&args[1], numeric_consts)?;
            }
            "slope" if args.len() == 2 => {
                object.advanced.slope_enabled = true;
                object.advanced.slope_left_offset = expr_to_f32_with_consts(&args[0], numeric_consts)?;
                object.advanced.slope_right_offset = expr_to_f32_with_consts(&args[1], numeric_consts)?;
            }
            "one_way" => object.advanced.one_way = true,
            "surface_velocity" if args.len() == 1 => {
                object.advanced.surface_velocity_enabled = true;
                object.advanced.surface_velocity_x = expr_to_f32_with_consts(&args[0], numeric_consts)?;
            }
            "surface" if args.len() == 2 => {
                object.advanced.surface_normal_enabled = true;
                object.advanced.surface_normal_x = expr_to_f32_with_consts(&args[0], numeric_consts)?;
                object.advanced.surface_normal_y = expr_to_f32_with_consts(&args[1], numeric_consts)?;
            }
            "align_to_slope" => object.advanced.align_to_slope = true,
            "align_to_slope_speed" if args.len() == 1 => {
                object.advanced.align_to_slope_speed = expr_to_f32_with_consts(&args[0], numeric_consts)?
            }
            "planet" if args.len() == 1 => {
                object.advanced.planet_enabled = true;
                object.advanced.planet_radius = expr_to_f32_with_consts(&args[0], numeric_consts)?;
            }
            "gravity_well" if args.len() == 2 => {
                object.advanced.planet_enabled = true;
                object.advanced.planet_radius = expr_to_f32_with_consts(&args[0], numeric_consts)?;
                object.advanced.gravity_strength = expr_to_f32_with_consts(&args[1], numeric_consts)?;
                object.advanced.is_platform = false;
                object.advanced.collision_mode = QuartzObjectCollisionMode::NonPlatform;
            }
            "gravity_target" if args.len() == 1 => {
                object.advanced.gravity_target_enabled = true;
                object.advanced.gravity_target_tag = extract_string_literal(&args[0]).unwrap_or_default();
            }
            "gravity_strength" if args.len() == 1 => {
                object.advanced.gravity_strength = expr_to_f32_with_consts(&args[0], numeric_consts)?
            }
            "gravity_influence_mult" if args.len() == 1 => {
                object.advanced.gravity_influence_mult = expr_to_f32_with_consts(&args[0], numeric_consts)?
            }
            "gravity_falloff" if args.len() == 1 => {
                if let Some(value) = path_last_ident(&args[0]) {
                    object.advanced.gravity_falloff = match value.as_str() {
                        "Constant" => crate::core::quartz_domain::QuartzGravityFalloff::Constant,
                        "InverseSquare" => crate::core::quartz_domain::QuartzGravityFalloff::InverseSquare,
                        _ => crate::core::quartz_domain::QuartzGravityFalloff::Linear,
                    };
                }
            }
            "gravity_all_sources" | "all_gravity_sources" => {
                object.advanced.gravity_all_sources = true
            }
            "celestial_physics" => {
                object.advanced.gravity_all_sources = true;
                object.advanced.gravity_falloff = crate::core::quartz_domain::QuartzGravityFalloff::InverseSquare;
            }
            "unlimited_gravity_range" => object.advanced.gravity_influence_mult = f32::MAX,
            "gravity_identity" if args.len() == 1 => {
                object.advanced.gravity_identity_enabled = true;
                object.advanced.gravity_identity = extract_string_literal(&args[0]).unwrap_or_default();
            }
            "auto_align" => object.advanced.auto_align = true,
            "auto_align_speed" if args.len() == 1 => {
                object.advanced.auto_align_speed = expr_to_f32_with_consts(&args[0], numeric_consts)?
            }
            "auto_align_threshold" if args.len() == 1 => {
                object.advanced.auto_align_threshold = expr_to_f32_with_consts(&args[0], numeric_consts)?;
            }
            "auto_align_min_depth" if args.len() == 1 => {
                object.advanced.auto_align_min_depth = expr_to_f32_with_consts(&args[0], numeric_consts)?;
            }
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

fn parse_collision_mode_expr(expr: &Expr) -> Option<(QuartzObjectCollisionMode, Option<f32>)> {
    match expr {
        Expr::Path(path) => {
            let last = path.path.segments.last()?.ident.to_string();
            if last == "Surface" {
                return Some((QuartzObjectCollisionMode::Surface, None));
            }
            if last == "NonPlatform" {
                return Some((QuartzObjectCollisionMode::NonPlatform, None));
            }
            None
        }
        Expr::Call(call) => {
            let name = path_last_ident(&call.func)?;
            match name.as_str() {
                "non_platform" => Some((QuartzObjectCollisionMode::NonPlatform, None)),
                "solid" => Some((QuartzObjectCollisionMode::SolidRectangle, None)),
                "solid_circle" => {
                    let radius = call.args.first().and_then(|arg| expr_to_f32(arg).ok());
                    Some((QuartzObjectCollisionMode::SolidCircle, radius))
                }
                _ => None,
            }
        }
        Expr::Paren(paren) => parse_collision_mode_expr(&paren.expr),
        _ => None,
    }
}

fn parse_glow_config_expr(expr: &Expr) -> Option<([u8; 4], f32)> {
    let Expr::Struct(ExprStruct { fields, .. }) = expr else {
        return None;
    };

    let mut color = [255u8, 255u8, 255u8, 255u8];
    let mut width = 4.0f32;
    for field in fields {
        let Member::Named(name) = &field.member else { continue; };
        match name.to_string().as_str() {
            "color" => {
                if let Some(parsed) = expr_to_rgba_u8(&field.expr) {
                    color = parsed;
                }
            }
            "width" => {
                if let Ok(parsed) = expr_to_f32(&field.expr) {
                    width = parsed;
                }
            }
            _ => {}
        }
    }
    Some((color, width))
}

fn parse_optional_glow_expr(expr: &Expr) -> Option<Option<([u8; 4], f32)>> {
    if let Expr::Path(path) = expr
        && path.path.is_ident("None")
    {
        return Some(None);
    }

    if let Expr::Call(call) = expr
        && let Some(name) = path_last_ident(&call.func)
        && name == "Some"
    {
        return call.args.first().and_then(parse_glow_config_expr).map(Some);
    }

    parse_glow_config_expr(expr).map(Some)
}

fn parse_highlight_effect_expr(
    expr: &Expr,
) -> Option<(Option<[u8; 4]>, Option<([u8; 4], f32)>)> {
    let Expr::Struct(ExprStruct { fields, .. }) = expr else {
        return None;
    };

    let mut tint: Option<[u8; 4]> = None;
    let mut glow: Option<([u8; 4], f32)> = None;
    for field in fields {
        let Member::Named(name) = &field.member else { continue; };
        match name.to_string().as_str() {
            "tint" => {
                tint = parse_optional_rgba_u8_expr(&field.expr).unwrap_or(None);
            }
            "glow" => {
                glow = parse_optional_glow_expr(&field.expr).unwrap_or(None);
            }
            _ => {}
        }
    }

    Some((tint, glow))
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

fn parse_set_drawable_solid_circle_rgb(call: &ExprMethodCall) -> Option<[u8; 3]> {
    let drawable = call.args.first()?;
    let inner = unwrap_box_new_expr(drawable);
    let Expr::Call(ExprCall { func, args, .. }) = inner else {
        return None;
    };
    if path_last_ident(func).as_deref() != Some("solid_circle") || args.len() < 2 {
        return None;
    }
    let rgba = expr_to_rgba_u8(&args[1])?;
    Some([rgba[0], rgba[1], rgba[2]])
}

fn unwrap_box_new_expr<'a>(expr: &'a Expr) -> &'a Expr {
    if let Expr::Call(ExprCall { func, args, .. }) = expr
        && path_last_ident(func).as_deref() == Some("new")
    {
        if let Expr::Path(path) = &**func {
            let mut segments = path.path.segments.iter();
            let is_box_new = matches!(
                (segments.next(), segments.next(), segments.next()),
                (Some(first), Some(second), None) if first.ident == "Box" && second.ident == "new"
            );
            if is_box_new {
                if let Some(inner) = args.first() {
                    return inner;
                }
            }
        }
    }
    expr
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
    if let Expr::Path(ExprPath { path, .. }) = expr {
        match path.segments.last().map(|seg| seg.ident.to_string()).as_deref() {
            Some("EnableCrystalline") => return QuartzAction::EnableCrystalline,
            Some("DisableCrystalline") => return QuartzAction::DisableCrystalline,
            _ => {
                return QuartzAction::Expr {
                    raw: expr.to_token_stream().to_string(),
                }
            }
        }
    }
    if let Expr::Call(ExprCall { func, args, .. }) = expr {
        match path_last_ident(func).as_deref() {
            Some("expr") => {
                if let Some(raw) = args.first().and_then(extract_to_owned_string) {
                    return QuartzAction::Expr { raw };
                }
                return QuartzAction::Expr {
                    raw: expr.to_token_stream().to_string(),
                };
            }
            Some("Multi") => {
                let actions = args
                    .first()
                    .map(parse_action_list_expr)
                    .unwrap_or_default();
                return QuartzAction::Multi { actions };
            }
            _ => {}
        }
    }
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
        Some("Conditional") => {
            let mut condition = QuartzCondition::Expr {
                raw: "true".to_owned(),
            };
            let mut if_true = QuartzAction::Expr {
                raw: "Action::Custom { name: \"noop\".to_owned() }".to_owned(),
            };
            let mut if_false: Option<Box<QuartzAction>> = None;

            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "condition" => {
                        condition = parse_condition_expr(&field.expr);
                    }
                    "if_true" => {
                        if_true = parse_boxed_action_expr(&field.expr)
                            .unwrap_or_else(|| parse_action_expr(&field.expr));
                    }
                    "if_false" => {
                        if_false = parse_optional_boxed_action_expr(&field.expr);
                    }
                    _ => {}
                }
            }

            QuartzAction::Conditional {
                condition,
                if_true: Box::new(if_true),
                if_false,
            }
        }
        Some("Multi") => {
            let mut actions = Vec::new();
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                if member == "actions" {
                    actions = parse_action_list_expr(&field.expr);
                }
            }
            QuartzAction::Multi { actions }
        }
        Some("ApplyForce") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut fx = 0.0f32;
            let mut fy = 0.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "fx" => fx = expr_to_f32(&field.expr).unwrap_or(0.0),
                    "fy" => fy = expr_to_f32(&field.expr).unwrap_or(0.0),
                    _ => {}
                }
            }
            QuartzAction::ApplyForce { target, fx, fy }
        }
        Some("ApplyImpulse") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut ix = 0.0f32;
            let mut iy = 0.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "ix" => ix = expr_to_f32(&field.expr).unwrap_or(0.0),
                    "iy" => iy = expr_to_f32(&field.expr).unwrap_or(0.0),
                    _ => {}
                }
            }
            QuartzAction::ApplyImpulse { target, ix, iy }
        }
        Some("SetPosition") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut x = 0.0f32;
            let mut y = 0.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "x" => x = expr_to_f32(&field.expr).unwrap_or(0.0),
                    "y" => y = expr_to_f32(&field.expr).unwrap_or(0.0),
                    _ => {}
                }
            }
            QuartzAction::SetPosition { target, x, y }
        }
        Some("AddRotation") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut deg = 0.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "deg" | "value" => deg = expr_to_f32(&field.expr).unwrap_or(0.0),
                    _ => {}
                }
            }
            QuartzAction::AddRotation { target, deg }
        }
        Some("ApplyRotation") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut deg = 0.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "deg" | "value" => deg = expr_to_f32(&field.expr).unwrap_or(0.0),
                    _ => {}
                }
            }
            QuartzAction::ApplyRotation { target, deg }
        }
        Some("SetMaterial") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut material = ObjectPhysicsMaterialSpec::resolved_defaults(ObjectPhysicsMaterialPreset::Default);
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "material" => {
                        material = expr_to_material(&field.expr).unwrap_or_else(|_| {
                            ObjectPhysicsMaterialSpec::resolved_defaults(ObjectPhysicsMaterialPreset::Default)
                        });
                    }
                    _ => {}
                }
            }
            QuartzAction::SetMaterial { target, material }
        }
        Some("SetDensity") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut value = 1.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "value" => value = expr_to_f32(&field.expr).unwrap_or(1.0),
                    _ => {}
                }
            }
            QuartzAction::SetDensity { target, value }
        }
        Some("SetElasticity") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut value = 0.5f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "value" => value = expr_to_f32(&field.expr).unwrap_or(0.5),
                    _ => {}
                }
            }
            QuartzAction::SetElasticity { target, value }
        }
        Some("SetFriction") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut value = 0.5f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "value" => value = expr_to_f32(&field.expr).unwrap_or(0.5),
                    _ => {}
                }
            }
            QuartzAction::SetFriction { target, value }
        }
        Some("FreezeBody") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                if member == "target" {
                    if let Some(parsed) = parse_target_ref(&field.expr) {
                        target = parsed;
                    }
                }
            }
            QuartzAction::FreezeBody { target }
        }
        Some("UnfreezeBody") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                if member == "target" {
                    if let Some(parsed) = parse_target_ref(&field.expr) {
                        target = parsed;
                    }
                }
            }
            QuartzAction::UnfreezeBody { target }
        }
        Some("WakeBody") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                if member == "target" {
                    if let Some(parsed) = parse_target_ref(&field.expr) {
                        target = parsed;
                    }
                }
            }
            QuartzAction::WakeBody { target }
        }
        Some("SetPhysicsQuality") => {
            let mut quality = "Medium".to_owned();
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                if member == "quality" {
                    quality = path_last_ident(&field.expr)
                        .or_else(|| extract_to_owned_string(&field.expr))
                        .unwrap_or_else(|| "Medium".to_owned());
                }
            }
            QuartzAction::SetPhysicsQuality { quality }
        }
        Some("SetCollisionMode") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut mode = "Solid".to_owned();
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "mode" => {
                        mode = path_last_ident(&field.expr)
                            .or_else(|| extract_to_owned_string(&field.expr))
                            .unwrap_or_else(|| "Solid".to_owned());
                    }
                    _ => {}
                }
            }
            QuartzAction::SetCollisionMode { target, mode }
        }
        Some("SetSlope") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut left_offset = 0.0f32;
            let mut right_offset = 0.0f32;
            let mut auto_rotate = false;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "left_offset" => left_offset = expr_to_f32(&field.expr).unwrap_or(0.0),
                    "right_offset" => right_offset = expr_to_f32(&field.expr).unwrap_or(0.0),
                    "auto_rotate" => auto_rotate = expr_to_bool(&field.expr).unwrap_or(false),
                    _ => {}
                }
            }
            QuartzAction::SetSlope {
                target,
                left_offset,
                right_offset,
                auto_rotate,
            }
        }
        Some("SetSurfaceNormal") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut nx = 0.0f32;
            let mut ny = -1.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "nx" => nx = expr_to_f32(&field.expr).unwrap_or(0.0),
                    "ny" => ny = expr_to_f32(&field.expr).unwrap_or(-1.0),
                    _ => {}
                }
            }
            QuartzAction::SetSurfaceNormal { target, nx, ny }
        }
        Some("TransferMomentum") => {
            let mut from = QuartzTargetRef::Name("player".to_owned());
            let mut to = QuartzTargetRef::Name("crate".to_owned());
            let mut scale = 1.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "from" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            from = parsed;
                        }
                    }
                    "to" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            to = parsed;
                        }
                    }
                    "scale" => scale = expr_to_f32(&field.expr).unwrap_or(1.0),
                    _ => {}
                }
            }
            QuartzAction::TransferMomentum { from, to, scale }
        }
        Some("SpawnEmitter") => {
            let mut name = "emitter".to_owned();
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                if member == "emitter" {
                    name = parse_emitter_name_expr(&field.expr).unwrap_or_else(|| "emitter".to_owned());
                }
            }
            QuartzAction::SpawnEmitter { name }
        }
        Some("RemoveEmitter") => {
            let mut name = String::new();
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                if member == "name" {
                    name = extract_to_owned_string(&field.expr).unwrap_or_default();
                }
            }
            QuartzAction::RemoveEmitter { name }
        }
        Some("AttachEmitter") => {
            let mut emitter_name = String::new();
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut location: Option<QuartzLocationRef> = None;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "emitter_name" => {
                        emitter_name = extract_to_owned_string(&field.expr).unwrap_or_default();
                    }
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "location" => {
                        location = parse_optional_location_ref_expr(&field.expr).unwrap_or(None);
                    }
                    _ => {}
                }
            }
            QuartzAction::AttachEmitter {
                emitter_name,
                target,
                location,
            }
        }
        Some("DetachEmitter") => {
            let mut emitter_name = String::new();
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                if member == "emitter_name" {
                    emitter_name = extract_to_owned_string(&field.expr).unwrap_or_default();
                }
            }
            QuartzAction::DetachEmitter { emitter_name }
        }
        Some("SetEmitterRate") => {
            let mut name = String::new();
            let mut value = 0.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "value" => value = expr_to_f32(&field.expr).unwrap_or(0.0),
                    _ => {}
                }
            }
            QuartzAction::SetEmitterRate { name, value }
        }
        Some("SetEmitterLifetime") => {
            let mut name = String::new();
            let mut value = 1.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "value" => value = expr_to_f32(&field.expr).unwrap_or(1.0),
                    _ => {}
                }
            }
            QuartzAction::SetEmitterLifetime { name, value }
        }
        Some("SetEmitterVelocity") => {
            let mut name = String::new();
            let mut x = 0.0f32;
            let mut y = 0.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "value" => {
                        if let Some((vx, vy)) = expr_to_f32_pair(&field.expr) {
                            x = vx;
                            y = vy;
                        }
                    }
                    _ => {}
                }
            }
            QuartzAction::SetEmitterVelocity { name, x, y }
        }
        Some("SetEmitterSpread") => {
            let mut name = String::new();
            let mut x = 0.0f32;
            let mut y = 0.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "value" => {
                        if let Some((sx, sy)) = expr_to_f32_pair(&field.expr) {
                            x = sx;
                            y = sy;
                        }
                    }
                    _ => {}
                }
            }
            QuartzAction::SetEmitterSpread { name, x, y }
        }
        Some("SetEmitterSize") => {
            let mut name = String::new();
            let mut value = 1.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "value" => value = expr_to_f32(&field.expr).unwrap_or(1.0),
                    _ => {}
                }
            }
            QuartzAction::SetEmitterSize { name, value }
        }
        Some("SetEmitterColor") => {
            let mut name = String::new();
            let mut rgba = [255u8, 255u8, 255u8, 255u8];
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "value" => {
                        if let Some(parsed) = expr_to_rgba_u8(&field.expr) {
                            rgba = parsed;
                        }
                    }
                    _ => {}
                }
            }
            QuartzAction::SetEmitterColor { name, rgba }
        }
        Some("SetEmitterGravityScale") => {
            let mut name = String::new();
            let mut value = 1.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "value" => value = expr_to_f32(&field.expr).unwrap_or(1.0),
                    _ => {}
                }
            }
            QuartzAction::SetEmitterGravityScale { name, value }
        }
        Some("SetEmitterCollision") => {
            let mut name = String::new();
            let mut mode = "None".to_owned();
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "value" => {
                        mode = parse_collision_response_expr(&field.expr).unwrap_or_else(|| "None".to_owned())
                    }
                    _ => {}
                }
            }
            QuartzAction::SetEmitterCollision { name, mode }
        }
        Some("SetEmitterRenderLayer") => {
            let mut name = String::new();
            let mut value = 0i32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "value" => value = expr_to_i32(&field.expr).unwrap_or(0),
                    _ => {}
                }
            }
            QuartzAction::SetEmitterRenderLayer { name, value }
        }
        Some("SetEmitterSizeEnd") => {
            let mut name = String::new();
            let mut value = 0.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "value" => value = expr_to_f32(&field.expr).unwrap_or(0.0),
                    _ => {}
                }
            }
            QuartzAction::SetEmitterSizeEnd { name, value }
        }
        Some("SetEmitterColorEnd") => {
            let mut name = String::new();
            let mut rgba: Option<[u8; 4]> = None;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "value" => {
                        rgba = parse_optional_rgba_u8_expr(&field.expr).unwrap_or(None);
                    }
                    _ => {}
                }
            }
            QuartzAction::SetEmitterColorEnd { name, rgba }
        }
        Some("SetEmitterShape") => {
            let mut name = String::new();
            let mut shape = "Circle".to_owned();
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "value" => {
                        shape = parse_particle_shape_expr(&field.expr).unwrap_or_else(|| "Circle".to_owned())
                    }
                    _ => {}
                }
            }
            QuartzAction::SetEmitterShape { name, shape }
        }
        Some("SetEmitterAlignToVelocity") => {
            let mut name = String::new();
            let mut enabled = false;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "value" => enabled = expr_to_bool(&field.expr).unwrap_or(false),
                    _ => {}
                }
            }
            QuartzAction::SetEmitterAlignToVelocity { name, enabled }
        }
        Some("SetEmitterInterpolatePosition") => {
            let mut name = String::new();
            let mut enabled = false;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "value" => enabled = expr_to_bool(&field.expr).unwrap_or(false),
                    _ => {}
                }
            }
            QuartzAction::SetEmitterInterpolatePosition { name, enabled }
        }
        Some("AddZoom") => {
            let mut value = 0.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                if member == "value" {
                    value = expr_to_f32(&field.expr).unwrap_or(0.0);
                }
            }
            QuartzAction::AddZoom { value }
        }
        Some("SmoothZoomAt") => {
            let mut delta = 0.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                if member == "delta" {
                    delta = expr_to_f32(&field.expr).unwrap_or(0.0);
                }
            }
            QuartzAction::SmoothZoomAt { delta }
        }
        Some("CameraFlashWith") => {
            let mut color_rgba = [255u8, 255u8, 255u8, 255u8];
            let mut duration_s = 0.1f32;
            let mut mode = "FadeOut".to_owned();
            let mut ease = "Linear".to_owned();
            let mut intensity = 1.0f32;
            let mut freeze_frame_s = 0.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "color" => {
                        if let Some(parsed) = expr_to_rgba_u8(&field.expr) {
                            color_rgba = parsed;
                        }
                    }
                    "duration" => duration_s = expr_to_f32(&field.expr).unwrap_or(0.1),
                    "mode" => {
                        mode = path_last_ident(&field.expr)
                            .or_else(|| extract_to_owned_string(&field.expr))
                            .unwrap_or_else(|| "FadeOut".to_owned());
                    }
                    "ease" => {
                        ease = path_last_ident(&field.expr)
                            .or_else(|| extract_to_owned_string(&field.expr))
                            .unwrap_or_else(|| "Linear".to_owned());
                    }
                    "intensity" => intensity = expr_to_f32(&field.expr).unwrap_or(1.0),
                    "freeze_frame" => freeze_frame_s = expr_to_f32(&field.expr).unwrap_or(0.0),
                    _ => {}
                }
            }
            QuartzAction::CameraFlashWith {
                color_rgba,
                duration_s,
                mode,
                ease,
                intensity,
                freeze_frame_s,
            }
        }
        Some("SetGlow") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut color_rgb = [255u8, 255u8, 255u8];
            let mut width = 2.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "color" => {
                        if let Some(parsed) = expr_to_rgb_u8(&field.expr) {
                            color_rgb = parsed;
                        }
                    }
                    "width" => width = expr_to_f32(&field.expr).unwrap_or(2.0),
                    _ => {}
                }
            }
            QuartzAction::SetGlow {
                target,
                color_rgb,
                width,
            }
        }
        Some("ClearGlow") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                if member == "target" {
                    if let Some(parsed) = parse_target_ref(&field.expr) {
                        target = parsed;
                    }
                }
            }
            QuartzAction::ClearGlow { target }
        }
        Some("SetTint") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut color_rgba = [255u8, 255u8, 255u8, 255u8];
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "color" => {
                        if let Some(parsed) = expr_to_rgba_u8(&field.expr) {
                            color_rgba = parsed;
                        }
                    }
                    _ => {}
                }
            }
            QuartzAction::SetTint { target, color_rgba }
        }
        Some("ClearTint") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                if member == "target" {
                    if let Some(parsed) = parse_target_ref(&field.expr) {
                        target = parsed;
                    }
                }
            }
            QuartzAction::ClearTint { target }
        }
        Some("PluginCall") => {
            let mut name = String::new();
            let mut payload = String::new();
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "payload" => payload = parse_owned_string_expr(&field.expr).unwrap_or_default(),
                    _ => {}
                }
            }
            QuartzAction::PluginCall { name, payload }
        }
        Some("RunPlugin") => {
            let mut name = String::new();
            let mut data = String::new();
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "name" => name = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "data" => data = parse_owned_string_expr(&field.expr).unwrap_or_default(),
                    _ => {}
                }
            }
            QuartzAction::RunPlugin { name, data }
        }
        Some("EnableCrystalline") => QuartzAction::EnableCrystalline,
        Some("DisableCrystalline") => QuartzAction::DisableCrystalline,
        Some("Spawn") => {
            let mut template_id = String::new();
            let mut location = QuartzLocationRef::At { x: 0.0, y: 0.0 };
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "template_id" => template_id = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "object" => template_id = parse_spawn_template_id_expr(&field.expr).unwrap_or_default(),
                    "location" => {
                        if let Some(parsed) = parse_location_ref_expr(&field.expr) {
                            location = parsed;
                        }
                    }
                    _ => {}
                }
            }
            QuartzAction::Spawn { template_id, location }
        }
        Some("SpawnObject") => {
            let mut template_id = String::new();
            let mut location = QuartzLocationRef::At { x: 0.0, y: 0.0 };
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "template_id" => template_id = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    "object" => template_id = parse_spawn_template_id_expr(&field.expr).unwrap_or_default(),
                    "location" => {
                        if let Some(parsed) = parse_location_ref_expr(&field.expr) {
                            location = parsed;
                        }
                    }
                    _ => {}
                }
            }
            QuartzAction::SpawnObject { template_id, location }
        }
        Some("SetGravityStrength") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut value = 9.8f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "value" => value = expr_to_f32(&field.expr).unwrap_or(9.8),
                    _ => {}
                }
            }
            QuartzAction::SetGravityStrength { target, value }
        }
        Some("SetPlanetRadius") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut value = 100.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "value" => value = expr_to_f32(&field.expr).unwrap_or(100.0),
                    _ => {}
                }
            }
            QuartzAction::SetPlanetRadius { target, value }
        }
        Some("SetGravityTarget") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut tag = String::new();
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "tag" => tag = extract_to_owned_string(&field.expr).unwrap_or_default(),
                    _ => {}
                }
            }
            QuartzAction::SetGravityTarget { target, tag }
        }
        Some("SetGravityInfluenceMult") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut value = 1.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "value" => value = expr_to_f32(&field.expr).unwrap_or(1.0),
                    _ => {}
                }
            }
            QuartzAction::SetGravityInfluenceMult { target, value }
        }
        Some("SetGravityFalloff") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut falloff = "Linear".to_owned();
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "falloff" => {
                        falloff = path_last_ident(&field.expr)
                            .or_else(|| extract_to_owned_string(&field.expr))
                            .unwrap_or_else(|| "Linear".to_owned());
                    }
                    _ => {}
                }
            }
            QuartzAction::SetGravityFalloff { target, falloff }
        }
        Some("SetGravityAllSources") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut enabled = false;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "enabled" => enabled = expr_to_bool(&field.expr).unwrap_or(false),
                    _ => {}
                }
            }
            QuartzAction::SetGravityAllSources { target, enabled }
        }
        Some("SetAlignToSlope") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut enabled = false;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "enabled" => enabled = expr_to_bool(&field.expr).unwrap_or(false),
                    _ => {}
                }
            }
            QuartzAction::SetAlignToSlope { target, enabled }
        }
        Some("SetAlignToSlopeSpeed") => {
            let mut target = QuartzTargetRef::Name("player".to_owned());
            let mut value = 0.0f32;
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                match member.to_string().as_str() {
                    "target" => {
                        if let Some(parsed) = parse_target_ref(&field.expr) {
                            target = parsed;
                        }
                    }
                    "value" => value = expr_to_f32(&field.expr).unwrap_or(0.0),
                    _ => {}
                }
            }
            QuartzAction::SetAlignToSlopeSpeed { target, value }
        }
        _ => QuartzAction::Expr {
            raw: expr.to_token_stream().to_string(),
        },
    }
}

fn parse_condition_expr(expr: &Expr) -> QuartzCondition {
    if let Expr::Path(path) = expr {
        return match path.path.segments.last().map(|seg| seg.ident.to_string()).as_deref() {
            Some("Always") => QuartzCondition::Always,
            Some("CrystallineEnabled") => QuartzCondition::CrystallineEnabled,
            _ => QuartzCondition::Expr {
                raw: expr.to_token_stream().to_string(),
            },
        };
    }

    let Expr::Call(ExprCall { func, args, .. }) = expr else {
        return QuartzCondition::Expr {
            raw: expr.to_token_stream().to_string(),
        };
    };

    let Some(variant) = path_last_ident(func) else {
        return QuartzCondition::Expr {
            raw: expr.to_token_stream().to_string(),
        };
    };

    match variant.as_str() {
        "expr" if !args.is_empty() => {
            if let Some(raw) = extract_to_owned_string(&args[0]) {
                QuartzCondition::Expr { raw }
            } else {
                QuartzCondition::Expr {
                    raw: expr.to_token_stream().to_string(),
                }
            }
        }
        "Always" => QuartzCondition::Always,
        "CrystallineEnabled" => QuartzCondition::CrystallineEnabled,
        "VarExists" if !args.is_empty() => {
            let variable = extract_to_owned_string(&args[0]).unwrap_or_default();
            QuartzCondition::VarExists { variable }
        }
        "Not" if !args.is_empty() => {
            parse_boxed_condition_expr(&args[0])
                .map(|inner| QuartzCondition::Not {
                    inner: Box::new(inner),
                })
                .unwrap_or_else(|| QuartzCondition::Expr {
                    raw: expr.to_token_stream().to_string(),
                })
        }
        "And" if args.len() >= 2 => {
            if let (Some(left), Some(right)) = (
                parse_boxed_condition_expr(&args[0]),
                parse_boxed_condition_expr(&args[1]),
            ) {
                QuartzCondition::And {
                    left: Box::new(left),
                    right: Box::new(right),
                }
            } else {
                QuartzCondition::Expr {
                    raw: expr.to_token_stream().to_string(),
                }
            }
        }
        "Or" if args.len() >= 2 => {
            if let (Some(left), Some(right)) = (
                parse_boxed_condition_expr(&args[0]),
                parse_boxed_condition_expr(&args[1]),
            ) {
                QuartzCondition::Or {
                    left: Box::new(left),
                    right: Box::new(right),
                }
            } else {
                QuartzCondition::Expr {
                    raw: expr.to_token_stream().to_string(),
                }
            }
        }
        "KeyHeld" if !args.is_empty() => {
            let key = parse_key_literal(&args[0]).unwrap_or_else(|| "Space".to_owned());
            QuartzCondition::KeyHeld { key }
        }
        "KeyNotHeld" if !args.is_empty() => {
            let key = parse_key_literal(&args[0]).unwrap_or_else(|| "Space".to_owned());
            QuartzCondition::KeyNotHeld { key }
        }
        "Collision" if !args.is_empty() => parse_target_ref(&args[0])
            .map(|target| QuartzCondition::Collision { target })
            .unwrap_or_else(|| QuartzCondition::Expr {
                raw: expr.to_token_stream().to_string(),
            }),
        "NoCollision" if !args.is_empty() => parse_target_ref(&args[0])
            .map(|target| QuartzCondition::NoCollision { target })
            .unwrap_or_else(|| QuartzCondition::Expr {
                raw: expr.to_token_stream().to_string(),
            }),
        "IsVisible" if !args.is_empty() => parse_target_ref(&args[0])
            .map(|target| QuartzCondition::IsVisible { target })
            .unwrap_or_else(|| QuartzCondition::Expr {
                raw: expr.to_token_stream().to_string(),
            }),
        "IsHidden" if !args.is_empty() => parse_target_ref(&args[0])
            .map(|target| QuartzCondition::IsHidden { target })
            .unwrap_or_else(|| QuartzCondition::Expr {
                raw: expr.to_token_stream().to_string(),
            }),
        "IsMoving" if !args.is_empty() => parse_target_ref(&args[0])
            .map(|target| QuartzCondition::IsMoving { target })
            .unwrap_or_else(|| QuartzCondition::Expr {
                raw: expr.to_token_stream().to_string(),
            }),
        "Grounded" if !args.is_empty() => parse_target_ref(&args[0])
            .map(|target| QuartzCondition::Grounded { target })
            .unwrap_or_else(|| QuartzCondition::Expr {
                raw: expr.to_token_stream().to_string(),
            }),
        "IsSleeping" if !args.is_empty() => parse_target_ref(&args[0])
            .map(|target| QuartzCondition::IsSleeping { target })
            .unwrap_or_else(|| QuartzCondition::Expr {
                raw: expr.to_token_stream().to_string(),
            }),
        "SpeedAbove" if args.len() >= 2 => {
            if let Some(target) = parse_target_ref(&args[0]) {
                QuartzCondition::SpeedAbove {
                    target,
                    value: expr_to_f32(&args[1]).unwrap_or(0.0),
                }
            } else {
                QuartzCondition::Expr {
                    raw: expr.to_token_stream().to_string(),
                }
            }
        }
        "SpeedBelow" if args.len() >= 2 => {
            if let Some(target) = parse_target_ref(&args[0]) {
                QuartzCondition::SpeedBelow {
                    target,
                    value: expr_to_f32(&args[1]).unwrap_or(0.0),
                }
            } else {
                QuartzCondition::Expr {
                    raw: expr.to_token_stream().to_string(),
                }
            }
        }
        "HasTag" if args.len() >= 2 => {
            if let Some(target) = parse_target_ref(&args[0]) {
                QuartzCondition::HasTag {
                    target,
                    tag: extract_to_owned_string(&args[1]).unwrap_or_default(),
                }
            } else {
                QuartzCondition::Expr {
                    raw: expr.to_token_stream().to_string(),
                }
            }
        }
        "Compare" if args.len() >= 3 => {
            let left = parse_expr_value(&args[0]);
            let op = parse_compare_op_expr(&args[1]);
            let right = parse_expr_value(&args[2]);
            QuartzCondition::Compare { left, op, right }
        }
        "IsRotating" if !args.is_empty() => parse_target_ref(&args[0])
            .map(|target| QuartzCondition::IsRotating { target })
            .unwrap_or_else(|| QuartzCondition::Expr {
                raw: expr.to_token_stream().to_string(),
            }),
        "IsStill" if !args.is_empty() => parse_target_ref(&args[0])
            .map(|target| QuartzCondition::IsStill { target })
            .unwrap_or_else(|| QuartzCondition::Expr {
                raw: expr.to_token_stream().to_string(),
            }),
        "EmitterActive" if !args.is_empty() => {
            let emitter = extract_to_owned_string(&args[0]).unwrap_or_default();
            QuartzCondition::EmitterActive { emitter }
        }
        "OnPlanet" if args.len() >= 2 => {
            if let (Some(target), Some(planet)) = (parse_target_ref(&args[0]), parse_target_ref(&args[1])) {
                QuartzCondition::OnPlanet { target, planet }
            } else {
                QuartzCondition::Expr {
                    raw: expr.to_token_stream().to_string(),
                }
            }
        }
        "InGravityField" if args.len() >= 2 => {
            if let (Some(target), Some(planet)) = (parse_target_ref(&args[0]), parse_target_ref(&args[1])) {
                QuartzCondition::InGravityField { target, planet }
            } else {
                QuartzCondition::Expr {
                    raw: expr.to_token_stream().to_string(),
                }
            }
        }
        "HasDominantPlanet" if !args.is_empty() => parse_target_ref(&args[0])
            .map(|target| QuartzCondition::HasDominantPlanet { target })
            .unwrap_or_else(|| QuartzCondition::Expr {
                raw: expr.to_token_stream().to_string(),
            }),
        "DominantPlanetIs" if args.len() >= 2 => {
            if let (Some(target), Some(planet)) = (parse_target_ref(&args[0]), parse_target_ref(&args[1])) {
                QuartzCondition::DominantPlanetIs { target, planet }
            } else {
                QuartzCondition::Expr {
                    raw: expr.to_token_stream().to_string(),
                }
            }
        }
        "InAnyGravityField" if !args.is_empty() => parse_target_ref(&args[0])
            .map(|target| QuartzCondition::InAnyGravityField { target })
            .unwrap_or_else(|| QuartzCondition::Expr {
                raw: expr.to_token_stream().to_string(),
            }),
        _ => QuartzCondition::Expr {
            raw: expr.to_token_stream().to_string(),
        },
    }
}

fn parse_boxed_condition_expr(expr: &Expr) -> Option<QuartzCondition> {
    let Expr::Call(ExprCall { func, args, .. }) = expr else {
        return None;
    };
    if path_last_ident(func).as_deref() != Some("new") {
        return None;
    }
    let Expr::Path(path) = &**func else {
        return None;
    };
    let mut segments = path.path.segments.iter();
    let is_box_new = matches!(
        (segments.next(), segments.next(), segments.next()),
        (Some(a), Some(b), None) if a.ident == "Box" && b.ident == "new"
    );
    if !is_box_new {
        return None;
    }
    let inner = args.first()?;
    Some(parse_condition_expr(inner))
}

fn parse_compare_op_expr(expr: &Expr) -> crate::core::quartz_domain::CompareOp {
    match path_last_ident(expr).as_deref() {
        Some("Eq") => crate::core::quartz_domain::CompareOp::Eq,
        Some("Ne") => crate::core::quartz_domain::CompareOp::Ne,
        Some("Lt") => crate::core::quartz_domain::CompareOp::Lt,
        Some("Le") => crate::core::quartz_domain::CompareOp::Le,
        Some("Gt") => crate::core::quartz_domain::CompareOp::Gt,
        Some("Ge") => crate::core::quartz_domain::CompareOp::Ge,
        _ => crate::core::quartz_domain::CompareOp::Eq,
    }
}

fn parse_boxed_action_expr(expr: &Expr) -> Option<QuartzAction> {
    let Expr::Call(ExprCall { func, args, .. }) = expr else {
        return None;
    };
    if path_last_ident(func).as_deref() != Some("new") {
        return None;
    }
    let Expr::Path(path) = &**func else {
        return None;
    };
    let mut segments = path.path.segments.iter();
    let is_box_new = matches!(
        (segments.next(), segments.next(), segments.next()),
        (Some(a), Some(b), None) if a.ident == "Box" && b.ident == "new"
    );
    if !is_box_new {
        return None;
    }
    let inner = args.first()?;
    Some(parse_action_expr(inner))
}

fn parse_optional_boxed_action_expr(expr: &Expr) -> Option<Box<QuartzAction>> {
    let Expr::Call(ExprCall { func, args, .. }) = expr else {
        return None;
    };
    if path_last_ident(func).as_deref() == Some("None") {
        return None;
    }
    if path_last_ident(func).as_deref() != Some("Some") {
        return parse_boxed_action_expr(expr).map(Box::new);
    }
    let inner = args.first()?;
    parse_boxed_action_expr(inner)
        .map(Box::new)
        .or_else(|| Some(Box::new(parse_action_expr(inner))))
}

fn parse_action_list_expr(expr: &Expr) -> Vec<QuartzAction> {
    match expr {
        Expr::Array(array) => array.elems.iter().map(parse_action_expr).collect(),
        Expr::Macro(mac) if mac.mac.path.segments.last().map(|seg| seg.ident == "vec") == Some(true) => {
            let parser = Punctuated::<Expr, syn::Token![,]>::parse_terminated;
            parser
                .parse2(mac.mac.tokens.clone())
                .map(|items| items.into_iter().map(|item| parse_action_expr(&item)).collect())
                .unwrap_or_default()
        }
        _ => Vec::new(),
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
        Expr::Call(call) if path_last_ident(&call.func).as_deref() == Some("f32") => {
            if let Some(arg) = call.args.first() {
                let raw = match arg {
                    Expr::Lit(ExprLit { lit: Lit::Float(v), .. }) => v.base10_digits().to_owned(),
                    Expr::Lit(ExprLit { lit: Lit::Int(v), .. }) => v.base10_digits().to_owned(),
                    _ => arg.to_token_stream().to_string(),
                };
                QuartzExpr {
                    kind: QuartzExprKind::F32,
                    raw,
                }
            } else {
                QuartzExpr::default()
            }
        }
        Expr::Call(call) if path_last_ident(&call.func).as_deref() == Some("i32") => {
            if let Some(arg) = call.args.first() {
                let raw = match arg {
                    Expr::Lit(ExprLit { lit: Lit::Int(v), .. }) => v.base10_digits().to_owned(),
                    _ => arg.to_token_stream().to_string(),
                };
                QuartzExpr {
                    kind: QuartzExprKind::I32,
                    raw,
                }
            } else {
                QuartzExpr {
                    kind: QuartzExprKind::I32,
                    raw: "0".to_owned(),
                }
            }
        }
        Expr::Call(call) if path_last_ident(&call.func).as_deref() == Some("bool") => {
            let raw = call
                .args
                .first()
                .map(|arg| arg.to_token_stream().to_string())
                .unwrap_or_else(|| "false".to_owned());
            QuartzExpr {
                kind: QuartzExprKind::Bool,
                raw,
            }
        }
        Expr::Call(call) if path_last_ident(&call.func).as_deref() == Some("str") => {
            let raw = call
                .args
                .first()
                .and_then(extract_to_owned_string)
                .unwrap_or_default();
            QuartzExpr {
                kind: QuartzExprKind::Str,
                raw,
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
        match path_last_ident(func).as_deref() {
            Some("Character") => {
                if let Some(inner) = args.first() {
                    // Key::Character("a".to_owned()) – string path
                    if let Some(s) = extract_to_owned_string(inner) {
                        return Some(s);
                    }
                    // Key::Character('a'.to_string()) – char literal path
                    if let Expr::MethodCall(call) = inner {
                        if let Expr::Lit(ExprLit { lit: Lit::Char(ch), .. }) = &*call.receiver {
                            return Some(ch.value().to_string());
                        }
                    }
                    // Key::Character('a') – bare char literal
                    if let Expr::Lit(ExprLit { lit: Lit::Char(ch), .. }) = inner {
                        return Some(ch.value().to_string());
                    }
                }
                return None;
            }
            Some("Named") => {
                if let Some(named_expr) = args.first() {
                    return path_last_ident(named_expr)
                        .or_else(|| extract_to_owned_string(named_expr));
                }
            }
            _ => {}
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
        if call.method == "to_owned" || call.method == "into" {
            return extract_string_literal(&call.receiver);
        }
    }
    if let Expr::Call(ExprCall { func, args, .. }) = expr {
        if path_last_ident(func).as_deref() == Some("from") {
            return args.first().and_then(extract_string_literal);
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
        Expr::Path(path) => {
            let ident = path
                .path
                .segments
                .last()
                .map(|seg| seg.ident.to_string())
                .ok_or_else(|| anyhow::anyhow!("unsupported numeric path expression"))?;

            IMPORT_NUMERIC_CONSTS.with(|slot| {
                slot.borrow()
                    .get(&ident)
                    .copied()
                    .ok_or_else(|| anyhow::anyhow!("unknown numeric constant: {ident}"))
            })
        }
        Expr::Unary(unary) if unary.op.to_token_stream().to_string() == "-" => Ok(-expr_to_f32(&unary.expr)?),
        Expr::Paren(paren) => expr_to_f32(&paren.expr),
        _ => anyhow::bail!("unsupported numeric expression: {}", expr.to_token_stream()),
    }
}

fn expr_to_f32_with_consts(
    expr: &Expr,
    consts: &std::collections::BTreeMap<String, f32>,
) -> Result<f32> {
    match expr {
        Expr::Path(path) => {
            let last = path
                .path
                .segments
                .last()
                .map(|seg| seg.ident.to_string())
                .ok_or_else(|| anyhow::anyhow!("unsupported numeric path expression"))?;
            if let Some(value) = consts.get(&last) {
                Ok(*value)
            } else {
                expr_to_f32(expr)
            }
        }
        Expr::Binary(binary) => {
            use syn::BinOp;
            let left = expr_to_f32_with_consts(&binary.left, consts)?;
            let right = expr_to_f32_with_consts(&binary.right, consts)?;
            match binary.op {
                BinOp::Add(_) => Ok(left + right),
                BinOp::Sub(_) => Ok(left - right),
                BinOp::Mul(_) => Ok(left * right),
                BinOp::Div(_) => Ok(left / right),
                _ => anyhow::bail!("unsupported numeric expression: {}", expr.to_token_stream()),
            }
        }
        Expr::Unary(unary) if unary.op.to_token_stream().to_string() == "-" => {
            Ok(-expr_to_f32_with_consts(&unary.expr, consts)?)
        }
        Expr::Paren(paren) => expr_to_f32_with_consts(&paren.expr, consts),
        _ => expr_to_f32(expr),
    }
}

fn expr_to_i32(expr: &Expr) -> Result<i32> {
    Ok(expr_to_f32(expr)? as i32)
}

fn expr_to_f32_pair(expr: &Expr) -> Option<(f32, f32)> {
    match expr {
        Expr::Tuple(tuple) if tuple.elems.len() >= 2 => {
            let x = expr_to_f32(&tuple.elems[0]).ok()?;
            let y = expr_to_f32(&tuple.elems[1]).ok()?;
            Some((x, y))
        }
        Expr::Paren(paren) => expr_to_f32_pair(&paren.expr),
        _ => None,
    }
}

fn expr_to_rgba_u8(expr: &Expr) -> Option<[u8; 4]> {
    match expr {
        Expr::Tuple(tuple) if tuple.elems.len() >= 4 => Some([
            expr_to_f32(&tuple.elems[0]).ok()? as u8,
            expr_to_f32(&tuple.elems[1]).ok()? as u8,
            expr_to_f32(&tuple.elems[2]).ok()? as u8,
            expr_to_f32(&tuple.elems[3]).ok()? as u8,
        ]),
        Expr::Call(ExprCall { func, args, .. }) if path_last_ident(func).as_deref() == Some("Color") && args.len() >= 4 => {
            Some([
                expr_to_f32(&args[0]).ok()? as u8,
                expr_to_f32(&args[1]).ok()? as u8,
                expr_to_f32(&args[2]).ok()? as u8,
                expr_to_f32(&args[3]).ok()? as u8,
            ])
        }
        Expr::Paren(paren) => expr_to_rgba_u8(&paren.expr),
        _ => None,
    }
}

fn expr_to_rgb_u8(expr: &Expr) -> Option<[u8; 3]> {
    match expr {
        Expr::Tuple(tuple) if tuple.elems.len() >= 3 => Some([
            expr_to_f32(&tuple.elems[0]).ok()? as u8,
            expr_to_f32(&tuple.elems[1]).ok()? as u8,
            expr_to_f32(&tuple.elems[2]).ok()? as u8,
        ]),
        Expr::Call(ExprCall { func, args, .. }) if path_last_ident(func).as_deref() == Some("from_rgb") && args.len() >= 3 => {
            Some([
                expr_to_f32(&args[0]).ok()? as u8,
                expr_to_f32(&args[1]).ok()? as u8,
                expr_to_f32(&args[2]).ok()? as u8,
            ])
        }
        Expr::Paren(paren) => expr_to_rgb_u8(&paren.expr),
        _ => None,
    }
}

fn parse_optional_rgba_u8_expr(expr: &Expr) -> Option<Option<[u8; 4]>> {
    if path_last_ident(expr).as_deref() == Some("None") {
        return Some(None);
    }
    if let Expr::Call(ExprCall { func, args, .. }) = expr {
        if path_last_ident(func).as_deref() == Some("Some") {
            return args.first().and_then(expr_to_rgba_u8).map(Some);
        }
    }
    expr_to_rgba_u8(expr).map(Some)
}

fn parse_location_ref_expr(expr: &Expr) -> Option<QuartzLocationRef> {
    match expr {
        Expr::Call(ExprCall { func, args, .. }) => {
            match path_last_ident(func)?.as_str() {
                "at" if args.len() >= 2 => Some(QuartzLocationRef::At {
                    x: expr_to_f32(&args[0]).ok()?,
                    y: expr_to_f32(&args[1]).ok()?,
                }),
                "at_target" if !args.is_empty() => parse_target_ref(&args[0]).map(QuartzLocationRef::AtTarget),
                _ => None,
            }
        }
        Expr::Paren(paren) => parse_location_ref_expr(&paren.expr),
        _ => None,
    }
}

fn parse_optional_location_ref_expr(expr: &Expr) -> Option<Option<QuartzLocationRef>> {
    if path_last_ident(expr).as_deref() == Some("None") {
        return Some(None);
    }
    if let Expr::Call(ExprCall { func, args, .. }) = expr {
        if path_last_ident(func).as_deref() == Some("Some") {
            return args.first().and_then(parse_location_ref_expr).map(Some);
        }
    }
    parse_location_ref_expr(expr).map(Some)
}

fn parse_owned_string_expr(expr: &Expr) -> Option<String> {
    if let Expr::Call(ExprCall { func, args, .. }) = expr {
        if path_last_ident(func).as_deref() == Some("new") {
            return args.first().and_then(extract_to_owned_string);
        }
    }
    extract_to_owned_string(expr)
}

fn parse_spawn_template_id_expr(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Call(ExprCall { func, args, .. }) => {
            if path_last_ident(func).as_deref() == Some("new") {
                let Expr::Path(ExprPath { path, .. }) = &**func else { return None; };
                let mut segments = path.segments.iter();
                let is_box_new = matches!(
                    (segments.next(), segments.next(), segments.next()),
                    (Some(first), Some(second), None) if first.ident == "Box" && second.ident == "new"
                );
                if is_box_new {
                    return args.first().and_then(parse_spawn_template_id_expr);
                }
            }
            let ident = path_last_ident(func)?;
            ident.strip_prefix("spawn_").map(|value| value.to_owned())
        }
        Expr::Paren(paren) => parse_spawn_template_id_expr(&paren.expr),
        Expr::Path(_) => path_last_ident(expr).and_then(|ident| ident.strip_prefix("spawn_").map(|value| value.to_owned())),
        _ => None,
    }
}

fn parse_collision_response_expr(expr: &Expr) -> Option<String> {
    if let Expr::Struct(ExprStruct { path, .. }) = expr {
        return path.segments.last().map(|seg| seg.ident.to_string());
    }
    path_last_ident(expr)
}

fn parse_particle_shape_expr(expr: &Expr) -> Option<String> {
    if let Expr::Struct(ExprStruct { path, .. }) = expr {
        return path.segments.last().map(|seg| seg.ident.to_string());
    }
    path_last_ident(expr)
}

fn parse_emitter_name_expr(expr: &Expr) -> Option<String> {
    match expr {
        Expr::MethodCall(call) => {
            if call.method == "new" {
                return call.args.first().and_then(extract_to_owned_string);
            }
            if call.method == "named" {
                return call.args.first().and_then(extract_to_owned_string);
            }
            parse_emitter_name_expr(&call.receiver)
        }
        Expr::Call(ExprCall { func, args, .. }) => {
            if let Some(name) = path_last_ident(func) {
                if name == "new" {
                    return args.first().and_then(extract_to_owned_string);
                }
                return Some(name);
            }
            None
        }
        Expr::Struct(ExprStruct { fields, .. }) => {
            for field in fields {
                let Member::Named(member) = &field.member else { continue; };
                if member == "name" {
                    if let Some(name) = extract_to_owned_string(&field.expr) {
                        return Some(name);
                    }
                }
            }
            None
        }
        Expr::Paren(paren) => parse_emitter_name_expr(&paren.expr),
        _ => None,
    }
}

fn expr_to_u32(expr: &Expr) -> Result<u32> {
    match expr {
        Expr::Lit(ExprLit { lit: Lit::Int(value), .. }) => value
            .base10_parse::<u32>()
            .map_err(|err| anyhow::anyhow!("invalid u32 literal {}: {err}", value.base10_digits())),
        Expr::Paren(paren) => expr_to_u32(&paren.expr),
        Expr::Path(path) => {
            let last = path
                .path
                .segments
                .last()
                .map(|seg| seg.ident.to_string())
                .ok_or_else(|| anyhow::anyhow!("unsupported u32 path expression"))?;
            match last.as_str() {
                "NONE" => Ok(0),
                "DEFAULT" => Ok(1 << 0),
                "PLAYER" => Ok(1 << 1),
                "ENEMY" => Ok(1 << 2),
                "PROJECTILE" => Ok(1 << 3),
                "PICKUP" => Ok(1 << 4),
                "TRIGGER" => Ok(1 << 5),
                "TERRAIN" => Ok(1 << 6),
                "PARTICLE" => Ok(1 << 7),
                "ALL" => Ok(u32::MAX),
                _ => Err(anyhow::anyhow!(
                    "unsupported u32 path expression: {}",
                    expr.to_token_stream()
                )),
            }
        }
        Expr::Binary(binary) => {
            use syn::BinOp;
            let left = expr_to_u32(&binary.left)?;
            let right = expr_to_u32(&binary.right)?;
            match binary.op {
                BinOp::BitOr(_) => Ok(left | right),
                BinOp::BitAnd(_) => Ok(left & right),
                BinOp::BitXor(_) => Ok(left ^ right),
                BinOp::Shl(_) => Ok(left << right),
                BinOp::Shr(_) => Ok(left >> right),
                _ => Err(anyhow::anyhow!(
                    "unsupported u32 binary operation: {}",
                    expr.to_token_stream()
                )),
            }
        }
        _ => Ok(expr_to_f32(expr)? as u32),
    }
}

fn expr_to_bool(expr: &Expr) -> Result<bool> {
    match expr {
        Expr::Lit(ExprLit { lit: Lit::Bool(value), .. }) => Ok(value.value),
        Expr::Paren(paren) => expr_to_bool(&paren.expr),
        _ => anyhow::bail!("unsupported bool expression: {}", expr.to_token_stream()),
    }
}

fn expr_to_material(expr: &Expr) -> Result<ObjectPhysicsMaterialSpec> {
    if let Expr::Call(ExprCall { func, args, .. }) = expr {
        if path_last_ident(func).as_deref() == Some("new") && args.len() >= 3 {
            return Ok(ObjectPhysicsMaterialSpec {
                preset: ObjectPhysicsMaterialPreset::Custom,
                elasticity: expr_to_f32(&args[0])?,
                friction: expr_to_f32(&args[1])?,
                density: expr_to_f32(&args[2])?,
            });
        }
    }

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

fn collect_u32_constants_from_ast(ast: &File) -> std::collections::BTreeMap<String, u32> {
    let mut out = std::collections::BTreeMap::<String, u32>::new();

    // Multi-pass so dependent constants (e.g. A = B | C) can resolve after B/C are known.
    for _ in 0..6 {
        let mut changed = false;
        for item in &ast.items {
            let Item::Const(item_const) = item else { continue; };
            let name = item_const.ident.to_string();
            if out.contains_key(&name) {
                continue;
            }
            if let Ok(value) = expr_to_u32_with_consts(&item_const.expr, &out) {
                out.insert(name, value);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    out
}

fn expr_to_u32_with_consts(
    expr: &Expr,
    consts: &std::collections::BTreeMap<String, u32>,
) -> Result<u32> {
    match expr {
        Expr::Path(path) => {
            let last = path
                .path
                .segments
                .last()
                .map(|seg| seg.ident.to_string())
                .ok_or_else(|| anyhow::anyhow!("unsupported u32 path expression"))?;
            if let Some(value) = consts.get(&last) {
                return Ok(*value);
            }
            expr_to_u32(expr)
        }
        Expr::Binary(binary) => {
            use syn::BinOp;
            let left = expr_to_u32_with_consts(&binary.left, consts)?;
            let right = expr_to_u32_with_consts(&binary.right, consts)?;
            match binary.op {
                BinOp::BitOr(_) => Ok(left | right),
                BinOp::BitAnd(_) => Ok(left & right),
                BinOp::BitXor(_) => Ok(left ^ right),
                BinOp::Shl(_) => Ok(left << right),
                BinOp::Shr(_) => Ok(left >> right),
                _ => expr_to_u32(expr),
            }
        }
        Expr::Paren(paren) => expr_to_u32_with_consts(&paren.expr, consts),
        _ => expr_to_u32(expr),
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
    use super::require_import_ticket_negative_coverage;
    use crate::core::project::EditorProjectState;
    use crate::core::quartz_domain::{
        CustomCodeKind, LogicNode, QuartzAction, QuartzCondition, QuartzLocationRef,
    };
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
        assert_eq!(report.imported_object_count, 3);
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
        assert_eq!(report.imported_object_count, 3);
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
        assert!(state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .filter(|b| b.kind == CustomCodeKind::UpdateLoops)
            .collect::<Vec<_>>()
            .is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_reads_handwritten_register_logic_into_update_loops() {
        let root = temp_root("register_logic_raw_update_loops");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn register_logic(canvas: &mut Canvas) {
    canvas.on_update(|canvas| {
        let dt = 1.0 / 60.0;
        if canvas.has_var("game_over") {
            canvas.set_var("cooldown", dt);
        }
    });
    canvas.on_update(|canvas| {
        refresh_hud(canvas);
    });
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_logic_tree_count, 0);
        assert!(state.manifest.scenes[0].logic_trees.is_empty());

        let update_loops = state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .filter(|b| b.kind == CustomCodeKind::UpdateLoops)
            .collect::<Vec<_>>();
        assert_eq!(update_loops.len(), 2);
        assert!(update_loops.iter().any(|b| b.code.contains("let dt = 1.0 / 60.0")));
        assert!(update_loops.iter().any(|b| b.code.contains("refresh_hud")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_replaces_stale_logic_trees_with_update_loops() {
        let root = temp_root("replace_stale_logic_trees_with_update_loops");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";

        let mut stale_tree = crate::core::quartz_domain::LogicTree::new(
            "logic_stale".to_owned(),
            "stale".to_owned(),
        );
        stale_tree.output_file = scene_file.to_owned();
        stale_tree.nodes.push(LogicNode::Action(QuartzAction::Expr {
            raw: "canvas.run(Action::Custom { name: \"stale\".to_owned() })".to_owned(),
        }));
        state.manifest.scenes[0].logic_trees.push(stale_tree);

        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn register_logic(canvas: &mut Canvas) {
    canvas.on_update(|canvas| {
        let dt = 1.0 / 60.0;
        canvas.set_var("cooldown", dt);
    });
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_logic_tree_count, 0);
        assert!(state.manifest.scenes[0].logic_trees.is_empty());
        assert_eq!(
            state.manifest.scenes[0]
                .custom_code_blocks
                .iter()
                .filter(|b| b.kind == CustomCodeKind::UpdateLoops && b.output_file == scene_file)
                .count(),
            1
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_real_asteroid_rush_scene_does_not_fallback_to_manual_override() {
        let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .to_path_buf();
        let root = workspace_root.join("asteroid_rush");
        let scene_file = "src/scenes/main_scene.rs";

        assert!(root.join(scene_file).is_file(), "expected asteroid_rush scene file to exist");

        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = scene_file.to_owned();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], false)
            .expect("real asteroid_rush scene should import without fallback");

        assert!(report.fallback_manual_override_files.is_empty());
        assert!(state.manifest.scenes[0].custom_code_blocks.iter().all(|b| {
            !(b.kind == CustomCodeKind::ManualFileOverride && b.output_file == scene_file)
        }));
        assert!(
            !state.manifest.scenes[0].objects.is_empty()
                || !state.manifest.scenes[0].events.is_empty()
                || !state.manifest.scenes[0].logic_trees.is_empty()
                || state.manifest.scenes[0]
                    .custom_code_blocks
                    .iter()
                    .any(|b| b.kind != CustomCodeKind::ManualFileOverride),
            "expected semantic import to produce structured data"
        );
    }

    #[test]
    fn semantic_import_ignores_non_canvas_on_update_calls() {
        let root = temp_root("register_logic_non_canvas_on_update");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn register_logic(canvas: &mut Canvas, scheduler: &mut Scheduler) {
    scheduler.on_update(|canvas| {
        canvas.set_var("ignored", Value::Bool(true));
    });
    canvas.on_update(|canvas| {
        canvas.set_var("accepted", Value::Bool(true));
    });
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_logic_tree_count, 0);

        let update_loops = state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .filter(|b| b.kind == CustomCodeKind::UpdateLoops)
            .collect::<Vec<_>>();
        assert_eq!(update_loops.len(), 1);
        assert!(update_loops[0].code.contains("accepted"));
        assert!(!update_loops[0].code.contains("ignored"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_routes_unclassified_functions_to_top_level() {
        let root = temp_root("import_top_level_fallback");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn helper_damage(seed: i32) -> i32 {
    seed * 2
}

pub fn helper_spawn_name() -> &'static str {
    "orb"
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_custom_block_count, 1);

        let top_level = state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .find(|b| b.kind == CustomCodeKind::TopLevel)
            .expect("expected top-level fallback block");
        assert!(top_level.code.contains("pub fn helper_damage"));
        assert!(top_level.code.contains("pub fn helper_spawn_name"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_preserves_builder_helpers_as_top_level_code() {
        let root = temp_root("import_builder_helper_top_level");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn fire_bullet(canvas: &mut Canvas) {
    let mut bullet = GameObject::build("bullet")
        .size(14.0, 14.0)
        .position(0.0, 0.0)
        .finish();
    canvas.add_game_object("bullet".to_owned(), bullet);
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_object_count, 0);

        let top_level = state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .find(|b| b.kind == CustomCodeKind::TopLevel)
            .expect("expected top-level fallback block");
        assert!(top_level.code.contains("pub fn fire_bullet"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_preserves_spawn_prefix_helpers_without_gameobject_return() {
        let root = temp_root("import_spawn_prefix_helper_top_level");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn spawn_asteroid(canvas: &mut Canvas) {
    let mut asteroid = GameObject::build("asteroid")
        .size(8.0, 8.0)
        .position(0.0, 0.0)
        .finish();
    canvas.add_game_object("asteroid".to_owned(), asteroid);
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_object_count, 0);

        let top_level = state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .find(|b| b.kind == CustomCodeKind::TopLevel)
            .expect("expected top-level fallback block");
        assert!(top_level.code.contains("pub fn spawn_asteroid"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_reads_character_keys_in_key_events() {
        let root = temp_root("register_events_character_keys");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn register_events(canvas: &mut Canvas) {
    canvas.add_event(
        GameEvent::KeyHold {
            key: Key::Character('a'.to_string()),
            action: Action::Custom { name: "left".into() },
            target: Target::name("player"),
            modifiers: None,
        },
        Target::name("player"),
    );
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_event_count, 1);
        let event = &state.manifest.scenes[0].events[0];
        match &event.kind {
            crate::core::quartz_domain::QuartzEventKind::KeyHold { key, .. } => {
                assert_eq!(key, "a")
            }
            other => panic!("expected KeyHold, got {:?}", other),
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_scaffold_lib_rs_does_not_become_manual_override() {
        let root = temp_root("scaffold_no_manual_override");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let lib_file = "src/lib.rs";

        std::fs::write(
            root.join(lib_file),
            r#"use quartz::*;
pub struct App;
ramp::run! { []; |ctx: &mut Context| { Canvas::new(ctx, CanvasMode::Landscape) } }
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[lib_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_custom_block_count, 0);
        // lib.rs must NOT be tracked as a ManualFileOverride
        assert!(state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .all(|b| b.output_file != lib_file));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_purges_stale_scaffold_manual_override_blocks() {
        let root = temp_root("scaffold_purge_stale");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let lib_file = "src/lib.rs";

        // Pre-populate a stale ManualFileOverride block as if it was imported before the fix.
        let initial_len = state.manifest.scenes[0].custom_code_blocks.len();
        state.manifest.scenes[0].custom_code_blocks.push(
            crate::core::quartz_domain::CustomCodeBlock::new(
                "stale_manual".to_owned(),
                "stale_manual".to_owned(),
                CustomCodeKind::ManualFileOverride,
                lib_file.to_owned(),
            ),
        );
        assert_eq!(
            state.manifest.scenes[0].custom_code_blocks.len(),
            initial_len + 1,
            "stale block should have been added"
        );

        std::fs::write(
            root.join(lib_file),
            "use quartz::*;\nramp::run! { []; |ctx: &mut Context| { Canvas::new(ctx, CanvasMode::Landscape) } }\n",
        )
        .unwrap();

        import_files_into_state(&mut state, &root, &[lib_file.to_owned()], true).unwrap();

        // Stale block must be purged.
        assert!(state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .all(|b| b.output_file != lib_file));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_reads_named_keys_in_key_events() {
        let root = temp_root("register_events_named_keys");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn register_events(canvas: &mut Canvas) {
    canvas.add_event(
        GameEvent::KeyHold {
            key: Key::Named(NamedKey::ArrowLeft),
            action: Action::Custom { name: "left".into() },
            target: Target::name("player"),
            modifiers: None,
        },
        Target::name("player"),
    );
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_event_count, 1);
        let event = &state.manifest.scenes[0].events[0];
        match &event.kind {
            crate::core::quartz_domain::QuartzEventKind::KeyHold { key, .. } => {
                assert_eq!(key, "ArrowLeft")
            }
            other => panic!("expected KeyHold, got {:?}", other),
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_reads_canvas_on_update_from_non_register_functions() {
        let root = temp_root("on_update_non_register_function");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn wire_extra_updates(canvas: &mut Canvas) {
    canvas.on_update(|canvas| {
        canvas.set_var("ticks", 1i32);
    });
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_custom_block_count, 1);

        let update_loops = state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .filter(|b| b.kind == CustomCodeKind::UpdateLoops)
            .collect::<Vec<_>>();
        assert_eq!(update_loops.len(), 1);
        assert!(update_loops[0].code.contains("set_var"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_skips_scaffold_lib_rs_top_level_fallback() {
        let root = temp_root("skip_scaffold_lib_rs");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let lib_file = "src/lib.rs";

        std::fs::write(
            root.join(lib_file),
            r#"use quartz::*;
use ramp::prism;
use ramp::Drawable;

#[path = "scenes/main_scene.rs"]
mod generated_scene;

pub struct App;

impl App {
    fn new(ctx: &mut Context) -> impl Drawable {
        let mut canvas = Canvas::new(ctx, CanvasMode::Landscape);
        generated_scene::setup_scene(&mut canvas);
        generated_scene::register_logic(&mut canvas);
        generated_scene::register_events(&mut canvas);
        canvas
    }
}

ramp::run! { []; |ctx: &mut Context| { App::new(ctx) } }
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[lib_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_custom_block_count, 0);
        assert!(state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .all(|b| !(b.output_file == lib_file && b.id == "top_level_imported")));

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

    #[test]
    fn semantic_import_preserves_setup_runtime_and_crystalline_action_enable() {
        let root = temp_root("setup_runtime_crystalline");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        let scene_file = "src/scenes/main_scene.rs";
        state.manifest.scenes[0].source_file = scene_file.to_owned();

        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn setup_scene(canvas: &mut Canvas) {
    canvas.run(Action::EnableCrystalline);

    let mut bg = GameObject::build("bg")
        .size(3840.0, 2160.0)
        .position(0.0, 0.0)
        .layer(-10)
        .screen_space()
        .finish();
    bg.set_drawable(Box::new(quartz::sprite::tint_overlay(3840.0, 2160.0, Color(8, 12, 24, 255))));
    canvas.add_game_object("bg".to_owned(), bg);

    let mut player = GameObject::build("player")
        .size(48.0, 48.0)
        .position(10.0, 20.0)
        .layer(2)
        .solid_circle(24.0)
        .collision_layer(1)
        .collision_mask(1)
        .tag("player")
        .finish();
    player.set_drawable(Box::new(quartz::sprite::solid_circle(48.0, Color(84, 210, 255, 255))));
    canvas.add_game_object("player".to_owned(), player);

    let mut overlay = GameObject::build("game_over_overlay")
        .size(1300.0, 120.0)
        .position(1270.0, 1020.0)
        .layer(30)
        .screen_space()
        .finish();
    overlay.visible = false;
    overlay.set_drawable(Box::new(canvas.make_text("GAME OVER  -  PRESS R TO RESTART".into(), 54.0, Color(255, 80, 80, 255), Align::Center, Arc::new(Font::from_bytes(include_bytes!("../../assets/font.ttf")).unwrap()))));
    canvas.add_game_object("game_over_overlay".to_owned(), overlay);

    let mut camera = Camera::new((3840.0, 2160.0), (3840.0, 2160.0));
    camera.follow(Some(Target::name("player")));
    canvas.set_camera(camera);
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_object_count, 3);
        assert!(state.manifest.scenes[0].canvas.crystalline_enabled);
        let player = state.manifest.scenes[0]
            .objects
            .iter()
            .find(|obj| obj.id == "player")
            .expect("player object should import");
        assert_eq!(player.color_rgb, [84, 210, 255]);
        assert!(state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .any(|b| b.kind == CustomCodeKind::TypedVars
                && b.id == "setup_runtime"
                && b.code.contains("set_camera")
                && b.code.contains("game_over_overlay . set_drawable")
                && b.code.contains("game_over_overlay . visible = false")
                && b.code.contains("bg . set_drawable")
                && b.code.contains("player . set_drawable")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_preserves_no_collision_as_zero_masks() {
        let root = temp_root("no_collision_zero_masks");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        let scene_file = "src/scenes/main_scene.rs";
        state.manifest.scenes[0].source_file = scene_file.to_owned();

        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn setup_scene(canvas: &mut Canvas) {
    let mut hud = GameObject::build("hud")
        .size(100.0, 20.0)
        .position(0.0, 0.0)
        .screen_space()
        .no_collision()
        .finish();
    canvas.add_game_object("hud".to_owned(), hud);
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_object_count, 1);
        let hud = state.manifest.scenes[0]
            .objects
            .iter()
            .find(|obj| obj.id == "hud")
            .expect("hud object should import");
        assert_eq!(hud.advanced.collision_layer, 0);
        assert_eq!(hud.advanced.collision_mask, 0);
        assert_eq!(hud.advanced.collision_mode, crate::core::quartz_domain::QuartzObjectCollisionMode::NonPlatform);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scene_import_does_not_short_circuit_to_raw_custom_when_output_file_matches() {
        let root = temp_root("scene_not_raw_custom_short_circuit");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        let scene_file = "src/scenes/main_scene.rs";
        state.manifest.scenes[0].source_file = scene_file.to_owned();

        for block in &mut state.manifest.scenes[0].custom_code_blocks {
            if matches!(block.kind, CustomCodeKind::Constants | CustomCodeKind::GameStateVars) {
                block.output_file = scene_file.to_owned();
                block.code = "stale".to_owned();
            }
        }

        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

const PLAYER_SPEED: f32 = 240.0;

pub fn setup_scene(canvas: &mut Canvas) {
    let player = GameObject::build("player")
        .size(48.0, 48.0)
        .position(10.0, 20.0)
        .layer(2)
        .build(canvas);
    canvas.add_game_object("player".to_owned(), player);
    canvas.set_var("score", Value::I32(0));
}

pub fn register_logic(canvas: &mut Canvas) {
    canvas.on_update(|canvas| {
        canvas.run(Action::Custom { name: "tick".to_owned() });
    });
}

pub fn register_events(canvas: &mut Canvas) {
    canvas.add_event(
        GameEvent::Collision { action: Action::Custom { name: "hit".to_owned() }, target: Target::tag("enemy") },
        Target::name("player"),
    );
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_object_count, 1);
        assert_eq!(report.imported_logic_tree_count, 1);
        assert_eq!(report.imported_event_count, 1);
        assert!(report.fallback_manual_override_files.is_empty());

        let constants_blocks = state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .filter(|b| b.kind == CustomCodeKind::Constants && b.output_file == scene_file)
            .collect::<Vec<_>>();
        assert!(!constants_blocks.is_empty(), "constants block should exist");
        assert!(
            constants_blocks
                .iter()
                .any(|block| block.code.contains("const PLAYER_SPEED")),
            "at least one scene constants block should contain imported const definitions"
        );
        assert!(
            constants_blocks
                .iter()
                .all(|block| !block.code.contains("pub fn setup_scene")),
            "scene constants blocks should not be replaced with full file contents"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scene_import_with_comments_still_extracts_structured_content() {
        let root = temp_root("scene_comments_structured");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        let scene_file = "src/scenes/main_scene.rs";
        state.manifest.scenes[0].source_file = scene_file.to_owned();

        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

// harmless helper note with marker-like text: custom code block: constants
const PLAYER_SPEED: f32 = 240.0;

pub fn setup_scene(canvas: &mut Canvas) {
    // game var block: should not alter semantic import routing
    let player = GameObject::build("player")
        // object builder comments should be ignored
        .size(48.0, 48.0)
        .position(10.0, 20.0)
        .layer(2)
        .build(canvas);
    canvas.add_game_object("player".to_owned(), player);
    canvas.set_var("score", Value::I32(0));
}

pub fn register_logic(canvas: &mut Canvas) {
    canvas.on_update(|canvas| {
        // update comment
        canvas.run(Action::Custom { name: "tick".to_owned() });
    });
}

pub fn register_events(canvas: &mut Canvas) {
    canvas.add_event(
        GameEvent::Collision { action: Action::Custom { name: "hit".to_owned() }, target: Target::tag("enemy") },
        Target::name("player"),
    );
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_object_count, 1);
        assert_eq!(report.imported_logic_tree_count, 1);
        assert_eq!(report.imported_event_count, 1);
        assert!(report.fallback_manual_override_files.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ticket_completion_fails_when_negative_test_missing() {
        let result = require_import_ticket_negative_coverage(false);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("missing coverage: negative_tests"));
    }

    #[test]
    fn semantic_import_reads_new_condition_variants() {
        let root = temp_root("new_condition_variants");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn register_logic(canvas: &mut Canvas) {
    canvas.on_update(|canvas| {
        canvas.run(Action::Conditional {
            condition: Condition::IsRotating(Target::name("player")),
            if_true: Box::new(Action::Custom { name: "rotating".to_owned() }),
            if_false: None,
        });
    });

    canvas.on_update(|canvas| {
        canvas.run(Action::Conditional {
            condition: Condition::InGravityField(Target::name("player"), Target::tag("planet")),
            if_true: Box::new(Action::Custom { name: "inside_gravity".to_owned() }),
            if_false: Some(Box::new(Action::Custom { name: "outside_gravity".to_owned() })),
        });
    });
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_logic_tree_count, 2);

        let trees = &state.manifest.scenes[0].logic_trees;
        let mut saw_is_rotating = false;
        let mut saw_in_gravity_field = false;
        for tree in trees {
            for node in &tree.nodes {
                if let LogicNode::Action(QuartzAction::Conditional { condition, .. }) = node {
                    match condition {
                        QuartzCondition::IsRotating { .. } => saw_is_rotating = true,
                        QuartzCondition::InGravityField { .. } => saw_in_gravity_field = true,
                        _ => {}
                    }
                }
            }
        }

        assert!(saw_is_rotating);
        assert!(saw_in_gravity_field);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_reads_physics_material_action_variants() {
        let root = temp_root("physics_material_variants");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn register_logic(canvas: &mut Canvas) {
    canvas.on_update(|canvas| {
        canvas.run(Action::ApplyForce { target: Target::name("player"), fx: 1.0, fy: -2.0 });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::ApplyImpulse { target: Target::tag("enemy"), ix: 3.0, iy: -4.0 });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetMaterial { target: Target::name("player"), material: PhysicsMaterial::new(0.8, 0.4, 1.2) });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetDensity { target: Target::name("player"), value: 1.25 });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetElasticity { target: Target::name("player"), value: 0.75 });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetFriction { target: Target::name("player"), value: 0.35 });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::FreezeBody { target: Target::name("player") });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::UnfreezeBody { target: Target::name("player") });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::WakeBody { target: Target::name("player") });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetPhysicsQuality { quality: PhysicsQuality::High });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetCollisionMode { target: Target::name("player"), mode: CollisionMode::Sensor });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetSlope { target: Target::name("player"), left_offset: -2.0, right_offset: 2.0, auto_rotate: true });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetSurfaceNormal { target: Target::name("player"), nx: 0.0, ny: -1.0 });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::TransferMomentum { from: Target::name("player"), to: Target::name("crate"), scale: 1.5 });
    });
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_logic_tree_count, 14);

        let trees = &state.manifest.scenes[0].logic_trees;
        let mut saw_apply_force = false;
        let mut saw_apply_impulse = false;
        let mut saw_set_material = false;
        let mut saw_set_density = false;
        let mut saw_set_elasticity = false;
        let mut saw_set_friction = false;
        let mut saw_freeze = false;
        let mut saw_unfreeze = false;
        let mut saw_wake = false;
        let mut saw_quality = false;
        let mut saw_mode = false;
        let mut saw_slope = false;
        let mut saw_surface_normal = false;
        let mut saw_transfer = false;

        for tree in trees {
            for node in &tree.nodes {
                if let LogicNode::Action(action) = node {
                    match action {
                        QuartzAction::ApplyForce { .. } => saw_apply_force = true,
                        QuartzAction::ApplyImpulse { .. } => saw_apply_impulse = true,
                        QuartzAction::SetMaterial { .. } => saw_set_material = true,
                        QuartzAction::SetDensity { .. } => saw_set_density = true,
                        QuartzAction::SetElasticity { .. } => saw_set_elasticity = true,
                        QuartzAction::SetFriction { .. } => saw_set_friction = true,
                        QuartzAction::FreezeBody { .. } => saw_freeze = true,
                        QuartzAction::UnfreezeBody { .. } => saw_unfreeze = true,
                        QuartzAction::WakeBody { .. } => saw_wake = true,
                        QuartzAction::SetPhysicsQuality { .. } => saw_quality = true,
                        QuartzAction::SetCollisionMode { .. } => saw_mode = true,
                        QuartzAction::SetSlope { .. } => saw_slope = true,
                        QuartzAction::SetSurfaceNormal { .. } => saw_surface_normal = true,
                        QuartzAction::TransferMomentum { .. } => saw_transfer = true,
                        _ => {}
                    }
                }
            }
        }

        assert!(saw_apply_force);
        assert!(saw_apply_impulse);
        assert!(saw_set_material);
        assert!(saw_set_density);
        assert!(saw_set_elasticity);
        assert!(saw_set_friction);
        assert!(saw_freeze);
        assert!(saw_unfreeze);
        assert!(saw_wake);
        assert!(saw_quality);
        assert!(saw_mode);
        assert!(saw_slope);
        assert!(saw_surface_normal);
        assert!(saw_transfer);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_reads_emitter_and_camera_actions() {
        let root = temp_root("emitter_camera_variants");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn register_logic(canvas: &mut Canvas) {
    canvas.on_update(|canvas| {
        canvas.run(Action::SpawnEmitter { emitter: EmitterBuilder::new("trail").build() });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::AttachEmitter {
            emitter_name: "trail".to_owned(),
            target: Target::name("player"),
            location: Some(Location::at_target(Target::name("player"))),
        });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetEmitterCollision { name: "trail".to_owned(), value: CollisionResponse::Bounce { elasticity: 0.8 } });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetEmitterShape { name: "trail".to_owned(), value: ParticleShape::Rect { aspect_ratio: 2.0 } });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetEmitterColorEnd { name: "trail".to_owned(), value: Some((255, 120, 0, 0)) });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::CameraFlashWith {
            color: Color(255, 255, 255, 200),
            duration: 0.2,
            mode: FlashMode::Pulse,
            ease: FlashEase::Smooth,
            intensity: 0.9,
            freeze_frame: 0.05,
        });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetGlow { target: Target::name("player"), color: Color::from_rgb(120, 220, 255), width: 3.0 });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetTint { target: Target::name("player"), color: Color(255, 128, 128, 220) });
    });
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_logic_tree_count, 8);

        let trees = &state.manifest.scenes[0].logic_trees;
        let mut saw_spawn = false;
        let mut saw_attach = false;
        let mut saw_collision = false;
        let mut saw_shape = false;
        let mut saw_color_end = false;
        let mut saw_flash = false;
        let mut saw_glow = false;
        let mut saw_tint = false;

        for tree in trees {
            for node in &tree.nodes {
                if let LogicNode::Action(action) = node {
                    match action {
                        QuartzAction::SpawnEmitter { name } => {
                            saw_spawn = true;
                            assert_eq!(name, "trail");
                        }
                        QuartzAction::AttachEmitter { location, .. } => {
                            saw_attach = true;
                            assert!(matches!(location, Some(crate::core::quartz_domain::QuartzLocationRef::AtTarget(_))));
                        }
                        QuartzAction::SetEmitterCollision { mode, .. } => {
                            saw_collision = true;
                            assert_eq!(mode, "Bounce");
                        }
                        QuartzAction::SetEmitterShape { shape, .. } => {
                            saw_shape = true;
                            assert_eq!(shape, "Rect");
                        }
                        QuartzAction::SetEmitterColorEnd { rgba, .. } => {
                            saw_color_end = true;
                            assert_eq!(*rgba, Some([255, 120, 0, 0]));
                        }
                        QuartzAction::CameraFlashWith { mode, ease, .. } => {
                            saw_flash = true;
                            assert_eq!(mode, "Pulse");
                            assert_eq!(ease, "Smooth");
                        }
                        QuartzAction::SetGlow { color_rgb, .. } => {
                            saw_glow = true;
                            assert_eq!(*color_rgb, [120, 220, 255]);
                        }
                        QuartzAction::SetTint { color_rgba, .. } => {
                            saw_tint = true;
                            assert_eq!(*color_rgba, [255, 128, 128, 220]);
                        }
                        _ => {}
                    }
                }
            }
        }

        assert!(saw_spawn);
        assert!(saw_attach);
        assert!(saw_collision);
        assert!(saw_shape);
        assert!(saw_color_end);
        assert!(saw_flash);
        assert!(saw_glow);
        assert!(saw_tint);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_reads_gravity_planet_action_variants() {
        let root = temp_root("gravity_planet_variants");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn register_logic(canvas: &mut Canvas) {
    canvas.on_update(|canvas| {
        canvas.run(Action::EnableCrystalline);
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetGravityStrength { target: Target::name("player"), value: 9.8 });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetPlanetRadius { target: Target::name("planet"), value: 240.0 });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetGravityTarget { target: Target::name("player"), tag: "planet".to_owned() });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetGravityInfluenceMult { target: Target::name("player"), value: 2.5 });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetGravityFalloff { target: Target::name("player"), falloff: GravityFalloff::InverseSquare });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetGravityAllSources { target: Target::name("player"), enabled: true });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetAlignToSlope { target: Target::name("player"), enabled: true });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::SetAlignToSlopeSpeed { target: Target::name("player"), value: 3.5 });
    });
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_logic_tree_count, 9);

        let trees = &state.manifest.scenes[0].logic_trees;
        let mut saw_enable = false;
        let mut saw_strength = false;
        let mut saw_radius = false;
        let mut saw_target = false;
        let mut saw_mult = false;
        let mut saw_falloff = false;
        let mut saw_all_sources = false;
        let mut saw_align = false;
        let mut saw_align_speed = false;

        for tree in trees {
            for node in &tree.nodes {
                if let LogicNode::Action(action) = node {
                    match action {
                        QuartzAction::EnableCrystalline => saw_enable = true,
                        QuartzAction::SetGravityStrength { value, .. } => {
                            saw_strength = true;
                            assert_eq!(*value, 9.8);
                        }
                        QuartzAction::SetPlanetRadius { value, .. } => {
                            saw_radius = true;
                            assert_eq!(*value, 240.0);
                        }
                        QuartzAction::SetGravityTarget { tag, .. } => {
                            saw_target = true;
                            assert_eq!(tag, "planet");
                        }
                        QuartzAction::SetGravityInfluenceMult { value, .. } => {
                            saw_mult = true;
                            assert_eq!(*value, 2.5);
                        }
                        QuartzAction::SetGravityFalloff { falloff, .. } => {
                            saw_falloff = true;
                            assert_eq!(falloff, "InverseSquare");
                        }
                        QuartzAction::SetGravityAllSources { enabled, .. } => {
                            saw_all_sources = true;
                            assert!(*enabled);
                        }
                        QuartzAction::SetAlignToSlope { enabled, .. } => {
                            saw_align = true;
                            assert!(*enabled);
                        }
                        QuartzAction::SetAlignToSlopeSpeed { value, .. } => {
                            saw_align_speed = true;
                            assert_eq!(*value, 3.5);
                        }
                        _ => {}
                    }
                }
            }
        }

        assert!(saw_enable);
        assert!(saw_strength);
        assert!(saw_radius);
        assert!(saw_target);
        assert!(saw_mult);
        assert!(saw_falloff);
        assert!(saw_all_sources);
        assert!(saw_align);
        assert!(saw_align_speed);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_reads_spawn_and_plugincall() {
        let root = temp_root("spawn_plugincall_variants");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn register_logic(canvas: &mut Canvas) {
    canvas.on_update(|canvas| {
        canvas.run(Action::PluginCall {
            name: "terrain_collision".to_owned(),
            payload: std::sync::Arc::new("refresh".to_owned()),
        });
    });
    canvas.on_update(|canvas| {
        canvas.run(Action::Spawn {
            object: Box::new(spawn_enemy(canvas)),
            location: Location::at_target(Target::name("player")),
        });
    });
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_logic_tree_count, 2);

        let trees = &state.manifest.scenes[0].logic_trees;
        let mut saw_plugincall = false;
        let mut saw_spawn = false;

        for tree in trees {
            for node in &tree.nodes {
                if let LogicNode::Action(action) = node {
                    match action {
                        QuartzAction::PluginCall { name, payload } => {
                            saw_plugincall = true;
                            assert_eq!(name, "terrain_collision");
                            assert_eq!(payload, "refresh");
                        }
                        QuartzAction::Spawn { template_id, location } => {
                            saw_spawn = true;
                            assert_eq!(template_id, "enemy");
                            assert!(matches!(location, QuartzLocationRef::AtTarget(_)));
                        }
                        _ => {}
                    }
                }
            }
        }

        assert!(saw_plugincall);
        assert!(saw_spawn);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_reads_full_action_condition_matrix() {
        let root = temp_root("full_action_condition_matrix");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn register_logic(canvas: &mut Canvas) {
    canvas.on_update(|canvas| {
        canvas.run(Action::Conditional {
            condition: Condition::Compare(Expr::var("score"), CompareOp::Ge, Expr::f32(10.0)),
            if_true: Box::new(Action::SetVar {
                name: "phase".to_owned(),
                value: Expr::str("combat"),
            }),
            if_false: Some(Box::new(Action::Custom {
                name: "idle".to_owned(),
            })),
        });
    });

    canvas.on_update(|canvas| {
        canvas.run(Action::Multi {
            actions: vec![
                Action::SetPosition {
                    target: Target::name("player"),
                    x: 320.0,
                    y: 240.0,
                },
                Action::Spawn {
                    object: Box::new(spawn_enemy(canvas)),
                    location: Location::at_target(Target::name("player")),
                },
                Action::PluginCall {
                    name: "terrain_collision".to_owned(),
                    payload: std::sync::Arc::new("refresh".to_owned()),
                },
            ],
        });
    });

    canvas.on_update(|canvas| {
        canvas.run(Action::Conditional {
            condition: Condition::EmitterActive("trail".to_owned()),
            if_true: Box::new(Action::SetGravityStrength {
                target: Target::name("player"),
                value: 9.8,
            }),
            if_false: None,
        });
    });
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_logic_tree_count, 3);
        assert!(report.fallback_manual_override_files.is_empty());

        let trees = &state.manifest.scenes[0].logic_trees;
        let mut saw_compare = false;
        let mut saw_emitter_active = false;
        let mut saw_setposition = false;
        let mut saw_spawn = false;
        let mut saw_plugincall = false;
        let mut saw_gravity_strength = false;

        for tree in trees {
            for node in &tree.nodes {
                if let LogicNode::Action(action) = node {
                    match action {
                        QuartzAction::Conditional {
                            condition,
                            if_true,
                            ..
                        } => {
                            match condition {
                                QuartzCondition::Compare { .. } => saw_compare = true,
                                QuartzCondition::EmitterActive { emitter } => {
                                    saw_emitter_active = true;
                                    assert_eq!(emitter, "trail");
                                }
                                _ => {}
                            }

                            if let QuartzAction::SetGravityStrength { value, .. } = if_true.as_ref() {
                                saw_gravity_strength = true;
                                assert_eq!(*value, 9.8);
                            }
                        }
                        QuartzAction::Multi { actions } => {
                            for nested in actions {
                                match nested {
                                    QuartzAction::SetPosition { x, y, .. } => {
                                        saw_setposition = true;
                                        assert_eq!(*x, 320.0);
                                        assert_eq!(*y, 240.0);
                                    }
                                    QuartzAction::Spawn { template_id, location } => {
                                        saw_spawn = true;
                                        assert_eq!(template_id, "enemy");
                                        assert!(matches!(location, QuartzLocationRef::AtTarget(_)));
                                    }
                                    QuartzAction::PluginCall { name, payload } => {
                                        saw_plugincall = true;
                                        assert_eq!(name, "terrain_collision");
                                        assert_eq!(payload, "refresh");
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        assert!(saw_compare);
        assert!(saw_emitter_active);
        assert!(saw_setposition);
        assert!(saw_spawn);
        assert!(saw_plugincall);
        assert!(saw_gravity_strength);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_import_preserves_typed_not_and_tuple_multi_in_events() {
        let root = temp_root("typed_not_tuple_multi_events");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        state.manifest.scenes[0].source_file = "src/scenes/main_scene.rs".to_owned();
        let scene_file = "src/scenes/main_scene.rs";
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn register_events(canvas: &mut Canvas) {
    canvas.add_event(
        GameEvent::KeyHold {
            key: Key::Character('a'.to_string()),
            action: Action::Conditional {
                condition: Condition::Not(Box::new(Condition::VarExists("game_over_true".into()))),
                if_true: Box::new(Action::Multi(vec![
                    Action::AddRotation {
                        target: Target::name("player"),
                        value: -3.0,
                    },
                    Action::ModVar {
                        name: "player_angle".into(),
                        op: MathOp::Sub,
                        operand: Expr::f32(3.0),
                    },
                ])),
                if_false: None,
            },
            target: Target::name("player"),
            modifiers: None,
        },
        Target::name("player"),
    );
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert_eq!(report.imported_event_count, 1);

        let binding = &state.manifest.scenes[0].events[0];
        let Some(action) = &binding.action else {
            panic!("expected imported event action");
        };

        match action {
            QuartzAction::Conditional {
                condition,
                if_true,
                if_false,
            } => {
                assert!(if_false.is_none());
                match condition {
                    QuartzCondition::Not { inner } => match inner.as_ref() {
                        QuartzCondition::VarExists { variable } => {
                            assert_eq!(variable, "game_over_true");
                        }
                        other => panic!("expected VarExists inside Not, got {other:?}"),
                    },
                    other => panic!("expected Not condition, got {other:?}"),
                }

                match if_true.as_ref() {
                    QuartzAction::Multi { actions } => {
                        assert_eq!(actions.len(), 2);
                        assert!(matches!(actions[0], QuartzAction::AddRotation { .. }));
                        assert!(matches!(actions[1], QuartzAction::ModVar { .. }));
                    }
                    other => panic!("expected tuple-style Multi to import as QuartzAction::Multi, got {other:?}"),
                }
            }
            other => panic!("expected conditional event action, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unsupported_matrix_entries_fall_back_to_manual_override() {
        let root = temp_root("unsupported_matrix_entries");
        std::fs::create_dir_all(root.join("src/scenes")).unwrap();
        let mut state = EditorProjectState::new("import_test".to_owned());
        let scene_file = "src/scenes/main_scene.rs";
        state.manifest.scenes[0].source_file = scene_file.to_owned();
        std::fs::write(
            root.join(scene_file),
            r#"use quartz::prelude::*;

pub fn register_logic(canvas: &mut Canvas) {
    // malformed syntax forces semantic importer fallback to ManualFileOverride
    let _broken = ;
}
"#,
        )
        .unwrap();

        let report = import_files_into_state(&mut state, &root, &[scene_file.to_owned()], true).unwrap();
        assert!(report.imported_files.is_empty());
        assert_eq!(report.fallback_manual_override_files, vec![scene_file.to_owned()]);
        assert!(state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .any(|block| block.kind == CustomCodeKind::ManualFileOverride && block.output_file == scene_file));

        let _ = std::fs::remove_dir_all(&root);
    }
}

fn normalize_rel_like(path: &str) -> String {
    path.trim().replace('\\', "/")
}

fn is_canvas_on_update_call(call: &ExprMethodCall) -> bool {
    if call.method != "on_update" {
        return false;
    }
    if expr_ident_name(&call.receiver).as_deref() != Some("canvas") {
        return false;
    }
    let Some(Expr::Closure(closure)) = call.args.first() else {
        return false;
    };
    closure.inputs.len() == 1
}

fn collect_f32_constants_from_ast(ast: &File) -> std::collections::BTreeMap<String, f32> {
    let mut out = std::collections::BTreeMap::<String, f32>::new();

    for _ in 0..8 {
        let mut changed = false;
        for item in &ast.items {
            let Item::Const(item_const) = item else { continue; };
            let name = item_const.ident.to_string();
            if out.contains_key(&name) {
                continue;
            }
            if let Ok(value) = expr_to_f32_with_consts(&item_const.expr, &out) {
                out.insert(name, value);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    out
}

fn extract_setup_scene_runtime_statements(func: &ItemFn) -> Option<String> {
    let builder_locals = func
        .block
        .stmts
        .iter()
        .filter_map(|stmt| {
            let Stmt::Local(local) = stmt else { return None; };
            let Some(init) = &local.init else { return None; };
            let Pat::Ident(PatIdent { ident, .. }) = &local.pat else {
                return None;
            };
            let Some((object_id, _)) = extract_builder_chain(&init.expr) else {
                return None;
            };
            let object_id = object_id.unwrap_or_else(|| ident.to_string());
            Some((ident.to_string(), object_id))
        })
        .collect::<Vec<_>>();

    let rewrite_stmt = |text: String| {
        rewrite_builder_local_references(text, &builder_locals)
    };

    let lines = func
        .block
        .stmts
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Local(local) => {
                let Some(init) = &local.init else { return None; };
                if extract_builder_chain(&init.expr).is_some() {
                    return None;
                }
                rewrite_stmt(stmt.to_token_stream().to_string())
            }
            Stmt::Expr(Expr::MethodCall(call), _) => {
                if let Some(receiver) = expr_ident_name(&call.receiver)
                    && receiver == "canvas"
                    && (call.method == "add_game_object"
                        || call.method == "set_var"
                        || call.method == "mod_var")
                {
                    return None;
                }
                rewrite_stmt(stmt.to_token_stream().to_string())
            }
            _ => rewrite_stmt(stmt.to_token_stream().to_string()),
        })
        .collect::<Vec<_>>();

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn rewrite_builder_local_references(
    mut text: String,
    builder_locals: &[(String, String)],
) -> Option<String> {
    for (local_name, object_id) in builder_locals {
        text = text.replace(&format!("{local_name} ."), &format!("{object_id} ."));
        text = text.replace(&format!("{local_name}."), &format!("{object_id}."));
        text = text.replace(&format!("& {local_name}"), &format!("& {object_id}"));
        text = text.replace(&format!("&{local_name}"), &format!("&{object_id}"));
    }

    // Drop statements that still reference a renamed builder local after rewrite.
    if builder_locals.iter().any(|(local_name, object_id)| {
        local_name != object_id && contains_identifier(&text, local_name)
    }) {
        return None;
    }

    Some(text)
}

fn contains_identifier(text: &str, ident: &str) -> bool {
    if ident.is_empty() {
        return false;
    }

    let mut search_from = 0usize;
    while let Some(found_at) = text[search_from..].find(ident) {
        let start = search_from + found_at;
        let end = start + ident.len();
        let left_ok = text[..start]
            .chars()
            .next_back()
            .map(|ch| !is_ident_char(ch))
            .unwrap_or(true);
        let right_ok = text[end..]
            .chars()
            .next()
            .map(|ch| !is_ident_char(ch))
            .unwrap_or(true);
        if left_ok && right_ok {
            return true;
        }
        search_from = end;
    }

    false
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}