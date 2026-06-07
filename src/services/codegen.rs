use crate::core::project::EditorProjectState;
use crate::core::quartz_domain::{
    CrystallineConfigProfile, CrystallineQuality, LogicNode, ObjectVisualAssetMode,
    QuartzObjectCollisionMode,
    QuartzAction, QuartzCondition, QuartzEventBinding, QuartzEventKind, QuartzExpr,
    QuartzExprKind, QuartzMathOp,
};
use crate::services::codegen_text::{
    comp_op_expr, key_expr, location_expr, modifiers_expr, mouse_button_expr,
    physics_material_expr, scroll_axis_expr, target_expr, gravity_falloff_expr,
};

fn f32_lit(value: f32) -> String {
    if !value.is_finite() {
        return "0.0".to_owned();
    }
    let mut out = format!("{}", value);
    if !out.contains('.') && !out.contains('e') && !out.contains('E') {
        out.push_str(".0");
    }
    out
}

fn rust_str_lit(value: &str) -> String {
    format!("{:?}", value)
}

fn looks_like_rust_action_expr(raw: &str) -> bool {
    let trimmed = raw.trim_start();
    trimmed.starts_with("Action::") || trimmed.starts_with("Action ::")
}

fn looks_like_rust_condition_expr(raw: &str) -> bool {
    let trimmed = raw.trim_start();
    trimmed.starts_with("Condition::") || trimmed.starts_with("Condition ::")
}

pub fn generate_quartz_preview(state: &EditorProjectState) -> String {
    let Some(scene) = state.manifest.scenes.get(state.active_scene_index) else {
        return "// no active scene".to_owned();
    };

    let mut out = String::new();
    out.push_str("use quartz::prelude::*;\n\n");
    out.push_str("pub fn setup_scene(canvas: &mut Canvas) {\n");
    out.push_str(&scene_setup_physics_lines(scene));

    for obj in &scene.objects {
        if !obj.enabled {
            continue;
        }
        if obj.spawn_only {
            out.push_str(&object_function_source(obj));
            out.push('\n');
            continue;
        }
        out.push_str(&format!("    let mut {} = GameObject::build({})\n", obj.id, rust_str_lit(&obj.id)));
        out.push_str(&format!("        .size({}, {})\n", f32_lit(obj.w), f32_lit(obj.h)));
        out.push_str(&format!("        .position({}, {})\n", f32_lit(obj.x), f32_lit(obj.y)));
        out.push_str(&format!("        .layer({})\n", obj.layer));
        out.push_str(&format!(
            "        .momentum({}, {})\n",
            f32_lit(obj.advanced.momentum_x), f32_lit(obj.advanced.momentum_y)
        ));
        out.push_str(&format!(
            "        .resistance({}, {})\n",
            f32_lit(obj.advanced.resistance_x), f32_lit(obj.advanced.resistance_y)
        ));
        out.push_str(&format!("        .gravity({})\n", f32_lit(obj.advanced.gravity)));
        out.push_str(&format!("        .rotation({})\n", f32_lit(obj.advanced.rotation_deg)));
        out.push_str(&format!(
            "        .rotation_resistance({})\n",
            f32_lit(obj.advanced.rotation_resistance)
        ));
        out.push_str(&format!(
            "        .pivot({}, {})\n",
            f32_lit(obj.advanced.pivot_x), f32_lit(obj.advanced.pivot_y)
        ));
        out.push_str(&format!(
            "        .material({})\n",
            physics_material_expr(&obj.advanced.material)
        ));
        out.push_str(&format!(
            "        .collision_layer({})\n",
            obj.advanced.collision_layer
        ));
        out.push_str(&format!(
            "        .collision_mask({})\n",
            obj.advanced.collision_mask
        ));
        append_advanced_builder_lines(&mut out, &obj.advanced);
        if obj.visual_asset_mode == ObjectVisualAssetMode::StaticImage {
            if let Some(image_expr) = static_image_expr(obj) {
                out.push_str(&format!("        .image({})\n", image_expr));
            }
        }
        append_camera_space_builder_lines(&mut out, &obj.advanced);
        if !obj.tags.is_empty() {
            for t in &obj.tags {
                out.push_str(&format!("        .tag({})\n", rust_str_lit(t)));
            }
        }
        out.push_str("        .finish();\n");
        append_post_build_lines(&mut out, &obj.id, &obj.advanced);
        if obj.visual_asset_mode == ObjectVisualAssetMode::AnimatedSprite {
            if let Some(bytes_expr) = asset_include_expr(&obj.visual_asset_path) {
                out.push_str(&format!(
                    "    {}.set_animation(quartz::sprite::load_animation({}, ({}, {}), {}));\n",
                    obj.id, bytes_expr, obj.w, obj.h, obj.visual_asset_fps
                ));
            }
        }
        out.push_str(&format!(
            "    canvas.add_game_object({}.to_owned(), {});\n\n",
            rust_str_lit(&obj.id),
            obj.id
        ));
    }
    out.push_str("}\n\n");

    out.push_str("pub fn register_logic(canvas: &mut Canvas) {\n");
    for tree in &scene.logic_trees {
        out.push_str(&format!("    // Update Script: {}\n", tree.name));
        out.push_str("    canvas.on_update(|canvas| {\n");
        for line in emit_action_lines(&tree.nodes, "        ") {
            out.push_str(&line);
            out.push('\n');
        }
        out.push_str("    });\n");
    }
    out.push_str("}\n");

    out.push_str("\n");
    out.push_str("pub fn register_events(canvas: &mut Canvas) {\n");
    for event in &scene.events {
        write_event_binding(&mut out, event, &scene.logic_trees, 1);
    }
    out.push_str("}\n");

    out
}

pub fn generated_file_name(state: &EditorProjectState) -> String {
    let scene_name = state
        .manifest
        .scenes
        .get(state.active_scene_index)
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "scene".to_owned());
    format!("{}_generated.rs", scene_name.replace(' ', "_").to_lowercase())
}

pub fn file_module_name(rel_path: &str) -> String {
    let mut module_name = String::from("component");
    for ch in rel_path.chars() {
        if ch.is_ascii_alphanumeric() {
            module_name.push(ch.to_ascii_lowercase());
        } else {
            module_name.push('_');
        }
    }
    module_name
}

pub fn object_function_name(object: &crate::core::quartz_domain::QuartzObjectBlueprint) -> String {
    format!("spawn_{}", object.id)
}

pub fn event_function_name(binding: &QuartzEventBinding) -> String {
    format!("bind_{}", binding.id)
}

pub fn object_function_source(object: &crate::core::quartz_domain::QuartzObjectBlueprint) -> String {
    let mut out = String::new();
    if object.spawn_only {
        // Template functions return a fresh `GameObject` — caller passes it to Action::Spawn.
        out.push_str(&format!(
            "pub fn {}(canvas: &mut Canvas) -> GameObject {{\n",
            object_function_name(object)
        ));
        out.push_str(&spawn_template_body(object));
        out.push_str("}\n");
    } else {
        out.push_str(&format!("pub fn {}(canvas: &mut Canvas) {{\n", object_function_name(object)));
        out.push_str(&object_registration_body(object));
        out.push_str("}\n");
    }
    out
}

pub fn event_function_source(
    binding: &QuartzEventBinding,
    logic_trees: &[crate::core::quartz_domain::LogicTree],
) -> String {
    let mut out = String::new();
    out.push_str(&format!("pub fn {}(canvas: &mut Canvas) {{\n", event_function_name(binding)));
    out.push_str(&event_binding_body(binding, logic_trees));
    out.push_str("}\n");
    out
}

pub fn logic_tree_function_name(tree: &crate::core::quartz_domain::LogicTree) -> String {
    format!("update_{}", tree.id)
}

pub fn logic_tree_function_source(tree: &crate::core::quartz_domain::LogicTree) -> String {
    let mut out = String::new();
    out.push_str(&format!("pub fn {}(canvas: &mut Canvas) {{\n", logic_tree_function_name(tree)));
    out.push_str("    canvas.on_update(|canvas| {\n");
    for line in emit_action_lines(&tree.nodes, "        ") {
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str("    });\n");
    out.push_str("}\n");
    out
}

pub fn logic_tree_action_expr(tree: &crate::core::quartz_domain::LogicTree) -> String {
    nodes_to_action_expr(&tree.nodes)
}

fn nodes_to_action_expr(nodes: &[LogicNode]) -> String {
    match nodes {
        [] => "Action::Multi(vec![])".to_owned(),
        [single] => node_to_action_expr(single),
        many => {
            let parts = many.iter().map(node_to_action_expr).collect::<Vec<_>>().join(", ");
            format!("Action::Multi(vec![{}])", parts)
        }
    }
}

pub fn object_registration_body(object: &crate::core::quartz_domain::QuartzObjectBlueprint) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "    let mut {} = GameObject::build({})\n",
        object.id,
        rust_str_lit(&object.id)
    ));
    out.push_str(&format!("        .size({}, {})\n", f32_lit(object.w), f32_lit(object.h)));
    out.push_str(&format!("        .position({}, {})\n", f32_lit(object.x), f32_lit(object.y)));
    out.push_str(&format!("        .layer({})\n", object.layer));
    out.push_str(&format!(
        "        .momentum({}, {})\n",
        f32_lit(object.advanced.momentum_x), f32_lit(object.advanced.momentum_y)
    ));
    out.push_str(&format!(
        "        .resistance({}, {})\n",
        f32_lit(object.advanced.resistance_x), f32_lit(object.advanced.resistance_y)
    ));
    out.push_str(&format!("        .gravity({})\n", f32_lit(object.advanced.gravity)));
    out.push_str(&format!("        .rotation({})\n", f32_lit(object.advanced.rotation_deg)));
    out.push_str(&format!(
        "        .rotation_resistance({})\n",
        f32_lit(object.advanced.rotation_resistance)
    ));
    out.push_str(&format!(
        "        .pivot({}, {})\n",
        f32_lit(object.advanced.pivot_x), f32_lit(object.advanced.pivot_y)
    ));
    out.push_str(&format!(
        "        .material({})\n",
        physics_material_expr(&object.advanced.material)
    ));
    out.push_str(&format!(
        "        .collision_layer({})\n",
        object.advanced.collision_layer
    ));
    out.push_str(&format!(
        "        .collision_mask({})\n",
        object.advanced.collision_mask
    ));
    append_advanced_builder_lines(&mut out, &object.advanced);
    if object.visual_asset_mode == ObjectVisualAssetMode::StaticImage {
        if let Some(image_expr) = static_image_expr(object) {
            out.push_str(&format!("        .image({})\n", image_expr));
        }
    }
    append_camera_space_builder_lines(&mut out, &object.advanced);
    if !object.tags.is_empty() {
        for tag in &object.tags {
            out.push_str(&format!("        .tag({})\n", rust_str_lit(tag)));
        }
    }
    out.push_str("        .finish();\n");
    append_post_build_lines(&mut out, &object.id, &object.advanced);
    if object.visual_asset_mode == ObjectVisualAssetMode::None
        && object
            .tags
            .iter()
            .any(|tag| tag.eq_ignore_ascii_case("player"))
        && object.advanced.collision_mode == QuartzObjectCollisionMode::SolidCircle
    {
        let [r, g, b] = object.color_rgb;
        out.push_str(&format!(
            "    {}.set_drawable(Box::new(quartz::sprite::solid_circle({}, Color({}, {}, {}, 255))));\n",
            object.id,
            f32_lit(object.w.max(object.h)),
            r,
            g,
            b
        ));
    }
    if object.visual_asset_mode == ObjectVisualAssetMode::AnimatedSprite {
        if let Some(bytes_expr) = asset_include_expr(&object.visual_asset_path) {
            out.push_str(&format!(
                "    {}.set_animation(quartz::sprite::load_animation({}, ({}, {}), {}));\n",
                object.id, bytes_expr, object.w, object.h, object.visual_asset_fps
            ));
        }
    }
    out.push_str(&format!(
        "    canvas.add_game_object({}.to_owned(), {});\n",
        rust_str_lit(&object.id), object.id
    ));
    out
}

