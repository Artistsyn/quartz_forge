use eframe::egui::{self, Slider};

use super::QuartzForgeApp;
use crate::core::quartz_domain::{CompareOp, QuartzCondition, QuartzTargetRef};

impl QuartzForgeApp {
    pub(super) fn edit_condition(ui: &mut egui::Ui, condition: &mut QuartzCondition) -> bool {
        // Nested condition editors reuse labels; scope each call by pointer to avoid ID collisions.
        let condition_id = condition as *const QuartzCondition as usize;
        let mut changed = false;
        ui.push_id(condition_id, |ui| {
            changed = Self::edit_condition_scoped(ui, condition);
        });
        changed
    }

    fn edit_condition_scoped(ui: &mut egui::Ui, condition: &mut QuartzCondition) -> bool {
        let mut changed = false;

        let mut kind = match condition {
            QuartzCondition::Always => "Always",
            QuartzCondition::KeyHeld { .. } => "KeyHeld",
            QuartzCondition::KeyNotHeld { .. } => "KeyNotHeld",
            QuartzCondition::Collision { .. } => "Collision",
            QuartzCondition::NoCollision { .. } => "NoCollision",
            QuartzCondition::VarCompare { .. } => "VarCompare",
            QuartzCondition::VarExists { .. } => "VarExists",
            QuartzCondition::Expr { .. } => "Expr",
            QuartzCondition::IsVisible { .. } => "IsVisible",
            QuartzCondition::IsHidden { .. } => "IsHidden",
            QuartzCondition::IsMoving { .. } => "IsMoving",
            QuartzCondition::Grounded { .. } => "Grounded",
            QuartzCondition::HasTag { .. } => "HasTag",
            QuartzCondition::IsSleeping { .. } => "IsSleeping",
            QuartzCondition::SpeedAbove { .. } => "SpeedAbove",
            QuartzCondition::SpeedBelow { .. } => "SpeedBelow",
            QuartzCondition::CrystallineEnabled => "CrystallineEnabled",
            QuartzCondition::Plugin { .. } => "Plugin",
            QuartzCondition::CollisionWith { .. } => "CollisionWith",
            QuartzCondition::And { .. } => "And",
            QuartzCondition::Or { .. } => "Or",
            QuartzCondition::Not { .. } => "Not",
        };

        egui::ComboBox::from_label("condition kind")
            .selected_text(kind)
            .show_ui(ui, |ui| {
                for name in [
                    "Always",
                    "KeyHeld",
                    "KeyNotHeld",
                    "Collision",
                    "NoCollision",
                    "CollisionWith",
                    "VarCompare",
                    "VarExists",
                    "Expr",
                    "IsVisible",
                    "IsHidden",
                    "IsMoving",
                    "Grounded",
                    "HasTag",
                    "IsSleeping",
                    "SpeedAbove",
                    "SpeedBelow",
                    "CrystallineEnabled",
                    "Plugin",
                    "And",
                    "Or",
                    "Not",
                ] {
                    changed |= ui.selectable_value(&mut kind, name, name).changed();
                }
            });

        let replacement = match kind {
            "Always" => Some(QuartzCondition::Always),
            "KeyHeld" => Some(QuartzCondition::KeyHeld {
                key: "Space".to_owned(),
            }),
            "KeyNotHeld" => Some(QuartzCondition::KeyNotHeld {
                key: "Space".to_owned(),
            }),
            "Collision" => Some(QuartzCondition::Collision {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "NoCollision" => Some(QuartzCondition::NoCollision {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "CollisionWith" => Some(QuartzCondition::CollisionWith {
                object_a: "player".to_owned(),
                object_b: "enemy".to_owned(),
            }),
            "VarCompare" => Some(QuartzCondition::VarCompare {
                variable: "score".to_owned(),
                op: CompareOp::Ge,
                value: 10.0,
            }),
            "VarExists" => Some(QuartzCondition::VarExists {
                variable: "score".to_owned(),
            }),
            "Expr" => Some(QuartzCondition::Expr {
                raw: "score > 10".to_owned(),
            }),
            "IsVisible" => Some(QuartzCondition::IsVisible {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "IsHidden" => Some(QuartzCondition::IsHidden {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "IsMoving" => Some(QuartzCondition::IsMoving {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "Grounded" => Some(QuartzCondition::Grounded {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "HasTag" => Some(QuartzCondition::HasTag {
                target: QuartzTargetRef::Name("player".to_owned()),
                tag: "tag_name".to_owned(),
            }),
            "IsSleeping" => Some(QuartzCondition::IsSleeping {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "SpeedAbove" => Some(QuartzCondition::SpeedAbove {
                target: QuartzTargetRef::Name("player".to_owned()),
                value: 0.5,
            }),
            "SpeedBelow" => Some(QuartzCondition::SpeedBelow {
                target: QuartzTargetRef::Name("player".to_owned()),
                value: 0.5,
            }),
            "CrystallineEnabled" => Some(QuartzCondition::CrystallineEnabled),
            "Plugin" => Some(QuartzCondition::Plugin {
                name: "plugin_name".to_owned(),
                arg: None,
            }),
            "And" => Some(QuartzCondition::And {
                left: Box::new(QuartzCondition::Always),
                right: Box::new(QuartzCondition::Always),
            }),
            "Or" => Some(QuartzCondition::Or {
                left: Box::new(QuartzCondition::Always),
                right: Box::new(QuartzCondition::Always),
            }),
            "Not" => Some(QuartzCondition::Not {
                inner: Box::new(QuartzCondition::Always),
            }),
            _ => None,
        };

        if let Some(next) = replacement {
            if std::mem::discriminant(condition) != std::mem::discriminant(&next) {
                *condition = next;
                changed = true;
            }
        }

        match condition {
            QuartzCondition::KeyHeld { key } | QuartzCondition::KeyNotHeld { key } => {
                changed |= ui.text_edit_singleline(key).changed();
            }
            QuartzCondition::Collision { target } | QuartzCondition::NoCollision { target } => {
                changed |= Self::edit_target_ref(ui, "target", target);
            }
            QuartzCondition::CollisionWith { object_a, object_b } => {
                ui.label("object_a");
                changed |= ui.text_edit_singleline(object_a).changed();
                ui.label("object_b");
                changed |= ui.text_edit_singleline(object_b).changed();
            }
            QuartzCondition::VarCompare {
                variable,
                op,
                value,
            } => {
                changed |= ui.text_edit_singleline(variable).changed();
                changed |= ui.add(Slider::new(value, -10000.0..=10000.0).text("value")).changed();
                egui::ComboBox::from_label("compare op")
                    .selected_text(op.as_str())
                    .show_ui(ui, |ui| {
                        changed |= ui.selectable_value(op, CompareOp::Eq, "==").changed();
                        changed |= ui.selectable_value(op, CompareOp::Ne, "!=").changed();
                        changed |= ui.selectable_value(op, CompareOp::Lt, "<").changed();
                        changed |= ui.selectable_value(op, CompareOp::Le, "<=").changed();
                        changed |= ui.selectable_value(op, CompareOp::Gt, ">").changed();
                        changed |= ui.selectable_value(op, CompareOp::Ge, ">=").changed();
                    });
            }
            QuartzCondition::VarExists { variable } => {
                ui.label("variable");
                changed |= ui.text_edit_singleline(variable).changed();
            }
            QuartzCondition::Expr { raw } => {
                ui.label("expr source");
                changed |= ui.text_edit_singleline(raw).changed();
            }
            QuartzCondition::IsVisible { target }
            | QuartzCondition::IsHidden { target }
            | QuartzCondition::IsMoving { target }
            | QuartzCondition::Grounded { target }
            | QuartzCondition::IsSleeping { target } => {
                changed |= Self::edit_target_ref(ui, "target", target);
            }
            QuartzCondition::HasTag { target, tag } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.text_edit_singleline(tag).changed();
            }
            QuartzCondition::SpeedAbove { target, value }
            | QuartzCondition::SpeedBelow { target, value } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(value, 0.0..=1000.0).text("value")).changed();
            }
            QuartzCondition::Plugin { name, arg } => {
                changed |= ui.text_edit_singleline(name).changed();
                let mut arg_text = arg.clone().unwrap_or_default();
                if ui.text_edit_singleline(&mut arg_text).changed() {
                    *arg = if arg_text.trim().is_empty() {
                        None
                    } else {
                        Some(arg_text)
                    };
                    changed = true;
                }
            }
            QuartzCondition::And { left, right } | QuartzCondition::Or { left, right } => {
                ui.collapsing("left", |ui| {
                    changed |= Self::edit_condition(ui, left);
                });
                ui.collapsing("right", |ui| {
                    changed |= Self::edit_condition(ui, right);
                });
            }
            QuartzCondition::Not { inner } => {
                ui.collapsing("inner", |ui| {
                    changed |= Self::edit_condition(ui, inner);
                });
            }
            QuartzCondition::Always | QuartzCondition::CrystallineEnabled => {}
        }

        changed
    }
}
