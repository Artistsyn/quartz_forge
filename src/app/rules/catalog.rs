//! Plain-word names and menu groups for every action, condition and trigger, so the rule editor
//! reads "Push (add speed)" where the data says `ApplyMomentum`.

use crate::core::quartz_domain::{QuartzAction, QuartzCondition, QuartzEventKind, QuartzKeyModifiers, QuartzMouseButtonFilter, QuartzScrollAxisFilter};
use crate::core::samples::Samples;

/// The groups of the Add Action menu, in menu order.
pub(in crate::app) const CATEGORIES: [&str; 11] =
    ["Movement", "Turn and size", "Objects", "Look", "Camera", "Physics", "Gravity wells", "Particles", "Variables and text", "Sound", "Advanced"];

/// Action variants with a hand-written name and group; any other is named from its variant.
const ACTION_NAMES: [(&str, &str, &str); 47] = [
    ("RemoveParticles", "Particles", "Remove particle effect now"),
    ("PlayParticles", "Particles", "Play particle effect"),
    ("StopParticles", "Particles", "Stop particle effect"),
    ("Teleport", "Movement", "Move to"),
    ("SetPosition", "Movement", "Set position"),
    ("ApplyMomentum", "Movement", "Push (add speed)"),
    ("SetMomentum", "Movement", "Set speed"),
    ("SetResistance", "Movement", "Set resistance"),
    ("SetGravity", "Movement", "Set gravity"),
    ("TransferMomentum", "Movement", "Pass speed to another"),
    ("SetRotation", "Turn and size", "Set rotation"),
    ("AddRotation", "Turn and size", "Turn by"),
    ("ApplyRotation", "Turn and size", "Spin (add turning speed)"),
    ("SetPivot", "Turn and size", "Set pivot"),
    ("SetSize", "Turn and size", "Set size"),
    ("Spawn", "Objects", "Spawn a copy"),
    ("Remove", "Objects", "Remove"),
    ("Show", "Objects", "Show"),
    ("Hide", "Objects", "Hide"),
    ("Toggle", "Objects", "Show or hide"),
    ("AddTag", "Objects", "Add tag"),
    ("RemoveTag", "Objects", "Remove tag"),
    ("SetRenderLayer", "Objects", "Set draw layer"),
    ("SetCameraRelative", "Objects", "Pin to camera"),
    ("SetCollisionLayer", "Physics", "Set collision layer"),
    ("SetCollisionMode", "Physics", "Set collision kind"),
    ("SetAnimation", "Look", "Play animation"),
    ("SetTint", "Look", "Tint"),
    ("ClearTint", "Look", "Clear tint"),
    ("SetGlow", "Look", "Glow"),
    ("ClearGlow", "Look", "Clear glow"),
    ("SetText", "Variables and text", "Set text"),
    ("SetVar", "Variables and text", "Set variable"),
    ("ModVar", "Variables and text", "Change variable"),
    ("PlaySound", "Sound", "Play sound"),
    ("CameraFlash", "Camera", "Flash"),
    ("CameraFlashWith", "Camera", "Flash with colour"),
    ("CameraShake", "Camera", "Shake"),
    ("CameraZoomPunch", "Camera", "Zoom punch"),
    ("SetZoom", "Camera", "Set zoom"),
    ("AddZoom", "Camera", "Zoom by"),
    ("SmoothZoom", "Camera", "Ease zoom"),
    ("SmoothZoomAt", "Camera", "Ease zoom toward a point"),
    ("PluginCall", "Advanced", "Call plugin"),
    ("RunPlugin", "Advanced", "Run plugin command"),
    ("Expr", "Advanced", "Rust expression"),
    ("Custom", "Advanced", "Custom code"),
];

/// Actions the block editor shows as blocks of their own rather than as one action.
const STRUCTURAL: [&str; 2] = ["Conditional", "Multi"];

pub(in crate::app) struct Catalog;

impl Catalog {
    /// The variant name of an externally tagged enum value (`{"Teleport": {...}}` or `"Tick"`).
    pub fn variant<T: serde::Serialize>(value: &T) -> String {
        match serde_json::to_value(value) {
            Ok(serde_json::Value::Object(map)) => map.keys().next().cloned().unwrap_or_default(),
            Ok(serde_json::Value::String(name)) => name,
            _ => String::new(),
        }
    }

    /// "SetEmitterRate" as "Set emitter rate".
    pub fn words(variant: &str) -> String {
        let mut text = String::new();
        for (index, letter) in variant.chars().enumerate() {
            if letter.is_uppercase() && index > 0 {
                text.push(' ');
                text.extend(letter.to_lowercase());
            } else {
                text.push(letter);
            }
        }
        text
    }