/// Builds a GameObject local (including post-build mutations like set_drawable / is_platform)
/// but does NOT emit `canvas.add_game_object`. Use alongside `object_add_line` when you need
/// to interleave setup_runtime code between builds and adds.
pub fn object_build_body(object: &crate::core::quartz_domain::QuartzObjectBlueprint) -> String {
    use crate::core::quartz_domain::ObjectVisualAssetMode;
    let mut out = String::new();
    out.push_str(&format!(
        "    let mut {} = GameObject::build({})\n",
        object.id,
        rust_str_lit(&object.id)
    ));
    out.push_str(&format!("        .size({}, {})\n", f32_lit(object.w), f32_lit(object.h)));
    out.push_str(&format!("        .position({}, {})\n", f32_lit(object.x), f32_lit(object.y)));
    out.push_str(&format!("        .layer({})\n", object.layer));
    out.push_str(&format!(
        "        .momentum({}, {})\n",
        f32_lit(object.advanced.momentum_x), f32_lit(object.advanced.momentum_y)
    ));
    out.push_str(&format!(
        "        .resistance({}, {})\n",
        f32_lit(object.advanced.resistance_x), f32_lit(object.advanced.resistance_y)
    ));
    out.push_str(&format!("        .gravity({})\n", f32_lit(object.advanced.gravity)));
    out.push_str(&format!("        .rotation({})\n", f32_lit(object.advanced.rotation_deg)));
    out.push_str(&format!(
        "        .rotation_resistance({})\n",
        f32_lit(object.advanced.rotation_resistance)
    ));
    out.push_str(&format!(
        "        .pivot({}, {})\n",
        f32_lit(object.advanced.pivot_x), f32_lit(object.advanced.pivot_y)
    ));
    out.push_str(&format!(
        "        .material({})\n",
        physics_material_expr(&object.advanced.material)
    ));
    out.push_str(&format!(
        "        .collision_layer({})\n",
        object.advanced.collision_layer
    ));
    out.push_str(&format!(
        "        .collision_mask({})\n",
        object.advanced.collision_mask
    ));
    append_advanced_builder_lines(&mut out, &object.advanced);
    if object.visual_asset_mode == ObjectVisualAssetMode::StaticImage {
        if let Some(image_expr) = static_image_expr(object) {
            out.push_str(&format!("        .image({})\n", image_expr));
        }
    }
    append_camera_space_builder_lines(&mut out, &object.advanced);
    if !object.tags.is_empty() {
        for tag in &object.tags {
            out.push_str(&format!("        .tag({})\n", rust_str_lit(tag)));
        }
    }
    out.push_str("        .finish();\n");
    append_post_build_lines(&mut out, &object.id, &object.advanced);
    if object.visual_asset_mode == ObjectVisualAssetMode::None
        && object.tags.iter().any(|tag| tag.eq_ignore_ascii_case("player"))
        && object.advanced.collision_mode == QuartzObjectCollisionMode::SolidCircle
    {
        let [r, g, b] = object.color_rgb;
        out.push_str(&format!(
            "    {}.set_drawable(Box::new(quartz::sprite::solid_circle({}, Color({}, {}, {}, 255))));\n",
            object.id,
            f32_lit(object.w.max(object.h)),
            r,
            g,
            b
        ));
    }
    if object.visual_asset_mode == ObjectVisualAssetMode::AnimatedSprite {
        if let Some(bytes_expr) = asset_include_expr(&object.visual_asset_path) {
            out.push_str(&format!(
                "    {}.set_animation(quartz::sprite::load_animation({}, ({}, {}), {}));\n",
                object.id, bytes_expr, object.w, object.h, object.visual_asset_fps
            ));
        }
    }
    out
}

/// Emits just the `canvas.add_game_object(...)` line for an already-built object local.
pub fn object_add_line(object: &crate::core::quartz_domain::QuartzObjectBlueprint) -> String {
    format!(
        "    canvas.add_game_object({}.to_owned(), {});\n",
        rust_str_lit(&object.id), object.id
    )
}

/// Like `object_registration_body` but for spawn-only templates:
/// builds the `GameObject` and returns it — does NOT call `canvas.add_game_object()`.
/// The caller (Spawn action) passes the returned value to `Action::Spawn { object: Box::new(...) }`.
pub fn spawn_template_body(object: &crate::core::quartz_domain::QuartzObjectBlueprint) -> String {
    use crate::core::quartz_domain::ObjectVisualAssetMode;
    let mut out = String::new();
    out.push_str(&format!(
        "    let mut {} = GameObject::build({})\n",
        object.id,
        rust_str_lit(&object.id)
    ));
    out.push_str(&format!("        .size({}, {})\n", f32_lit(object.w), f32_lit(object.h)));
    out.push_str(&format!("        .position({}, {})\n", f32_lit(object.x), f32_lit(object.y)));
    out.push_str(&format!("        .layer({})\n", object.layer));
    out.push_str(&format!(
        "        .momentum({}, {})\n",
        f32_lit(object.advanced.momentum_x), f32_lit(object.advanced.momentum_y)
    ));
    out.push_str(&format!(
        "        .resistance({}, {})\n",
        f32_lit(object.advanced.resistance_x), f32_lit(object.advanced.resistance_y)
    ));
    out.push_str(&format!("        .gravity({})\n", f32_lit(object.advanced.gravity)));
    out.push_str(&format!("        .rotation({})\n", f32_lit(object.advanced.rotation_deg)));
    out.push_str(&format!(
        "        .rotation_resistance({})\n",
        f32_lit(object.advanced.rotation_resistance)
    ));
    out.push_str(&format!(
        "        .pivot({}, {})\n",
        f32_lit(object.advanced.pivot_x), f32_lit(object.advanced.pivot_y)
    ));
    out.push_str(&format!(
        "        .material({})\n",
        physics_material_expr(&object.advanced.material)
    ));
    out.push_str(&format!(
        "        .collision_layer({})\n",
        object.advanced.collision_layer
    ));
    out.push_str(&format!(
        "        .collision_mask({})\n",
        object.advanced.collision_mask
    ));
    append_advanced_builder_lines(&mut out, &object.advanced);
    if object.visual_asset_mode == ObjectVisualAssetMode::StaticImage {
        if let Some(image_expr) = static_image_expr(object) {
            out.push_str(&format!("        .image({})\n", image_expr));
        }
    }
    append_camera_space_builder_lines(&mut out, &object.advanced);
    if !object.tags.is_empty() {
        for tag in &object.tags {
            out.push_str(&format!("        .tag({})\n", rust_str_lit(tag)));
        }
    }
    // Build and return — no canvas.add_game_object() for templates
    out.push_str("        .finish();\n");
    append_post_build_lines(&mut out, &object.id, &object.advanced);
    if object.visual_asset_mode == ObjectVisualAssetMode::AnimatedSprite {
        if let Some(bytes_expr) = asset_include_expr(&object.visual_asset_path) {
            out.push_str(&format!(
                "    {}.set_animation(quartz::sprite::load_animation({}, ({}, {}), {}));\n",
                object.id, bytes_expr, object.w, object.h, object.visual_asset_fps
            ));
        }
    }
    out.push_str(&format!("    {}\n", object.id));
    out
}

