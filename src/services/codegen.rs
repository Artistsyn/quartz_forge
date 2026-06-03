use crate::core::project::EditorProjectState;
use crate::core::quartz_domain::{
    LogicNode, ObjectVisualAssetMode, QuartzAction, QuartzCondition, QuartzEventBinding,
    QuartzEventKind,
};
use crate::services::codegen_text::{
    comp_op_expr, key_expr, location_expr, modifiers_expr, mouse_button_expr,
    physics_material_expr, scroll_axis_expr, target_expr,
};

pub fn generate_quartz_preview(state: &EditorProjectState) -> String {
    let Some(scene) = state.manifest.scenes.get(state.active_scene_index) else {
        return "// no active scene".to_owned();
    };

    let mut out = String::new();
    out.push_str("use quartz::prelude::*;\n\n");
    out.push_str("pub fn setup_scene(canvas: &mut Canvas) {\n");

    for obj in &scene.objects {
        if !obj.enabled {
            continue;
        }
        out.push_str(&format!("    let mut {} = GameObject::build(\"{}\")\n", obj.id, obj.id));
        out.push_str(&format!("        .size({}, {})\n", obj.w, obj.h));
        out.push_str(&format!("        .position({}, {})\n", obj.x, obj.y));
        out.push_str(&format!("        .layer({})\n", obj.layer));
        out.push_str(&format!(
            "        .momentum({}, {})\n",
            obj.advanced.momentum_x, obj.advanced.momentum_y
        ));
        out.push_str(&format!(
            "        .resistance({}, {})\n",
            obj.advanced.resistance_x, obj.advanced.resistance_y
        ));
        out.push_str(&format!("        .gravity({})\n", obj.advanced.gravity));
        out.push_str(&format!("        .rotation({})\n", obj.advanced.rotation_deg));
        out.push_str(&format!(
            "        .pivot({}, {})\n",
            obj.advanced.pivot_x, obj.advanced.pivot_y
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
        if obj.visual_asset_mode == ObjectVisualAssetMode::StaticImage {
            if let Some(bytes_expr) = asset_include_expr(&obj.visual_asset_path) {
                out.push_str(&format!(
                    "        .image(quartz::sprite::load_image({}))\n",
                    bytes_expr
                ));
            }
        }
        if obj.advanced.is_camera_space_pinned() {
            out.push_str("        .screen_space()\n");
        } else if obj.advanced.ignore_zoom {
            out.push_str("        .ignore_zoom()\n");
        }
        if !obj.tags.is_empty() {
            for t in &obj.tags {
                out.push_str(&format!("        .tag(\"{}\")\n", t));
            }
        }
        out.push_str("        .build(canvas);\n");
        if obj.visual_asset_mode == ObjectVisualAssetMode::AnimatedSprite {
            if let Some(bytes_expr) = asset_include_expr(&obj.visual_asset_path) {
                out.push_str(&format!(
                    "    {}.set_animation(quartz::sprite::load_animation({}, ({}, {}), {}));\n",
                    obj.id, bytes_expr, obj.w, obj.h, obj.visual_asset_fps
                ));
            }
        }
        out.push_str(&format!("    canvas.add_game_object(\"{}\".to_owned(), {});\n\n", obj.id, obj.id));
    }
    out.push_str("}\n\n");

    out.push_str("pub fn register_logic(canvas: &mut Canvas) {\n");
    for tree in &scene.logic_trees {
        out.push_str(&format!("    // Update Script: {}\n", tree.name));
        out.push_str("    canvas.on_update(|canvas| {\n");
        out.push_str(&format!("        canvas.run({});\n", nodes_to_action_expr(&tree.nodes)));
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
    out.push_str(&format!("pub fn {}(canvas: &mut Canvas) {{\n", object_function_name(object)));
    out.push_str(&object_registration_body(object));
    out.push_str("}\n");
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
    out.push_str(&format!("    let mut {} = GameObject::build(\"{}\")\n", object.id, object.id));
    out.push_str(&format!("        .size({}, {})\n", object.w, object.h));
    out.push_str(&format!("        .position({}, {})\n", object.x, object.y));
    out.push_str(&format!("        .layer({})\n", object.layer));
    out.push_str(&format!(
        "        .momentum({}, {})\n",
        object.advanced.momentum_x, object.advanced.momentum_y
    ));
    out.push_str(&format!(
        "        .resistance({}, {})\n",
        object.advanced.resistance_x, object.advanced.resistance_y
    ));
    out.push_str(&format!("        .gravity({})\n", object.advanced.gravity));
    out.push_str(&format!("        .rotation({})\n", object.advanced.rotation_deg));
    out.push_str(&format!(
        "        .pivot({}, {})\n",
        object.advanced.pivot_x, object.advanced.pivot_y
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
    if object.visual_asset_mode == ObjectVisualAssetMode::StaticImage {
        if let Some(bytes_expr) = asset_include_expr(&object.visual_asset_path) {
            out.push_str(&format!(
                "        .image(quartz::sprite::load_image({}))\n",
                bytes_expr
            ));
        }
    }
    if object.advanced.is_camera_space_pinned() {
        out.push_str("        .screen_space()\n");
    } else if object.advanced.ignore_zoom {
        out.push_str("        .ignore_zoom()\n");
    }
    if !object.tags.is_empty() {
        for tag in &object.tags {
            out.push_str(&format!("        .tag(\"{}\")\n", tag));
        }
    }
    out.push_str("        .build(canvas);\n");
    if object.visual_asset_mode == ObjectVisualAssetMode::AnimatedSprite {
        if let Some(bytes_expr) = asset_include_expr(&object.visual_asset_path) {
            out.push_str(&format!(
                "    {}.set_animation(quartz::sprite::load_animation({}, ({}, {}), {}));\n",
                object.id, bytes_expr, object.w, object.h, object.visual_asset_fps
            ));
        }
    }
    out.push_str(&format!(
        "    canvas.add_game_object(\"{}\".to_owned(), {});\n",
        object.id, object.id
    ));
    out
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
        LogicNode::Action(action) => action_expr(action),
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

fn action_expr(action: &QuartzAction) -> String {
    match action {
        QuartzAction::Teleport { target, location } => format!(
            "Action::Teleport {{ target: {}, location: {} }}",
            target_expr(target),
            location_expr(location)
        ),
        QuartzAction::ApplyMomentum { target, mx, my } => format!(
            "Action::ApplyMomentum {{ target: {}, value: ({mx}, {my}) }}",
            target_expr(target)
        ),
        QuartzAction::SetMomentum { target, mx, my } => format!(
            "Action::SetMomentum {{ target: {}, value: ({mx}, {my}) }}",
            target_expr(target)
        ),
        QuartzAction::SetResistance { target, rx, ry } => format!(
            "Action::SetResistance {{ target: {}, value: ({rx}, {ry}) }}",
            target_expr(target)
        ),
        QuartzAction::SetGravity { target, value } => {
            format!("Action::SetGravity {{ target: {}, value: {} }}", target_expr(target), value)
        }
        QuartzAction::SetRotation { target, deg } => {
            format!("Action::SetRotation {{ target: {}, value: {} }}", target_expr(target), deg)
        }
        QuartzAction::SetPivot { target, x, y } => {
            format!("Action::SetPivot {{ target: {}, x: {}, y: {} }}", target_expr(target), x, y)
        }
        QuartzAction::AddRotation { target, deg } => {
            format!("Action::AddRotation {{ target: {}, value: {} }}", target_expr(target), deg)
        }
        QuartzAction::ApplyRotation { target, deg } => {
            format!("Action::ApplyRotation {{ target: {}, value: {} }}", target_expr(target), deg)
        }
        QuartzAction::SetSize { target, w, h } => {
            format!("Action::SetSize {{ target: {}, value: ({w}, {h}) }}", target_expr(target))
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
        QuartzAction::SetZoom { value } => format!("Action::SetZoom {{ value: {} }}", value),
        QuartzAction::SmoothZoom { value } => format!("Action::SmoothZoom {{ value: {} }}", value),
        QuartzAction::RunPlugin { name, data } => {
            format!("Action::RunPlugin {{ name: \"{}\".to_owned(), data: \"{}\".to_owned() }}", name, data)
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
        QuartzAction::Conditional {
            condition,
            if_true,
            if_false,
        } => {
            let if_true_expr = action_expr(if_true);
            if let Some(if_false) = if_false {
                format!(
                    "Action::Conditional {{ condition: {}, if_true: Box::new({}), if_false: Some(Box::new({})) }}",
                    condition_expr(condition),
                    if_true_expr,
                    action_expr(if_false)
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
            let parts = actions.iter().map(action_expr).collect::<Vec<_>>().join(", ");
            format!("Action::Multi(vec![{}])", parts)
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
        QuartzCondition::SpeedAbove { target, value } => {
            format!("Condition::SpeedAbove({}, {})", target_expr(target), value)
        }
        QuartzCondition::SpeedBelow { target, value } => {
            format!("Condition::SpeedBelow({}, {})", target_expr(target), value)
        }
        QuartzCondition::CrystallineEnabled => "Condition::CrystallineEnabled".to_owned(),
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
        action_expr(action)
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

