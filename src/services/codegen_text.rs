use crate::core::quartz_domain::{
    CompareOp, ObjectPhysicsMaterialPreset, ObjectPhysicsMaterialSpec, QuartzKeyModifiers,
    QuartzLocationRef, QuartzMouseButtonFilter, QuartzScrollAxisFilter, QuartzTargetRef,
};

pub(crate) fn comp_op_expr(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Eq => "CompOp::Eq",
        CompareOp::Ne => "CompOp::Ne",
        CompareOp::Lt => "CompOp::Lt",
        CompareOp::Le => "CompOp::Lte",
        CompareOp::Gt => "CompOp::Gt",
        CompareOp::Ge => "CompOp::Gte",
    }
}

pub(crate) fn key_expr(key: &str) -> String {
    match key.to_ascii_lowercase().as_str() {
        "escape" => "Key::Named(NamedKey::Escape)".to_owned(),
        "enter" => "Key::Named(NamedKey::Enter)".to_owned(),
        "tab" => "Key::Named(NamedKey::Tab)".to_owned(),
        "space" => "Key::Named(NamedKey::Space)".to_owned(),
        "arrowup" | "up" => "Key::Named(NamedKey::ArrowUp)".to_owned(),
        "arrowdown" | "down" => "Key::Named(NamedKey::ArrowDown)".to_owned(),
        "arrowleft" | "left" => "Key::Named(NamedKey::ArrowLeft)".to_owned(),
        "arrowright" | "right" => "Key::Named(NamedKey::ArrowRight)".to_owned(),
        "delete" => "Key::Named(NamedKey::Delete)".to_owned(),
        "backspace" => "Key::Named(NamedKey::Backspace)".to_owned(),
        "home" => "Key::Named(NamedKey::Home)".to_owned(),
        "end" => "Key::Named(NamedKey::End)".to_owned(),
        value => format!("Key::Character(\"{}\".to_owned())", value),
    }
}

pub(crate) fn modifiers_expr(modifiers: &QuartzKeyModifiers) -> String {
    if !modifiers.shift && !modifiers.control && !modifiers.alt && !modifiers.meta {
        "None".to_owned()
    } else {
        format!(
            "Some(Modifiers {{ shift: {}, control: {}, alt: {}, meta: {} }})",
            modifiers.shift, modifiers.control, modifiers.alt, modifiers.meta
        )
    }
}

pub(crate) fn mouse_button_expr(button: QuartzMouseButtonFilter) -> &'static str {
    match button {
        QuartzMouseButtonFilter::Any => "None",
        QuartzMouseButtonFilter::Left => "Some(MouseButton::Left)",
        QuartzMouseButtonFilter::Right => "Some(MouseButton::Right)",
        QuartzMouseButtonFilter::Middle => "Some(MouseButton::Middle)",
    }
}

pub(crate) fn scroll_axis_expr(axis: QuartzScrollAxisFilter) -> &'static str {
    match axis {
        QuartzScrollAxisFilter::Any => "None",
        QuartzScrollAxisFilter::X => "Some(ScrollAxis::X)",
        QuartzScrollAxisFilter::Y => "Some(ScrollAxis::Y)",
    }
}

pub(crate) fn target_expr(target: &QuartzTargetRef) -> String {
    match target {
        QuartzTargetRef::Name(v) => format!("Target::name(\"{}\")", v),
        QuartzTargetRef::Tag(v) => format!("Target::tag(\"{}\")", v),
        QuartzTargetRef::Id(v) => format!("Target::id(\"{}\")", v),
    }
}

pub(crate) fn location_expr(location: &QuartzLocationRef) -> String {
    match location {
        QuartzLocationRef::At { x, y } => format!("Location::at({x}, {y})"),
        QuartzLocationRef::AtTarget(target) => format!("Location::at_target({})", target_expr(target)),
    }
}

pub(crate) fn physics_material_expr(material: &ObjectPhysicsMaterialSpec) -> String {
    match material.preset {
        ObjectPhysicsMaterialPreset::Default => "PhysicsMaterial::default()".to_owned(),
        ObjectPhysicsMaterialPreset::Rubber => "PhysicsMaterial::rubber()".to_owned(),
        ObjectPhysicsMaterialPreset::Ice => "PhysicsMaterial::ice()".to_owned(),
        ObjectPhysicsMaterialPreset::Metal => "PhysicsMaterial::metal()".to_owned(),
        ObjectPhysicsMaterialPreset::Wood => "PhysicsMaterial::wood()".to_owned(),
        ObjectPhysicsMaterialPreset::Stone => "PhysicsMaterial::stone()".to_owned(),
        ObjectPhysicsMaterialPreset::Bouncy => "PhysicsMaterial::bouncy()".to_owned(),
        ObjectPhysicsMaterialPreset::Sticky => "PhysicsMaterial::sticky()".to_owned(),
        ObjectPhysicsMaterialPreset::Glass => "PhysicsMaterial::glass()".to_owned(),
        ObjectPhysicsMaterialPreset::Feather => "PhysicsMaterial::feather()".to_owned(),
        ObjectPhysicsMaterialPreset::Custom => {
            if let Some((elasticity, friction, density)) = material.to_custom_material() {
                format!(
                    "PhysicsMaterial {{ elasticity: {}, friction: {}, density: {} }}",
                    elasticity, friction, density
                )
            } else {
                "PhysicsMaterial::default()".to_owned()
            }
        }
    }
}