fn append_advanced_builder_lines(
    out: &mut String,
    advanced: &crate::core::quartz_domain::ObjectAdvancedParams,
) {
    match advanced.collision_mode {
        QuartzObjectCollisionMode::Auto => {}
        QuartzObjectCollisionMode::NonPlatform => {
            out.push_str("        .collision_mode(CollisionMode::non_platform())\n");
        }
        QuartzObjectCollisionMode::Surface => {
            out.push_str("        .collision_mode(CollisionMode::Surface)\n");
        }
        QuartzObjectCollisionMode::SolidRectangle => {
            out.push_str("        .collision_mode(CollisionMode::solid())\n");
        }
        QuartzObjectCollisionMode::SolidCircle => {
            out.push_str(&format!(
                "        .collision_mode(CollisionMode::solid_circle({}))\n",
                f32_lit(advanced.collision_circle_radius)
            ));
        }
    }

    if advanced.tint_enabled {
        out.push_str(&format!(
            "        .tint(Color({}, {}, {}, {}))\n",
            advanced.tint_rgba[0],
            advanced.tint_rgba[1],
            advanced.tint_rgba[2],
            advanced.tint_rgba[3]
        ));
    }
    if advanced.glow_enabled {
        out.push_str(&format!(
            "        .glow(GlowConfig {{ color: Color({}, {}, {}, {}), width: {} }})\n",
            advanced.glow_rgba[0],
            advanced.glow_rgba[1],
            advanced.glow_rgba[2],
            advanced.glow_rgba[3],
            advanced.glow_width
        ));
    }

    if advanced.slope_enabled {
        if advanced.slope_auto_rotation {
            out.push_str(&format!(
                "        .slope_auto_rotation({}, {})\n",
                advanced.slope_left_offset, advanced.slope_right_offset
            ));
        } else {
            out.push_str(&format!(
                "        .slope({}, {})\n",
                advanced.slope_left_offset, advanced.slope_right_offset
            ));
        }
    }
    if advanced.one_way {
        out.push_str("        .one_way()\n");
    }
    if advanced.surface_velocity_enabled {
        out.push_str(&format!(
            "        .surface_velocity({})\n",
            f32_lit(advanced.surface_velocity_x)
        ));
    }
    if advanced.surface_normal_enabled {
        out.push_str(&format!(
            "        .surface({}, {})\n",
            f32_lit(advanced.surface_normal_x), f32_lit(advanced.surface_normal_y)
        ));
    }
    if advanced.align_to_slope {
        out.push_str("        .align_to_slope()\n");
        out.push_str(&format!(
            "        .align_to_slope_speed({})\n",
            advanced.align_to_slope_speed
        ));
    }
    if advanced.planet_enabled {
        out.push_str(&format!("        .planet({})\n", f32_lit(advanced.planet_radius)));
    }
    if advanced.clip_enabled {
        out.push_str("        .clip()\n");
        if advanced.clip_origin_enabled {
            out.push_str(&format!(
                "        .clip_origin({}, {})\n",
                f32_lit(advanced.clip_origin_x), f32_lit(advanced.clip_origin_y)
            ));
        }
        if advanced.clip_size_enabled {
            out.push_str(&format!(
                "        .clip_size({}, {})\n",
                f32_lit(advanced.clip_size_w), f32_lit(advanced.clip_size_h)
            ));
        }
    }
    if advanced.gravity_target_enabled && !advanced.gravity_target_tag.trim().is_empty() {
        out.push_str(&format!(
            "        .gravity_target(\"{}\")\n",
            advanced.gravity_target_tag
        ));
    }
    out.push_str(&format!(
        "        .gravity_strength({})\n",
        f32_lit(advanced.gravity_strength)
    ));
    out.push_str(&format!(
        "        .gravity_influence_mult({})\n",
        f32_lit(advanced.gravity_influence_mult)
    ));
    out.push_str(&format!(
        "        .gravity_falloff({})\n",
        gravity_falloff_expr(advanced.gravity_falloff)
    ));
    if advanced.gravity_all_sources {
        out.push_str("        .all_gravity_sources()\n");
    }
    if advanced.gravity_identity_enabled && !advanced.gravity_identity.trim().is_empty() {
        out.push_str(&format!(
            "        .gravity_identity(\"{}\")\n",
            advanced.gravity_identity
        ));
    }
    if advanced.auto_align {
        out.push_str("        .auto_align()\n");
        out.push_str(&format!(
            "        .auto_align_speed({})\n",
            f32_lit(advanced.auto_align_speed)
        ));
        out.push_str(&format!(
            "        .auto_align_threshold({})\n",
            f32_lit(advanced.auto_align_threshold)
        ));
        out.push_str(&format!(
            "        .auto_align_min_depth({})\n",
            f32_lit(advanced.auto_align_min_depth)
        ));
    }
}

fn append_camera_space_builder_lines(
    out: &mut String,
    advanced: &crate::core::quartz_domain::ObjectAdvancedParams,
) {
    if advanced.screen_pin_enabled {
        out.push_str(&format!(
            "        .pin({}, {})\n",
            f32_lit(advanced.screen_pin_anchor_x), f32_lit(advanced.screen_pin_anchor_y)
        ));
        out.push_str(&format!(
            "        .pin_offset({}, {})\n",
            f32_lit(advanced.screen_pin_offset_x), f32_lit(advanced.screen_pin_offset_y)
        ));
    } else if advanced.is_camera_space_pinned() {
        out.push_str("        .screen_space()\n");
    } else if advanced.ignore_zoom {
        out.push_str("        .ignore_zoom()\n");
    }
}

fn append_post_build_lines(
    out: &mut String,
    object_id: &str,
    advanced: &crate::core::quartz_domain::ObjectAdvancedParams,
) {
    if advanced.collision_mode == QuartzObjectCollisionMode::Auto {
        if advanced.is_platform {
            out.push_str(&format!("    {}.is_platform = true;\n", object_id));
        }
        return;
    }

    out.push_str(&format!(
        "    {}.is_platform = {};\n",
        object_id,
        if advanced.is_platform { "true" } else { "false" }
    ));
}

pub fn scene_setup_physics_lines(scene: &crate::core::project::SceneDocument) -> String {
    if !scene.canvas.crystalline_enabled {
        return String::new();
    }

    let profile_expr = match scene.canvas.crystalline_profile {
        CrystallineConfigProfile::Platformer => "PhysicsConfig::platformer()",
        CrystallineConfigProfile::Floaty => "PhysicsConfig::floaty()",
        CrystallineConfigProfile::Realistic => "PhysicsConfig::realistic()",
        CrystallineConfigProfile::Arcade => "PhysicsConfig::arcade()",
    };
    let quality_expr = match scene.canvas.crystalline_quality {
        CrystallineQuality::Low => "PhysicsQuality::Low",
        CrystallineQuality::Medium => "PhysicsQuality::Medium",
        CrystallineQuality::High => "PhysicsQuality::High",
    };

    format!(
        "    canvas.enable_crystalline_with({profile_expr});\n    canvas.set_physics_quality({quality_expr});\n"
    )
}

pub fn event_binding_body(
    binding: &QuartzEventBinding,
    logic_trees: &[crate::core::quartz_domain::LogicTree],
) -> String {
    let mut out = String::new();
    let outer_target = target_expr(&binding.listener_target);
    let action_target = target_expr(&binding.action_target);
    let selected_tree_action = binding
        .linked_logic_tree_id
        .as_ref()
        .and_then(|tree_id| logic_trees.iter().find(|t| &t.id == tree_id))
        .map(|tree| nodes_to_action_expr(&tree.nodes));
    let fallback = event_action_expr(binding);
    let effective_action = selected_tree_action.unwrap_or(fallback);
    let event_expr = event_expr(binding, &action_target, &effective_action);
    out.push_str(&format!("    canvas.add_event({}, {});\n", event_expr, outer_target));
    if let Some(tree_id) = &binding.linked_logic_tree_id {
        out.push_str(&format!("    // linked_update_script_id: {}\n", tree_id));
    }
    out
}

fn node_to_action_expr(node: &LogicNode) -> String {
    match node {
        LogicNode::Action(action) => action_expr_inner(action),
        LogicNode::Branch {
            condition,
            then_nodes,
            else_nodes,
        } => {
            let if_true = nodes_to_action_expr(then_nodes);
            if else_nodes.is_empty() {
                format!(
                    "Action::Conditional {{ condition: {}, if_true: Box::new({}), if_false: None }}",
                    condition_expr(condition),
                    if_true
                )
            } else {
                let if_false = nodes_to_action_expr(else_nodes);
                format!(
                    "Action::Conditional {{ condition: {}, if_true: Box::new({}), if_false: Some(Box::new({})) }}",
                    condition_expr(condition),
                    if_true,
                    if_false
                )
            }
        }
    }
}