    pub fn action_label(action: &QuartzAction) -> String {
        Self::variant_label(&Self::variant(action))
    }

    /// The plain name of an action variant (`"ApplyMomentum"` is "Push (add speed)").
    pub fn variant_label(variant: &str) -> String {
        ACTION_NAMES.iter().find(|(name, _, _)| *name == variant).map_or_else(|| Self::words(variant), |(_, _, label)| (*label).to_owned())
    }

    pub fn action_category(variant: &str) -> &'static str {
        if let Some((_, category, _)) = ACTION_NAMES.iter().find(|(name, _, _)| *name == variant) {
            return category;
        }
        if variant.contains("Emitter") {
            "Particles"
        } else if variant.contains("Gravity") || variant.contains("Planet") {
            "Gravity wells"
        } else {
            "Physics"
        }
    }

    /// One fresh action of every kind the Add menu offers, grouped by category, with labels.
    pub fn menu(samples: &Samples) -> Vec<(&'static str, Vec<(String, QuartzAction)>)> {
        let actions: Vec<QuartzAction> = samples.actions().into_iter().filter(|action| !STRUCTURAL.contains(&Self::variant(action).as_str())).collect();
        CATEGORIES
            .iter()
            .map(|category| {
                let entries: Vec<(String, QuartzAction)> = actions
                    .iter()
                    .filter(|action| Self::action_category(&Self::variant(*action)) == *category)
                    .map(|action| (Self::action_label(action), action.clone()))
                    .collect();
                (*category, entries)
            })
            .filter(|(_, entries)| !entries.is_empty())
            .collect()
    }

    pub fn condition_label(condition: &QuartzCondition) -> String {
        Self::words(&Self::variant(condition))
    }

    /// The triggers an event can wait for, as (label, fresh value).
    pub fn triggers() -> Vec<(&'static str, QuartzEventKind)> {
        let key = || "Space".to_owned();
        vec![
            ("Key pressed", QuartzEventKind::KeyPress { key: key(), modifiers: QuartzKeyModifiers::default() }),
            ("Key held", QuartzEventKind::KeyHold { key: key(), modifiers: QuartzKeyModifiers::default() }),
            ("Key released", QuartzEventKind::KeyRelease { key: key(), modifiers: QuartzKeyModifiers::default() }),
            ("Touches something", QuartzEventKind::Collision),
            ("Hits the screen edge", QuartzEventKind::BoundaryCollision),
            ("Clicked", QuartzEventKind::MousePress { button: QuartzMouseButtonFilter::Left }),
            ("Mouse released", QuartzEventKind::MouseRelease { button: QuartzMouseButtonFilter::Left }),
            ("Mouse enters", QuartzEventKind::MouseEnter),
            ("Mouse leaves", QuartzEventKind::MouseLeave),
            ("Mouse over", QuartzEventKind::MouseOver),
            ("Mouse moves", QuartzEventKind::MouseMove),
            ("Scrolled", QuartzEventKind::MouseScroll { axis: QuartzScrollAxisFilter::Any }),
            ("Every frame", QuartzEventKind::Tick),
            ("Custom signal", QuartzEventKind::Custom { name: "signal".to_owned() }),
        ]
    }

    pub fn trigger_label(kind: &QuartzEventKind) -> &'static str {
        let wanted = std::mem::discriminant(kind);
        Self::triggers().into_iter().find(|(_, option)| std::mem::discriminant(option) == wanted).map_or("Event", |(label, _)| label)
    }
}

#[cfg(test)]
mod tests {
    use super::{Catalog, CATEGORIES};
    use crate::core::quartz_domain::QuartzTargetRef;
    use crate::core::samples::Samples;

    #[test]
    fn every_action_has_a_menu_place_and_a_name() {
        let samples = Samples {
            target: QuartzTargetRef::Name("player".to_owned()),
            other: QuartzTargetRef::Name("crate".to_owned()),
            template_id: "bullet".to_owned(),
            animation_asset: "assets/run.gif".to_owned(),
        };
        let menu = Catalog::menu(&samples);
        let listed: usize = menu.iter().map(|(_, entries)| entries.len()).sum();
        assert_eq!(listed, samples.actions().len() - 2, "every action but If/else and Multi is offered");
        assert!(menu.iter().all(|(category, _)| CATEGORIES.contains(category)));
        assert_eq!(Catalog::words("SetEmitterRate"), "Set emitter rate");
        assert!(menu.iter().flat_map(|(_, entries)| entries).all(|(label, _)| !label.is_empty()));
    }
}
