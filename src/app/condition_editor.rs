use eframe::egui::{self, Slider};

use super::QuartzForgeApp;
use super::EditorSuggestions;
use crate::core::quartz_domain::{
    CompareOp, QuartzCondition, QuartzExpr, QuartzExprKind, QuartzTargetRef,
};

impl QuartzForgeApp {
    pub(super) fn edit_condition(
        ui: &mut egui::Ui,
        condition: &mut QuartzCondition,
        suggestions: &EditorSuggestions,
    ) -> bool {
        // Nested condition editors reuse labels; scope each call by pointer to avoid ID collisions.
        let condition_id = condition as *const QuartzCondition as usize;
        let mut changed = false;
        ui.push_id(condition_id, |ui| {
            changed = Self::edit_condition_scoped(ui, condition, suggestions);
        });
        changed
    }

    fn edit_condition_scoped(
        ui: &mut egui::Ui,
        condition: &mut QuartzCondition,
        suggestions: &EditorSuggestions,
    ) -> bool {
        let mut changed = false;

        let mut kind = match condition {
            QuartzCondition::Always => "Always",
            QuartzCondition::KeyHeld { .. } => "KeyHeld",
            QuartzCondition::KeyNotHeld { .. } => "KeyNotHeld",
            QuartzCondition::Collision { .. } => "Collision",
            QuartzCondition::NoCollision { .. } => "NoCollision",
            QuartzCondition::VarCompare { .. } => "VarCompare",
            QuartzCondition::Compare { .. } => "Compare",
            QuartzCondition::VarExists { .. } => "VarExists",
            QuartzCondition::Expr { .. } => "Expr",
            QuartzCondition::IsVisible { .. } => "IsVisible",
            QuartzCondition::IsHidden { .. } => "IsHidden",
            QuartzCondition::IsMoving { .. } => "IsMoving",
            QuartzCondition::Grounded { .. } => "Grounded",
            QuartzCondition::HasTag { .. } => "HasTag",
            QuartzCondition::IsSleeping { .. } => "IsSleeping",
            QuartzCondition::IsRotating { .. } => "IsRotating",
            QuartzCondition::IsStill { .. } => "IsStill",
            QuartzCondition::SpeedAbove { .. } => "SpeedAbove",
            QuartzCondition::SpeedBelow { .. } => "SpeedBelow",
            QuartzCondition::CrystallineEnabled => "CrystallineEnabled",
            QuartzCondition::EmitterActive { .. } => "EmitterActive",
            QuartzCondition::OnPlanet { .. } => "OnPlanet",
            QuartzCondition::InGravityField { .. } => "InGravityField",
            QuartzCondition::HasDominantPlanet { .. } => "HasDominantPlanet",
            QuartzCondition::DominantPlanetIs { .. } => "DominantPlanetIs",
            QuartzCondition::InAnyGravityField { .. } => "InAnyGravityField",
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
                    "Compare",
                    "VarExists",
                    "Expr",
                    "IsVisible",
                    "IsHidden",
                    "IsMoving",
                    "Grounded",
                    "HasTag",
                    "IsSleeping",
                    "IsRotating",
                    "IsStill",
                    "SpeedAbove",
                    "SpeedBelow",
                    "CrystallineEnabled",
                    "EmitterActive",
                    "OnPlanet",
                    "InGravityField",
                    "HasDominantPlanet",
                    "DominantPlanetIs",
                    "InAnyGravityField",
                    "Plugin",
                    "And",
                    "Or",
                    "Not",
                ] {
                    changed |= ui.selectable_value(&mut kind, name, name).changed();
                }
            });

        let replacement = Self::condition_from_kind(kind);

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
                changed |= Self::suggestion_text_edit(
                    ui,
                    "condition_var_compare",
                    variable,
                    &suggestions.variable_names,
                );
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
            QuartzCondition::Compare { left, op, right } => {
                ui.label("left expr");
                changed |= Self::edit_condition_quartz_expr(ui, left, suggestions);
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
                ui.label("right expr");
                    changed |= Self::edit_condition_quartz_expr(ui, right, suggestions);
            }
            QuartzCondition::VarExists { variable } => {
                ui.label("variable");
                    changed |= Self::suggestion_text_edit(
                        ui,
                        "condition_var_exists",
                        variable,
                        &suggestions.variable_names,
                    );
            }
            QuartzCondition::Expr { raw } => {
                ui.label("expr source");
                changed |= ui.text_edit_singleline(raw).changed();
            }
            QuartzCondition::IsVisible { target }
            | QuartzCondition::IsHidden { target }
            | QuartzCondition::IsMoving { target }
            | QuartzCondition::Grounded { target }
            | QuartzCondition::IsSleeping { target }
            | QuartzCondition::IsRotating { target }
            | QuartzCondition::IsStill { target }
            | QuartzCondition::HasDominantPlanet { target }
            | QuartzCondition::InAnyGravityField { target } => {
                changed |= Self::edit_target_ref(ui, "target", target);
            }
            QuartzCondition::HasTag { target, tag } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.text_edit_singleline(tag).changed();
            }
            QuartzCondition::OnPlanet { target, planet }
            | QuartzCondition::InGravityField { target, planet }
            | QuartzCondition::DominantPlanetIs { target, planet } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= Self::edit_target_ref(ui, "planet", planet);
            }
            QuartzCondition::SpeedAbove { target, value }
            | QuartzCondition::SpeedBelow { target, value } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(value, 0.0..=1000.0).text("value")).changed();
            }
            QuartzCondition::EmitterActive { emitter } => {
                ui.label("emitter id");
                changed |= ui.text_edit_singleline(emitter).changed();
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
                    changed |= Self::edit_condition(ui, left, suggestions);
                });
                ui.collapsing("right", |ui| {
                    changed |= Self::edit_condition(ui, right, suggestions);
                });
            }
            QuartzCondition::Not { inner } => {
                ui.collapsing("inner", |ui| {
                    changed |= Self::edit_condition(ui, inner, suggestions);
                });
            }
            QuartzCondition::Always | QuartzCondition::CrystallineEnabled => {}
        }

        if let Err(message) = Self::validate_condition_combination(condition) {
            ui.colored_label(egui::Color32::from_rgb(220, 70, 70), message);
        }

        changed
    }

    fn edit_condition_quartz_expr(
        ui: &mut egui::Ui,
        expr: &mut QuartzExpr,
        suggestions: &EditorSuggestions,
    ) -> bool {
        let mut changed = false;
        egui::ComboBox::from_label("expr kind")
            .selected_text(match expr.kind {
                QuartzExprKind::F32 => "F32",
                QuartzExprKind::I32 => "I32",
                QuartzExprKind::Bool => "Bool",
                QuartzExprKind::Str => "Str",
                QuartzExprKind::Var => "Var",
            })
            .show_ui(ui, |ui| {
                changed |= ui.selectable_value(&mut expr.kind, QuartzExprKind::F32, "F32").changed();
                changed |= ui.selectable_value(&mut expr.kind, QuartzExprKind::I32, "I32").changed();
                changed |= ui.selectable_value(&mut expr.kind, QuartzExprKind::Bool, "Bool").changed();
                changed |= ui.selectable_value(&mut expr.kind, QuartzExprKind::Str, "Str").changed();
                changed |= ui.selectable_value(&mut expr.kind, QuartzExprKind::Var, "Var").changed();
            });
        let options = if expr.kind == QuartzExprKind::Var {
            &suggestions.variable_names
        } else {
            &suggestions.expression_names
        };
        changed |= Self::suggestion_text_edit(ui, "condition_expr_raw", &mut expr.raw, options);
        changed
    }

    fn condition_from_kind(kind: &str) -> Option<QuartzCondition> {
        match kind {
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
            "Compare" => Some(QuartzCondition::Compare {
                left: QuartzExpr {
                    kind: QuartzExprKind::Var,
                    raw: "score".to_owned(),
                },
                op: CompareOp::Ge,
                right: QuartzExpr {
                    kind: QuartzExprKind::I32,
                    raw: "10".to_owned(),
                },
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
            "IsRotating" => Some(QuartzCondition::IsRotating {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "IsStill" => Some(QuartzCondition::IsStill {
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
            "EmitterActive" => Some(QuartzCondition::EmitterActive {
                emitter: "thruster_smoke".to_owned(),
            }),
            "OnPlanet" => Some(QuartzCondition::OnPlanet {
                target: QuartzTargetRef::Name("player".to_owned()),
                planet: QuartzTargetRef::Tag("planet".to_owned()),
            }),
            "InGravityField" => Some(QuartzCondition::InGravityField {
                target: QuartzTargetRef::Name("player".to_owned()),
                planet: QuartzTargetRef::Tag("planet".to_owned()),
            }),
            "HasDominantPlanet" => Some(QuartzCondition::HasDominantPlanet {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "DominantPlanetIs" => Some(QuartzCondition::DominantPlanetIs {
                target: QuartzTargetRef::Name("player".to_owned()),
                planet: QuartzTargetRef::Tag("planet".to_owned()),
            }),
            "InAnyGravityField" => Some(QuartzCondition::InAnyGravityField {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
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
        }
    }

    fn validate_condition_combination(condition: &QuartzCondition) -> Result<(), String> {
        match condition {
            QuartzCondition::Compare { left, right, .. } => {
                if left.raw.trim().is_empty() || right.raw.trim().is_empty() {
                    return Err("compare expressions must not be empty".to_owned());
                }
            }
            QuartzCondition::EmitterActive { emitter } => {
                if emitter.trim().is_empty() {
                    return Err("emitter id must not be empty".to_owned());
                }
            }
            QuartzCondition::OnPlanet { target, planet }
            | QuartzCondition::InGravityField { target, planet }
            | QuartzCondition::DominantPlanetIs { target, planet } => {
                if target.short_label() == planet.short_label() {
                    return Err("target and planet must differ".to_owned());
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::QuartzForgeApp;
    use crate::core::quartz_domain::QuartzCondition;

    #[test]
    fn condition_editor_builds_compare_condition() {
        let built = QuartzForgeApp::condition_from_kind("Compare");
        match built {
            Some(QuartzCondition::Compare { left, right, .. }) => {
                assert_eq!(left.raw, "score");
                assert_eq!(right.raw, "10");
            }
            other => panic!("expected Compare condition, got {:?}", other),
        }
    }

    #[test]
    fn condition_editor_builds_gravity_conditions() {
        assert!(matches!(
            QuartzForgeApp::condition_from_kind("OnPlanet"),
            Some(QuartzCondition::OnPlanet { .. })
        ));
        assert!(matches!(
            QuartzForgeApp::condition_from_kind("InGravityField"),
            Some(QuartzCondition::InGravityField { .. })
        ));
        assert!(matches!(
            QuartzForgeApp::condition_from_kind("HasDominantPlanet"),
            Some(QuartzCondition::HasDominantPlanet { .. })
        ));
        assert!(matches!(
            QuartzForgeApp::condition_from_kind("DominantPlanetIs"),
            Some(QuartzCondition::DominantPlanetIs { .. })
        ));
        assert!(matches!(
            QuartzForgeApp::condition_from_kind("InAnyGravityField"),
            Some(QuartzCondition::InAnyGravityField { .. })
        ));
    }

    #[test]
    fn condition_editor_rejects_invalid_condition_combinations() {
        let invalid = QuartzCondition::OnPlanet {
            target: crate::core::quartz_domain::QuartzTargetRef::Name("player".to_owned()),
            planet: crate::core::quartz_domain::QuartzTargetRef::Name("player".to_owned()),
        };
        let err = QuartzForgeApp::validate_condition_combination(&invalid);
        assert!(err.is_err());
    }
}