fn action_expr_inner(action: &QuartzAction) -> String {
    match action {
        QuartzAction::Teleport { target, location } => format!(
            "Action::Teleport {{ target: {}, location: {} }}",
            target_expr(target),
            location_expr(location)
        ),
        QuartzAction::ApplyMomentum { target, mx, my } => format!(
            "Action::ApplyMomentum {{ target: {}, value: ({}, {}) }}",
            target_expr(target),
            f32_lit(*mx),
            f32_lit(*my)
        ),
        QuartzAction::SetMomentum { target, mx, my } => format!(
            "Action::SetMomentum {{ target: {}, value: ({}, {}) }}",
            target_expr(target),
            f32_lit(*mx),
            f32_lit(*my)
        ),
        QuartzAction::SetResistance { target, rx, ry } => format!(
            "Action::SetResistance {{ target: {}, value: ({}, {}) }}",
            target_expr(target),
            f32_lit(*rx),
            f32_lit(*ry)
        ),
        QuartzAction::SetGravity { target, value } => {
            format!(
                "Action::SetGravity {{ target: {}, value: {} }}",
                target_expr(target),
                f32_lit(*value)
            )
        }
        QuartzAction::SetRotation { target, deg } => {
            format!(
                "Action::SetRotation {{ target: {}, value: {} }}",
                target_expr(target),
                f32_lit(*deg)
            )
        }
        QuartzAction::SetPivot { target, x, y } => {
            format!(
                "Action::SetPivot {{ target: {}, x: {}, y: {} }}",
                target_expr(target),
                f32_lit(*x),
                f32_lit(*y)
            )
        }
        QuartzAction::AddRotation { target, deg } => {
            format!(
                "Action::AddRotation {{ target: {}, value: {} }}",
                target_expr(target),
                f32_lit(*deg)
            )
        }
        QuartzAction::ApplyRotation { target, deg } => {
            format!(
                "Action::ApplyRotation {{ target: {}, value: {} }}",
                target_expr(target),
                f32_lit(*deg)
            )
        }
        QuartzAction::SetSize { target, w, h } => {
            format!(
                "Action::SetSize {{ target: {}, value: ({}, {}) }}",
                target_expr(target),
                f32_lit(*w),
                f32_lit(*h)
            )
        }
        QuartzAction::SetCollisionLayer { target, layer } => format!(
            "Action::SetCollisionLayer {{ target: {}, layer: {} }}",
            target_expr(target),
            layer
        ),
        QuartzAction::SetCameraRelative { target, enabled } => format!(
            "Action::SetCameraRelative {{ target: {}, enabled: {} }}",
            target_expr(target),
            enabled
        ),
        QuartzAction::SetRenderLayer { target, layer } => format!(
            "Action::SetRenderLayer {{ target: {}, layer: {} }}",
            target_expr(target),
            layer
        ),
        QuartzAction::Show { target } => {
            format!("Action::Show {{ target: {} }}", target_expr(target))
        }
        QuartzAction::Hide { target } => {
            format!("Action::Hide {{ target: {} }}", target_expr(target))
        }
        QuartzAction::Toggle { target } => {
            format!("Action::Toggle {{ target: {} }}", target_expr(target))
        }
        QuartzAction::Remove { target } => {
            format!("Action::Remove {{ target: {} }}", target_expr(target))
        }
        QuartzAction::AddTag { target, tag } => format!(
            "Action::AddTag {{ target: {}, tag: \"{}\".to_owned() }}",
            target_expr(target),
            tag
        ),
        QuartzAction::RemoveTag { target, tag } => format!(
            "Action::RemoveTag {{ target: {}, tag: \"{}\".to_owned() }}",
            target_expr(target),
            tag
        ),
        QuartzAction::SetAnimation {
            target,
            animation_asset,
            fps,
        } => {
            if let Some(bytes_expr) = asset_include_expr(animation_asset) {
                format!(
                    "Action::SetAnimation {{ target: {}, animation_bytes: {}, fps: {} }}",
                    target_expr(target),
                    bytes_expr,
                    fps
                )
            } else {
                "Action::Custom { name: \"missing_animation_asset\".to_owned() }".to_owned()
            }
        }
        QuartzAction::PlaySound {
            path,
            volume,
            looping,
        } => format!(
            "Action::PlaySound {{ path: \"{}\".to_owned(), options: SoundOptions::new().volume({}).looping({}) }}",
            path,
            volume,
            looping
        ),
        QuartzAction::SetZoom { value } => {
            format!("Action::SetZoom {{ value: {} }}", f32_lit(*value))
        }
        QuartzAction::SmoothZoom { value } => {
            format!("Action::SmoothZoom {{ value: {} }}", f32_lit(*value))
        }
        QuartzAction::PluginCall { name, payload } => {
            format!(
                "Action::PluginCall {{ name: \"{}\".to_owned(), payload: std::sync::Arc::new(\"{}\".to_owned()) }}",
                name, payload
            )
        }
        QuartzAction::RunPlugin { name, data } => {
            format!(
                "Action::PluginCall {{ name: \"{}\".to_owned(), payload: std::sync::Arc::new(\"{}\".to_owned()) }}",
                name, data
            )
        }
        QuartzAction::Expr { raw } => {
            if looks_like_rust_action_expr(raw) {
                raw.trim().to_owned()
            } else {
                format!("Action::expr({})", rust_str_lit(raw))
            }
        }
        QuartzAction::Custom { name } => {
            format!("Action::Custom {{ name: \"{}\".to_owned() }}", name)
        }
        QuartzAction::CameraFlash {
            duration_s,
            intensity,
        } => {
            let alpha = ((*intensity).clamp(0.0, 1.0) * 255.0).round() as u8;
            format!(
                "Action::CameraFlash {{ color: Color(255, 255, 255, {}), duration: {} }}",
                alpha, duration_s
            )
        }
        QuartzAction::CameraShake {
            intensity,
            duration_s,
        } => format!(
            "Action::CameraShake {{ intensity: {}, duration: {} }}",
            intensity, duration_s
        ),
        QuartzAction::CameraZoomPunch { amount, duration_s } => format!(
            "Action::CameraZoomPunch {{ amount: {}, duration: {} }}",
            amount, duration_s
        ),
        QuartzAction::SetMaterial { target, material } => {
            format!(
                "Action::SetMaterial {{ target: {}, material: PhysicsMaterial::new({}, {}, {}) }}",
                target_expr(target), material.elasticity, material.friction, material.density
            )
        }
        QuartzAction::SetDensity { target, value } => format!(
            "Action::SetDensity {{ target: {}, value: {} }}",
            target_expr(target),
            value
        ),
        QuartzAction::SetElasticity { target, value } => format!(
            "Action::SetElasticity {{ target: {}, value: {} }}",
            target_expr(target),
            value
        ),
        QuartzAction::SetFriction { target, value } => format!(
            "Action::SetFriction {{ target: {}, value: {} }}",
            target_expr(target),
            value
        ),
        QuartzAction::ApplyForce { target, fx, fy } => format!(
            "Action::ApplyForce {{ target: {}, fx: {}, fy: {} }}",
            target_expr(target),
            fx,
            fy
        ),
        QuartzAction::ApplyImpulse { target, ix, iy } => format!(
            "Action::ApplyImpulse {{ target: {}, ix: {}, iy: {} }}",
            target_expr(target),
            ix,
            iy
        ),
        QuartzAction::SetPosition { target, x, y } => format!(
            "Action::SetPosition {{ target: {}, x: {}, y: {} }}",
            target_expr(target),
            x,
            y
        ),
        QuartzAction::FreezeBody { target } => {
            format!("Action::FreezeBody {{ target: {} }}", target_expr(target))
        }
        QuartzAction::UnfreezeBody { target } => {
            format!("Action::UnfreezeBody {{ target: {} }}", target_expr(target))
        }
        QuartzAction::WakeBody { target } => {
            format!("Action::WakeBody {{ target: {} }}", target_expr(target))
        }
        QuartzAction::SetPhysicsQuality { quality } => {
            let quality_expr = match quality.trim().to_ascii_lowercase().as_str() {
                "low" => "PhysicsQuality::Low",
                "high" => "PhysicsQuality::High",
                _ => "PhysicsQuality::Medium",
            };
            format!("Action::SetPhysicsQuality {{ quality: {} }}", quality_expr)
        }
        QuartzAction::SetCollisionMode { target, mode } => {
            let mode_expr = match mode.trim().to_ascii_lowercase().as_str() {
                "sensor" => "CollisionMode::Sensor",
                "disabled" => "CollisionMode::Disabled",
                _ => "CollisionMode::Solid",
            };
            format!(
                "Action::SetCollisionMode {{ target: {}, mode: {} }}",
                target_expr(target),
                mode_expr
            )
        }
        QuartzAction::SetSlope {
            target,
            left_offset,
            right_offset,
            auto_rotate,
        } => format!(
            "Action::SetSlope {{ target: {}, left_offset: {}, right_offset: {}, auto_rotate: {} }}",
            target_expr(target),
            left_offset,
            right_offset,
            auto_rotate
        ),
        QuartzAction::SetSurfaceNormal { target, nx, ny } => format!(
            "Action::SetSurfaceNormal {{ target: {}, nx: {}, ny: {} }}",
            target_expr(target),
            nx,
            ny
        ),
        QuartzAction::TransferMomentum { from, to, scale } => format!(
            "Action::TransferMomentum {{ from: {}, to: {}, scale: {} }}",
            target_expr(from),
            target_expr(to),
            scale
        ),
        QuartzAction::SpawnEmitter { name } => format!(
            "Action::SpawnEmitter {{ emitter: EmitterBuilder::new({:?}).build() }}",
            name
        ),
        QuartzAction::RemoveEmitter { name } => format!(
            "Action::RemoveEmitter {{ name: {:?}.to_owned() }}",
            name
        ),
        QuartzAction::AttachEmitter {
            emitter_name,
            target,
            location,
        } => {
            let location_expr_opt = location
                .as_ref()
                .map(|loc| format!("Some({})", location_expr(loc)))
                .unwrap_or_else(|| "None".to_owned());
            format!(
                "Action::AttachEmitter {{ emitter_name: {:?}.to_owned(), target: {}, location: {} }}",
                emitter_name,
                target_expr(target),
                location_expr_opt
            )
        }
        QuartzAction::DetachEmitter { emitter_name } => format!(
            "Action::DetachEmitter {{ emitter_name: {:?}.to_owned() }}",
            emitter_name
        ),
        QuartzAction::SetEmitterRate { name, value } => format!(
            "Action::SetEmitterRate {{ name: {:?}.to_owned(), value: {} }}",
            name,
            value
        ),
        QuartzAction::SetEmitterLifetime { name, value } => format!(
            "Action::SetEmitterLifetime {{ name: {:?}.to_owned(), value: {} }}",
            name,
            value
        ),
        QuartzAction::SetEmitterVelocity { name, x, y } => format!(
            "Action::SetEmitterVelocity {{ name: {:?}.to_owned(), value: ({}, {}) }}",
            name,
            x,
            y
        ),
        QuartzAction::SetEmitterSpread { name, x, y } => format!(
            "Action::SetEmitterSpread {{ name: {:?}.to_owned(), value: ({}, {}) }}",
            name,
            x,
            y
        ),
        QuartzAction::SetEmitterSize { name, value } => format!(
            "Action::SetEmitterSize {{ name: {:?}.to_owned(), value: {} }}",
            name,
            value
        ),
        QuartzAction::SetEmitterColor { name, rgba } => {
            let [r, g, b, a] = *rgba;
            format!(
                "Action::SetEmitterColor {{ name: {:?}.to_owned(), value: ({}, {}, {}, {}) }}",
                name, r, g, b, a
            )
        }
        QuartzAction::SetEmitterGravityScale { name, value } => format!(
            "Action::SetEmitterGravityScale {{ name: {:?}.to_owned(), value: {} }}",
            name,
            value
        ),
        QuartzAction::SetEmitterCollision { name, mode } => {
            let mode_expr = match mode.trim().to_ascii_lowercase().as_str() {
                "die" | "kill" => "CollisionResponse::Die",
                "bounce" => "CollisionResponse::Bounce { elasticity: 0.5 }",
                _ => "CollisionResponse::None",
            };
            format!(
                "Action::SetEmitterCollision {{ name: {:?}.to_owned(), value: {} }}",
                name,
                mode_expr
            )
        }
        QuartzAction::SetEmitterRenderLayer { name, value } => format!(
            "Action::SetEmitterRenderLayer {{ name: {:?}.to_owned(), value: {} }}",
            name,
            value
        ),
        QuartzAction::SetEmitterSizeEnd { name, value } => format!(
            "Action::SetEmitterSizeEnd {{ name: {:?}.to_owned(), value: {} }}",
            name,
            value
        ),
        QuartzAction::SetEmitterColorEnd { name, rgba } => {
            let value_expr = if let Some([r, g, b, a]) = rgba {
                format!("Some(({}, {}, {}, {}))", r, g, b, a)
            } else {
                "None".to_owned()
            };
            format!(
                "Action::SetEmitterColorEnd {{ name: {:?}.to_owned(), value: {} }}",
                name,
                value_expr
            )
        }
        QuartzAction::SetEmitterShape { name, shape } => {
            let shape_expr = match shape.trim().to_ascii_lowercase().as_str() {
                "ellipse" => "ParticleShape::Ellipse { aspect_ratio: 1.5 }",
                "rect" | "rectangle" => "ParticleShape::Rect { aspect_ratio: 2.0 }",
                "soft" => "ParticleShape::Soft { roundness: 0.5 }",
                "square" => "ParticleShape::Square",
                _ => "ParticleShape::Circle",
            };
            format!(
                "Action::SetEmitterShape {{ name: {:?}.to_owned(), value: {} }}",
                name,
                shape_expr
            )
        }
        QuartzAction::SetEmitterAlignToVelocity { name, enabled } => format!(
            "Action::SetEmitterAlignToVelocity {{ name: {:?}.to_owned(), value: {} }}",
            name,
            enabled
        ),
        QuartzAction::SetEmitterInterpolatePosition { name, enabled } => format!(
            "Action::SetEmitterInterpolatePosition {{ name: {:?}.to_owned(), value: {} }}",
            name,
            enabled
        ),
        QuartzAction::AddZoom { value } => format!("Action::AddZoom {{ value: {} }}", value),
        QuartzAction::SmoothZoomAt { delta } => {
            format!("Action::SmoothZoomAt {{ delta: {} }}", delta)
        }
        QuartzAction::CameraFlashWith {
            color_rgba,
            duration_s,
            mode,
            ease,
            intensity,
            freeze_frame_s,
        } => {
            let [r, g, b, a] = *color_rgba;
            let mode_expr = match mode.trim().to_ascii_lowercase().as_str() {
                "pulse" => "FlashMode::Pulse",
                _ => "FlashMode::FadeOut",
            };
            let ease_expr = match ease.trim().to_ascii_lowercase().as_str() {
                "smooth" => "FlashEase::Smooth",
                "sharp" => "FlashEase::Sharp",
                _ => "FlashEase::Linear",
            };
            format!(
                "Action::CameraFlashWith {{ color: Color({}, {}, {}, {}), duration: {}, mode: {}, ease: {}, intensity: {}, freeze_frame: {} }}",
                r,
                g,
                b,
                a,
                duration_s,
                mode_expr,
                ease_expr,
                intensity,
                freeze_frame_s
            )
        }
        QuartzAction::SetGlow {
            target,
            color_rgb,
            width,
        } => {
            let [r, g, b] = *color_rgb;
            format!(
                "Action::SetGlow {{ target: {}, color: Color::from_rgb({}, {}, {}), width: {} }}",
                target_expr(target),
                r,
                g,
                b,
                width
            )
        }
        QuartzAction::ClearGlow { target } => {
            format!("Action::ClearGlow {{ target: {} }}", target_expr(target))
        }
        QuartzAction::SetTint { target, color_rgba } => {
            let [r, g, b, a] = *color_rgba;
            format!(
                "Action::SetTint {{ target: {}, color: Color({}, {}, {}, {}) }}",
                target_expr(target),
                r,
                g,
                b,
                a
            )
        }
        QuartzAction::ClearTint { target } => {
            format!("Action::ClearTint {{ target: {} }}", target_expr(target))
        }
        QuartzAction::EnableCrystalline => "Action::EnableCrystalline".to_owned(),
        QuartzAction::DisableCrystalline => "Action::DisableCrystalline".to_owned(),
        QuartzAction::SetGravityStrength { target, value } => format!(
            "Action::SetGravityStrength {{ target: {}, value: {} }}",
            target_expr(target),
            value
        ),
        QuartzAction::SetPlanetRadius { target, value } => format!(
            "Action::SetPlanetRadius {{ target: {}, value: {} }}",
            target_expr(target),
            value
        ),
        QuartzAction::SetGravityTarget { target, tag } => format!(
            "Action::SetGravityTarget {{ target: {}, tag: {:?}.to_owned() }}",
            target_expr(target),
            tag
        ),
        QuartzAction::SetGravityInfluenceMult { target, value } => format!(
            "Action::SetGravityInfluenceMult {{ target: {}, value: {} }}",
            target_expr(target),
            value
        ),
        QuartzAction::SetGravityFalloff { target, falloff } => {
            let falloff_expr = match falloff.trim().to_ascii_lowercase().as_str() {
                "inversesquare" | "inverse_square" | "inverse-square" => "GravityFalloff::InverseSquare",
                _ => "GravityFalloff::Linear",
            };
            format!(
                "Action::SetGravityFalloff {{ target: {}, falloff: {} }}",
                target_expr(target),
                falloff_expr
            )
        }
        QuartzAction::SetGravityAllSources { target, enabled } => format!(
            "Action::SetGravityAllSources {{ target: {}, enabled: {} }}",
            target_expr(target),
            enabled
        ),
        QuartzAction::SetAlignToSlope { target, enabled } => format!(
            "Action::SetAlignToSlope {{ target: {}, enabled: {} }}",
            target_expr(target),
            enabled
        ),
        QuartzAction::SetAlignToSlopeSpeed { target, value } => format!(
            "Action::SetAlignToSlopeSpeed {{ target: {}, value: {} }}",
            target_expr(target),
            value
        ),
        QuartzAction::SetVar { name, value } => format!(
            "Action::SetVar {{ name: {:?}.to_owned(), value: {} }}",
            name,
            quartz_expr_to_code(value)
        ),
        QuartzAction::ModVar { name, op, operand } => format!(
            "Action::ModVar {{ name: {:?}.to_owned(), op: {}, operand: {} }}",
            name,
            quartz_math_op_code(op),
            quartz_expr_to_code(operand)
        ),
        QuartzAction::Spawn { template_id, location }
        | QuartzAction::SpawnObject { template_id, location } => format!(
            "Action::Spawn {{ object: Box::new(spawn_{}(canvas)), location: {} }}",
            template_id,
            location_expr(location)
        ),
        QuartzAction::SetText {
            target,
            content,
            font_size,
            color_rgb,
            font_asset_path,
        } => {
            format!(
                "Action::SetText {{ target: {}, text: {} }}",
                target_expr(target),
                set_text_value_expr(content, *font_size, color_rgb, font_asset_path)
            )
        }
        QuartzAction::Conditional {
            condition,
            if_true,
            if_false,
        } => {
            let if_true_expr = action_expr_inner(if_true);
            if let Some(if_false) = if_false {
                format!(
                    "Action::Conditional {{ condition: {}, if_true: Box::new({}), if_false: Some(Box::new({})) }}",
                    condition_expr(condition),
                    if_true_expr,
                    action_expr_inner(if_false)
                )
            } else {
                format!(
                    "Action::Conditional {{ condition: {}, if_true: Box::new({}), if_false: None }}",
                    condition_expr(condition),
                    if_true_expr
                )
            }
        }
        QuartzAction::Multi { actions } => {
            let parts = actions.iter().map(|a| action_expr_inner(a)).collect::<Vec<_>>().join(", ");
            format!("Action::Multi(vec![{}])", parts)
        }
    }
}

fn quartz_expr_to_code(expr: &QuartzExpr) -> String {
    match expr.kind {
        QuartzExprKind::F32  => format!("Expr::f32({})", expr.raw),
        QuartzExprKind::I32  => format!("Expr::i32({})", expr.raw),
        QuartzExprKind::Bool => format!("Expr::bool({})", expr.raw),
        QuartzExprKind::Str  => format!("Expr::str({:?})", expr.raw),
        QuartzExprKind::Var  => format!("Expr::var({:?})", expr.raw),
    }
}

fn quartz_math_op_code(op: &QuartzMathOp) -> &'static str {
    match op {
        QuartzMathOp::Add => "MathOp::Add",
        QuartzMathOp::Sub => "MathOp::Sub",
        QuartzMathOp::Mul => "MathOp::Mul",
        QuartzMathOp::Div => "MathOp::Div",
    }
}

fn set_text_font_expr(font_asset_path: &str) -> String {
    if let Some(bytes_expr) = asset_include_expr(font_asset_path) {
        format!(
            "{{ static QF_SET_TEXT_FONT: std::sync::OnceLock<Font> = std::sync::OnceLock::new(); std::sync::Arc::new(QF_SET_TEXT_FONT.get_or_init(|| Font::from_bytes({bytes_expr}).expect(\"failed to load SetText font\")).clone()) }}"
        )
    } else {
        "std::sync::Arc::new(Font::default())".to_owned()
    }
}

fn set_text_value_expr(
    content: &str,
    font_size: f32,
    color_rgb: &[u8; 3],
    font_asset_path: &str,
) -> String {
    let [r, g, b] = *color_rgb;
    let line_height = font_size * 1.25;
    let font_expr = set_text_font_expr(font_asset_path);
    format!(
        "{{ let font = {font_expr}; Text::new(vec![Span::new({content:?}.to_owned(), {font_size}f32, Some({line_height}f32), font, Color::from_rgb({r}, {g}, {b}), 0.0)], None, Align::Left, None) }}"
    )
}

/// Returns an action expression while preserving the recursive plumbing used by emit_action_lines.
/// SetText now emits direct Text::new/Span::new expressions, so no prelude is needed.
fn action_expr_with_prelude(
    action: &QuartzAction,
    prelude: &mut Vec<String>,
    counter: &mut usize,
) -> String {
    match action {
        QuartzAction::Multi { actions } => {
            let parts = actions
                .iter()
                .map(|a| action_expr_with_prelude(a, prelude, counter))
                .collect::<Vec<_>>()
                .join(", ");
            format!("Action::Multi(vec![{parts}])")
        }
        QuartzAction::Conditional { condition, if_true, if_false } => {
            let if_true_expr = action_expr_with_prelude(if_true, prelude, counter);
            if let Some(if_false) = if_false {
                let if_false_expr = action_expr_with_prelude(if_false, prelude, counter);
                format!(
                    "Action::Conditional {{ condition: {}, if_true: Box::new({if_true_expr}), if_false: Some(Box::new({if_false_expr})) }}",
                    condition_expr(condition)
                )
            } else {
                format!(
                    "Action::Conditional {{ condition: {}, if_true: Box::new({if_true_expr}), if_false: None }}",
                    condition_expr(condition)
                )
            }
        }
        other => action_expr_inner(other),
    }
}

/// Emit the canonical statements for a nodes list inside an on_update/event handler body.
/// Returns lines to push at the call-site indentation level.
pub fn emit_action_lines(nodes: &[LogicNode], indent: &str) -> Vec<String> {
    let mut prelude: Vec<String> = Vec::new();
    let mut counter = 0usize;
    let root_action = nodes_to_action_expr_rec(nodes, &mut prelude, &mut counter);
    let mut lines: Vec<String> = prelude.iter().map(|s| format!("{indent}{s}")).collect();
    lines.push(format!("{indent}canvas.run({root_action});"));
    lines
}

pub fn api_first_static_guard_violations(source: &str) -> Vec<String> {
    let mut violations = Vec::new();

    if source.contains("Arc<Mutex<") || source.contains("Arc < Mutex <") {
        violations.push(
            "Arc<Mutex<...>> scalar state is forbidden; use canvas game_vars via Action::SetVar/ModVar"
                .to_owned(),
        );
    }

    if source.contains("Action::SetPosition") && !source.contains("QF_ALLOW_SETPOSITION_STATIC") {
        violations.push(
            "Action::SetPosition detected without explicit guard marker; use Teleport/SetMomentum/ApplyMomentum for movement"
                .to_owned(),
        );
    }

    for on_update_body in source.split("on_update(|").skip(1) {
        let direct_plugin_patterns = [
            "get_plugin::<",
            "get_plugin_mut::<",
            ".on_action(",
            ".on_call(",
            ".on_condition(",
            ".on_post_update(",
            ".on_post_solve(",
            ".on_init(",
            ".on_update(",
        ];

        if direct_plugin_patterns
            .iter()
            .any(|pattern| on_update_body.contains(pattern))
        {
            violations.push(
                "Direct plugin interaction detected inside on_update; dispatch through Action::PluginCall"
                    .to_owned(),
            );
            break;
        }
    }

    violations
}

fn nodes_to_action_expr_rec(
    nodes: &[LogicNode],
    prelude: &mut Vec<String>,
    counter: &mut usize,
) -> String {
    match nodes {
        [] => "Action::Multi(vec![])".to_owned(),
        [single] => node_to_action_expr_rec(single, prelude, counter),
        many => {
            let parts = many
                .iter()
                .map(|n| node_to_action_expr_rec(n, prelude, counter))
                .collect::<Vec<_>>()
                .join(", ");
            format!("Action::Multi(vec![{parts}])")
        }
    }
}

fn node_to_action_expr_rec(
    node: &LogicNode,
    prelude: &mut Vec<String>,
    counter: &mut usize,
) -> String {
    match node {
        LogicNode::Action(a) => action_expr_with_prelude(a, prelude, counter),
        LogicNode::Branch { condition, then_nodes, else_nodes } => {
            let if_true_expr = nodes_to_action_expr_rec(then_nodes, prelude, counter);
            if !else_nodes.is_empty() {
                let if_false_expr = nodes_to_action_expr_rec(else_nodes, prelude, counter);
                format!(
                    "Action::Conditional {{ condition: {}, if_true: Box::new({if_true_expr}), if_false: Some(Box::new({if_false_expr})) }}",
                    condition_expr(condition)
                )
            } else {
                format!(
                    "Action::Conditional {{ condition: {}, if_true: Box::new({if_true_expr}), if_false: None }}",
                    condition_expr(condition)
                )
            }
        }
    }
}

fn condition_expr(condition: &QuartzCondition) -> String {
    match condition {
        QuartzCondition::Always => "Condition::Always".to_owned(),
        QuartzCondition::KeyHeld { key } => {
            format!("Condition::KeyHeld({})", key_expr(key))
        }
        QuartzCondition::KeyNotHeld { key } => {
            format!("Condition::KeyNotHeld({})", key_expr(key))
        }
        QuartzCondition::Collision { target } => {
            format!("Condition::Collision({})", target_expr(target))
        }
        QuartzCondition::NoCollision { target } => {
            format!("Condition::NoCollision({})", target_expr(target))
        }
        QuartzCondition::CollisionWith { object_a, object_b } => {
            format!(
                "Condition::And(Box::new(Condition::Collision(Target::name(\"{}\"))), Box::new(Condition::Collision(Target::name(\"{}\"))))",
                object_a, object_b
            )
        }
        QuartzCondition::VarCompare {
            variable,
            op,
            value,
        } => format!(
            "Condition::Compare(Expr::var(\"{}\"), {}, Expr::f32({}))",
            variable,
            comp_op_expr(*op),
            value
        ),
        QuartzCondition::Compare { left, op, right } => format!(
            "Condition::Compare({}, {}, {})",
            quartz_expr_to_code(left),
            comp_op_expr(*op),
            quartz_expr_to_code(right)
        ),
        QuartzCondition::VarExists { variable } => {
            format!("Condition::VarExists(\"{}\".to_owned())", variable)
        }
        QuartzCondition::Expr { raw } => {
            if looks_like_rust_condition_expr(raw) {
                raw.trim().to_owned()
            } else {
                format!("Condition::expr({})", rust_str_lit(raw))
            }
        }
        QuartzCondition::And { left, right } => {
            format!(
                "Condition::And(Box::new({}), Box::new({}))",
                condition_expr(left),
                condition_expr(right)
            )
        }
        QuartzCondition::Or { left, right } => {
            format!(
                "Condition::Or(Box::new({}), Box::new({}))",
                condition_expr(left),
                condition_expr(right)
            )
        }
        QuartzCondition::Not { inner } => {
            format!("Condition::Not(Box::new({}))", condition_expr(inner))
        }
        QuartzCondition::IsVisible { target } => {
            format!("Condition::IsVisible({})", target_expr(target))
        }
        QuartzCondition::IsHidden { target } => {
            format!("Condition::IsHidden({})", target_expr(target))
        }
        QuartzCondition::IsMoving { target } => {
            format!("Condition::IsMoving({})", target_expr(target))
        }
        QuartzCondition::Grounded { target } => {
            format!("Condition::Grounded({})", target_expr(target))
        }
        QuartzCondition::HasTag { target, tag } => {
            format!("Condition::HasTag({}, \"{}\".to_owned())", target_expr(target), tag)
        }
        QuartzCondition::IsSleeping { target } => {
            format!("Condition::IsSleeping({})", target_expr(target))
        }
        QuartzCondition::IsRotating { target } => {
            format!("Condition::IsRotating({})", target_expr(target))
        }
        QuartzCondition::IsStill { target } => {
            format!("Condition::IsStill({})", target_expr(target))
        }
        QuartzCondition::SpeedAbove { target, value } => {
            format!("Condition::SpeedAbove({}, {})", target_expr(target), value)
        }
        QuartzCondition::SpeedBelow { target, value } => {
            format!("Condition::SpeedBelow({}, {})", target_expr(target), value)
        }
        QuartzCondition::CrystallineEnabled => "Condition::CrystallineEnabled".to_owned(),
        QuartzCondition::EmitterActive { emitter } => {
            format!("Condition::EmitterActive(\"{}\".to_owned())", emitter)
        }
        QuartzCondition::OnPlanet { target, planet } => {
            format!(
                "Condition::OnPlanet({}, {})",
                target_expr(target),
                target_expr(planet)
            )
        }
        QuartzCondition::InGravityField { target, planet } => {
            format!(
                "Condition::InGravityField({}, {})",
                target_expr(target),
                target_expr(planet)
            )
        }
        QuartzCondition::HasDominantPlanet { target } => {
            format!("Condition::HasDominantPlanet({})", target_expr(target))
        }
        QuartzCondition::DominantPlanetIs { target, planet } => {
            format!(
                "Condition::DominantPlanetIs({}, {})",
                target_expr(target),
                target_expr(planet)
            )
        }
        QuartzCondition::InAnyGravityField { target } => {
            format!("Condition::InAnyGravityField({})", target_expr(target))
        }
        QuartzCondition::Plugin { name, arg } => {
            if let Some(arg) = arg {
                format!(
                    "Condition::Plugin {{ name: \"{}\".to_owned(), arg: Some(\"{}\".to_owned()) }}",
                    name, arg
                )
            } else {
                format!(
                    "Condition::Plugin {{ name: \"{}\".to_owned(), arg: None }}",
                    name
                )
            }
        }
    }
}

fn write_event_binding(
    out: &mut String,
    binding: &QuartzEventBinding,
    logic_trees: &[crate::core::quartz_domain::LogicTree],
    depth: usize,
) {
    let indent = "    ".repeat(depth);
    let outer_target = target_expr(&binding.listener_target);
    let action_target = target_expr(&binding.action_target);
    let selected_tree_action = binding
        .linked_logic_tree_id
        .as_ref()
        .and_then(|tree_id| logic_trees.iter().find(|t| &t.id == tree_id))
        .map(|tree| nodes_to_action_expr(&tree.nodes));
    let fallback = event_action_expr(binding);
    let effective_action = selected_tree_action.unwrap_or(fallback);
    let event_expr = event_expr(binding, &action_target, &effective_action);
    out.push_str(&format!("{}canvas.add_event({}, {});\n", indent, event_expr, outer_target));
    if let Some(tree_id) = &binding.linked_logic_tree_id {
        out.push_str(&format!(
            "{}// linked_update_script_id: {}\n",
            indent, tree_id
        ));
    }
}

fn event_expr(binding: &QuartzEventBinding, action_target_expr: &str, action_expr_value: &str) -> String {
    match &binding.kind {
        QuartzEventKind::Collision => {
            format!(
                "GameEvent::Collision {{ action: {}, target: {} }}",
                action_expr_value,
                action_target_expr
            )
        }
        QuartzEventKind::BoundaryCollision => {
            format!(
                "GameEvent::BoundaryCollision {{ action: {}, target: {} }}",
                action_expr_value,
                action_target_expr
            )
        }
        QuartzEventKind::KeyPress { key, modifiers } => {
            format!(
                "GameEvent::KeyPress {{ key: {}, action: {}, target: {}, modifiers: {} }}",
                key_expr(key),
                action_expr_value,
                action_target_expr,
                modifiers_expr(modifiers)
            )
        }
        QuartzEventKind::KeyRelease { key, modifiers } => {
            format!(
                "GameEvent::KeyRelease {{ key: {}, action: {}, target: {}, modifiers: {} }}",
                key_expr(key),
                action_expr_value,
                action_target_expr,
                modifiers_expr(modifiers)
            )
        }
        QuartzEventKind::KeyHold { key, modifiers } => {
            format!(
                "GameEvent::KeyHold {{ key: {}, action: {}, target: {}, modifiers: {} }}",
                key_expr(key),
                action_expr_value,
                action_target_expr,
                modifiers_expr(modifiers)
            )
        }
        QuartzEventKind::Tick => {
            format!(
                "GameEvent::Tick {{ action: {}, target: {} }}",
                action_expr_value,
                action_target_expr
            )
        }
        QuartzEventKind::Custom { name } => {
            format!(
                "GameEvent::Custom {{ name: \"{}\".to_owned(), target: {} }}",
                name, action_target_expr
            )
        }
        QuartzEventKind::MousePress { button } => {
            format!(
                "GameEvent::MousePress {{ action: {}, target: {}, button: {} }}",
                action_expr_value,
                action_target_expr,
                mouse_button_expr(*button)
            )
        }
        QuartzEventKind::MouseRelease { button } => {
            format!(
                "GameEvent::MouseRelease {{ action: {}, target: {}, button: {} }}",
                action_expr_value,
                action_target_expr,
                mouse_button_expr(*button)
            )
        }
        QuartzEventKind::MouseEnter => {
            format!(
                "GameEvent::MouseEnter {{ action: {}, target: {} }}",
                action_expr_value,
                action_target_expr
            )
        }
        QuartzEventKind::MouseLeave => {
            format!(
                "GameEvent::MouseLeave {{ action: {}, target: {} }}",
                action_expr_value,
                action_target_expr
            )
        }
        QuartzEventKind::MouseOver => {
            format!(
                "GameEvent::MouseOver {{ action: {}, target: {} }}",
                action_expr_value,
                action_target_expr
            )
        }
        QuartzEventKind::MouseScroll { axis } => {
            format!(
                "GameEvent::MouseScroll {{ action: {}, target: {}, axis: {} }}",
                action_expr_value,
                action_target_expr,
                scroll_axis_expr(*axis)
            )
        }
        QuartzEventKind::MouseMove => {
            format!(
                "GameEvent::MouseMove {{ action: {}, target: {} }}",
                action_expr_value,
                action_target_expr
            )
        }
    }
}

fn event_action_expr(binding: &QuartzEventBinding) -> String {
    if let Some(action) = &binding.action {
        action_expr_inner(action)
    } else {
        "Action::Custom { name: \"event_action\".to_owned() }".to_owned()
    }
}

fn asset_include_expr(path: &str) -> Option<String> {
    let normalized = path
        .trim()
        .trim_start_matches("./")
        .trim_start_matches('/');
    if normalized.is_empty() {
        return None;
    }
    let escaped = normalized.replace('\\', "/").replace('"', "\\\"");
    Some(format!(
        "include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{}\"))",
        escaped
    ))
}

fn object_asset_cache_key(object: &crate::core::quartz_domain::QuartzObjectBlueprint) -> String {
    let candidate = object.visual_asset_cache_key.trim();
    if candidate.is_empty() {
        if object.visual_asset_path.trim().is_empty() {
            object.id.clone()
        } else {
            object.visual_asset_path.replace('\\', "/")
        }
    } else {
        candidate.to_owned()
    }
}

fn static_image_expr(object: &crate::core::quartz_domain::QuartzObjectBlueprint) -> Option<String> {
    let bytes_expr = asset_include_expr(&object.visual_asset_path)?;
    if object.visual_asset_use_canvas_cache {
        let key = object_asset_cache_key(object).replace('"', "\\\"");
        if object.visual_asset_size_aware_cache {
            Some(format!(
                "canvas.load_image_sized_cached(\"{}\", {}, {}, {})",
                key, bytes_expr, object.w, object.h
            ))
        } else {
            Some(format!("canvas.load_image_cached(\"{}\", {})", key, bytes_expr))
        }
    } else {
        Some(format!("quartz::sprite::load_image({})", bytes_expr))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        action_expr_inner, api_first_static_guard_violations, condition_expr,
        object_registration_body,
    };
    use crate::core::quartz_domain::{
        CompareOp, ObjectPhysicsMaterialPreset, ObjectPhysicsMaterialSpec, QuartzAction,
        QuartzCondition, QuartzExpr, QuartzExprKind, QuartzLocationRef, QuartzTargetRef,
    };

    #[test]
    fn codegen_rejects_arc_mutex_scalar_state_patterns() {
        let snippet = "use std::sync::{Arc, Mutex}; let _state = Arc<Mutex<i32>>;";
        let violations = api_first_static_guard_violations(snippet);
        assert!(violations
            .iter()
            .any(|v| v.contains("Arc<Mutex<...>> scalar state is forbidden")));
    }

    #[test]
    fn codegen_flags_setposition_for_movement_intent() {
        let snippet = "canvas.run(Action::SetPosition { target: Target::name(\"player\"), x: 12.0, y: 4.0 });";
        let violations = api_first_static_guard_violations(snippet);
        assert!(violations
            .iter()
            .any(|v| v.contains("Action::SetPosition detected without explicit guard marker")));
    }

    #[test]
    fn setposition_used_as_movement_intent_is_flagged() {
        let snippet = "canvas.run(Action::SetPosition { target: Target::name(\"player\"), x: 24.0, y: 10.0 });";
        let violations = api_first_static_guard_violations(snippet);
        assert!(violations
            .iter()
            .any(|v| v.contains("Action::SetPosition detected without explicit guard marker")));
    }

    #[test]
    fn codegen_flags_direct_plugin_call_in_update() {
        let snippet = "canvas.on_update(|canvas| { if let Some(plugin) = canvas.get_plugin::<MyPlugin>() { plugin.on_update(canvas); } });";
        let violations = api_first_static_guard_violations(snippet);
        assert!(violations
            .iter()
            .any(|v| v.contains("Direct plugin interaction detected inside on_update")));
    }

    #[test]
    fn codegen_guard_accepts_api_first_pattern() {
        let snippet = "canvas.add_event(GameEvent::Tick { action: Action::ModVar { name: \"score\".to_owned(), op: MathOp::Add, operand: Expr::i32(1) }, target: Target::name(\"player\") }, Target::name(\"player\"));";
        let violations = api_first_static_guard_violations(snippet);
        assert!(violations.is_empty());
    }

    #[test]
    fn generated_code_emits_new_condition_variants() {
        let compare = QuartzCondition::Compare {
            left: QuartzExpr {
                kind: QuartzExprKind::Var,
                raw: "score".to_owned(),
            },
            op: CompareOp::Ge,
            right: QuartzExpr {
                kind: QuartzExprKind::I32,
                raw: "10".to_owned(),
            },
        };
        let compare_code = condition_expr(&compare);
        assert!(compare_code.contains("Condition::Compare("));
        assert!(compare_code.contains("Expr::var(\"score\")"));

        let planetary = QuartzCondition::DominantPlanetIs {
            target: QuartzTargetRef::Name("player".to_owned()),
            planet: QuartzTargetRef::Tag("planet".to_owned()),
        };
        let planetary_code = condition_expr(&planetary);
        assert!(planetary_code.contains("Condition::DominantPlanetIs("));
        assert!(planetary_code.contains("Target::name(\"player\")"));
        assert!(planetary_code.contains("Target::tag(\"planet\")"));
    }

    #[test]
    fn codegen_emits_apply_force_and_impulse() {
        let force = QuartzAction::ApplyForce {
            target: QuartzTargetRef::Name("player".to_owned()),
            fx: 12.5,
            fy: -3.0,
        };
        let impulse = QuartzAction::ApplyImpulse {
            target: QuartzTargetRef::Tag("enemy".to_owned()),
            ix: 2.0,
            iy: -8.0,
        };

        let force_code = action_expr_inner(&force);
        let impulse_code = action_expr_inner(&impulse);

        assert!(force_code.contains("Action::ApplyForce"));
        assert!(force_code.contains("Target::name(\"player\")"));
        assert!(force_code.contains("fx: 12.5"));
        assert!(force_code.contains("fy: -3"));

        assert!(impulse_code.contains("Action::ApplyImpulse"));
        assert!(impulse_code.contains("Target::tag(\"enemy\")"));
        assert!(impulse_code.contains("ix: 2"));
        assert!(impulse_code.contains("iy: -8"));
    }

    #[test]
    fn codegen_emits_material_property_actions() {
        let set_material = QuartzAction::SetMaterial {
            target: QuartzTargetRef::Name("crate".to_owned()),
            material: ObjectPhysicsMaterialSpec::resolved_defaults(ObjectPhysicsMaterialPreset::Rubber),
        };
        let set_density = QuartzAction::SetDensity {
            target: QuartzTargetRef::Name("crate".to_owned()),
            value: 1.25,
        };
        let set_elasticity = QuartzAction::SetElasticity {
            target: QuartzTargetRef::Name("crate".to_owned()),
            value: 0.9,
        };
        let set_friction = QuartzAction::SetFriction {
            target: QuartzTargetRef::Name("crate".to_owned()),
            value: 0.2,
        };

        let material_code = action_expr_inner(&set_material);
        let density_code = action_expr_inner(&set_density);
        let elasticity_code = action_expr_inner(&set_elasticity);
        let friction_code = action_expr_inner(&set_friction);

        assert!(material_code.contains("Action::SetMaterial"));
        assert!(material_code.contains("PhysicsMaterial::new("));
        assert!(density_code.contains("Action::SetDensity"));
        assert!(density_code.contains("value: 1.25"));
        assert!(elasticity_code.contains("Action::SetElasticity"));
        assert!(elasticity_code.contains("value: 0.9"));
        assert!(friction_code.contains("Action::SetFriction"));
        assert!(friction_code.contains("value: 0.2"));
    }

    #[test]
    fn codegen_emits_body_state_actions() {
        let freeze = QuartzAction::FreezeBody {
            target: QuartzTargetRef::Name("player".to_owned()),
        };
        let unfreeze = QuartzAction::UnfreezeBody {
            target: QuartzTargetRef::Name("player".to_owned()),
        };
        let wake = QuartzAction::WakeBody {
            target: QuartzTargetRef::Name("player".to_owned()),
        };

        let freeze_code = action_expr_inner(&freeze);
        let unfreeze_code = action_expr_inner(&unfreeze);
        let wake_code = action_expr_inner(&wake);

        assert!(freeze_code.contains("Action::FreezeBody"));
        assert!(freeze_code.contains("Target::name(\"player\")"));
        assert!(unfreeze_code.contains("Action::UnfreezeBody"));
        assert!(wake_code.contains("Action::WakeBody"));
    }

    #[test]
    fn codegen_emits_emitter_action_variants() {
        let spawn = QuartzAction::SpawnEmitter {
            name: "trail".to_owned(),
        };
        let attach = QuartzAction::AttachEmitter {
            emitter_name: "trail".to_owned(),
            target: QuartzTargetRef::Name("player".to_owned()),
            location: Some(QuartzLocationRef::AtTarget(QuartzTargetRef::Name("player".to_owned()))),
        };
        let collision = QuartzAction::SetEmitterCollision {
            name: "trail".to_owned(),
            mode: "Bounce".to_owned(),
        };
        let shape = QuartzAction::SetEmitterShape {
            name: "trail".to_owned(),
            shape: "Ellipse".to_owned(),
        };
        let flash = QuartzAction::CameraFlashWith {
            color_rgba: [255, 255, 255, 255],
            duration_s: 0.2,
            mode: "Pulse".to_owned(),
            ease: "Smooth".to_owned(),
            intensity: 0.8,
            freeze_frame_s: 0.05,
        };

        let spawn_code = action_expr_inner(&spawn);
        let attach_code = action_expr_inner(&attach);
        let collision_code = action_expr_inner(&collision);
        let shape_code = action_expr_inner(&shape);
        let flash_code = action_expr_inner(&flash);

        assert!(spawn_code.contains("Action::SpawnEmitter"));
        assert!(spawn_code.contains("EmitterBuilder::new(\"trail\")"));
        assert!(attach_code.contains("Action::AttachEmitter"));
        assert!(attach_code.contains("Some(Location::at_target(Target::name(\"player\")))"));
        assert!(collision_code.contains("Action::SetEmitterCollision"));
        assert!(collision_code.contains("CollisionResponse::Bounce { elasticity: 0.5 }"));
        assert!(shape_code.contains("ParticleShape::Ellipse"));
        assert!(flash_code.contains("Action::CameraFlashWith"));
        assert!(flash_code.contains("mode: FlashMode::Pulse"));
        assert!(flash_code.contains("ease: FlashEase::Smooth"));
    }

    #[test]
    fn codegen_emits_gravity_planet_action_variants() {
        let enable = QuartzAction::EnableCrystalline;
        let gravity_strength = QuartzAction::SetGravityStrength {
            target: QuartzTargetRef::Name("player".to_owned()),
            value: 9.8,
        };
        let gravity_falloff = QuartzAction::SetGravityFalloff {
            target: QuartzTargetRef::Name("player".to_owned()),
            falloff: "InverseSquare".to_owned(),
        };
        let align = QuartzAction::SetAlignToSlope {
            target: QuartzTargetRef::Name("player".to_owned()),
            enabled: true,
        };

        let enable_code = action_expr_inner(&enable);
        let gravity_strength_code = action_expr_inner(&gravity_strength);
        let gravity_falloff_code = action_expr_inner(&gravity_falloff);
        let align_code = action_expr_inner(&align);

        assert_eq!(enable_code, "Action::EnableCrystalline");
        assert!(gravity_strength_code.contains("Action::SetGravityStrength"));
        assert!(gravity_strength_code.contains("value: 9.8"));
        assert!(gravity_falloff_code.contains("Action::SetGravityFalloff"));
        assert!(gravity_falloff_code.contains("GravityFalloff::InverseSquare"));
        assert!(align_code.contains("Action::SetAlignToSlope"));
        assert!(align_code.contains("enabled: true"));
    }

    #[test]
    fn codegen_emits_plugincall_and_spawn() {
        let plugin_call = QuartzAction::PluginCall {
            name: "terrain_collision".to_owned(),
            payload: "refresh".to_owned(),
        };
        let legacy_plugin_call = QuartzAction::RunPlugin {
            name: "terrain_collision".to_owned(),
            data: "refresh".to_owned(),
        };
        let spawn = QuartzAction::Spawn {
            template_id: "enemy".to_owned(),
            location: QuartzLocationRef::At { x: 320.0, y: 180.0 },
        };

        let plugin_code = action_expr_inner(&plugin_call);
        let legacy_plugin_code = action_expr_inner(&legacy_plugin_call);
        let spawn_code = action_expr_inner(&spawn);

        assert!(plugin_code.contains("Action::PluginCall"));
        assert!(plugin_code.contains("payload: std::sync::Arc::new(\"refresh\".to_owned())"));
        assert!(legacy_plugin_code.contains("Action::PluginCall"));

        assert!(spawn_code.contains("Action::Spawn"));
        assert!(spawn_code.contains("Box::new(spawn_enemy(canvas))"));
    }

    #[test]
    fn codegen_emits_rust_typed_expr_raw_directly() {
        let action = QuartzAction::Expr {
            raw: "Action :: Custom { name : \"quoted\" . into () , }".to_owned(),
        };
        let condition = QuartzCondition::Expr {
            raw: "Condition :: VarExists (\"game_over_true\" . into ())".to_owned(),
        };

        let action_code = action_expr_inner(&action);
        let condition_code = condition_expr(&condition);

        assert_eq!(action_code, "Action :: Custom { name : \"quoted\" . into () , }");
        assert_eq!(
            condition_code,
            "Condition :: VarExists (\"game_over_true\" . into ())"
        );
    }

    #[test]
    fn codegen_escapes_parser_expr_strings_for_runtime_expr_api() {
        let action = QuartzAction::Expr {
            raw: "score > 10".to_owned(),
        };
        let condition = QuartzCondition::Expr {
            raw: "lives <= 0".to_owned(),
        };

        let action_code = action_expr_inner(&action);
        let condition_code = condition_expr(&condition);

        assert!(action_code.contains("Action::expr("));
        assert!(action_code.contains("score > 10"));
        assert!(condition_code.contains("Condition::expr("));
        assert!(condition_code.contains("lives <= 0"));
    }

    #[test]
    fn codegen_does_not_suffix_identifier_numeric_exprs() {
        let action = QuartzAction::ModVar {
            name: "player_angle".to_owned(),
            op: crate::core::quartz_domain::QuartzMathOp::Add,
            operand: QuartzExpr {
                kind: QuartzExprKind::F32,
                raw: "PLAYER_TURN_SPEED".to_owned(),
            },
        };

        let code = action_expr_inner(&action);
        assert!(code.contains("Expr::f32(PLAYER_TURN_SPEED)"));
        assert!(!code.contains("PLAYER_TURN_SPEEDf32"));
    }

    #[test]
    fn codegen_emits_rotation_values_as_float_literals() {
        let add_rotation = QuartzAction::AddRotation {
            target: crate::core::quartz_domain::QuartzTargetRef::Name("player".to_owned()),
            deg: 3.0,
        };
        let apply_rotation = QuartzAction::ApplyRotation {
            target: crate::core::quartz_domain::QuartzTargetRef::Name("player".to_owned()),
            deg: -3.0,
        };

        let add_code = action_expr_inner(&add_rotation);
        let apply_code = action_expr_inner(&apply_rotation);

        assert!(add_code.contains("value: 3.0"));
        assert!(apply_code.contains("value: -3.0"));
    }

    #[test]
    fn object_codegen_uses_finish_and_float_literals() {
        use crate::core::quartz_domain::QuartzObjectBlueprint;

        let mut object = QuartzObjectBlueprint::new("obj".to_owned(), "obj".to_owned());
        object.w = 48.0;
        object.h = 48.0;
        object.x = 120.0;
        object.y = 60.0;
        object.advanced.gravity = 0.0;
        object.advanced.rotation_deg = 0.0;
        object.advanced.rotation_resistance = 0.0;
        object.advanced.gravity_strength = 1.0;
        object.advanced.gravity_influence_mult = 3.0;

        let code = object_registration_body(&object);
        assert!(code.contains(".finish();"));
        assert!(code.contains(".size(48.0, 48.0)"));
        assert!(code.contains(".position(120.0, 60.0)"));
        assert!(code.contains(".gravity(0.0)"));
    }

    #[test]
    fn object_codegen_emits_player_circle_drawable_when_no_visual_asset() {
        use crate::core::quartz_domain::QuartzObjectBlueprint;

        let mut object = QuartzObjectBlueprint::new("player".to_owned(), "player".to_owned());
        object.w = 48.0;
        object.h = 48.0;
        object.tags.push("player".to_owned());
        object.advanced.collision_mode =
            crate::core::quartz_domain::QuartzObjectCollisionMode::SolidCircle;

        let code = object_registration_body(&object);
        assert!(code.contains("set_drawable(Box::new(quartz::sprite::solid_circle"));
    }

    #[test]
    fn object_codegen_emits_is_platform_false_for_non_auto_collision_mode() {
        use crate::core::quartz_domain::{QuartzObjectBlueprint, QuartzObjectCollisionMode};

        let mut object = QuartzObjectBlueprint::new("player".to_owned(), "player".to_owned());
        object.advanced.collision_mode = QuartzObjectCollisionMode::SolidCircle;
        object.advanced.is_platform = false;

        let code = object_registration_body(&object);
        assert!(code.contains("player.is_platform = false;"));
    }
}

