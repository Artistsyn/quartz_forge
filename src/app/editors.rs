use eframe::egui::{self, Slider};

use super::QuartzForgeApp;
use crate::core::quartz_domain::{
    QuartzAction, QuartzCondition, QuartzLocationRef, QuartzTargetRef,
};

impl QuartzForgeApp {
    pub(super) fn edit_target_ref(ui: &mut egui::Ui, label: &str, target: &mut QuartzTargetRef) -> bool {
        let mut changed = false;
        ui.label(label);

        let mut mode = match target {
            QuartzTargetRef::Name(_) => "Name",
            QuartzTargetRef::Tag(_) => "Tag",
            QuartzTargetRef::Id(_) => "Id",
        };

        egui::ComboBox::from_label(format!("{label} kind"))
            .selected_text(mode)
            .show_ui(ui, |ui| {
                changed |= ui.selectable_value(&mut mode, "Name", "Name").changed();
                changed |= ui.selectable_value(&mut mode, "Tag", "Tag").changed();
                changed |= ui.selectable_value(&mut mode, "Id", "Id").changed();
            });

        let mut value = match target {
            QuartzTargetRef::Name(v) | QuartzTargetRef::Tag(v) | QuartzTargetRef::Id(v) => v.clone(),
        };
        if ui.text_edit_singleline(&mut value).changed() {
            changed = true;
        }

        let next_target = match mode {
            "Tag" => QuartzTargetRef::Tag(value),
            "Id" => QuartzTargetRef::Id(value),
            _ => QuartzTargetRef::Name(value),
        };
        if std::mem::discriminant(target) != std::mem::discriminant(&next_target)
            || target.short_label() != next_target.short_label()
        {
            *target = next_target;
            changed = true;
        }

        changed
    }

    pub(super) fn edit_action(ui: &mut egui::Ui, action: &mut QuartzAction) -> bool {
        // Nested action editors reuse many labels; scope each call by pointer to avoid ID collisions.
        let action_id = action as *const QuartzAction as usize;
        let mut changed = false;
        ui.push_id(action_id, |ui| {
            changed = Self::edit_action_scoped(ui, action);
        });
        changed
    }

    fn edit_action_scoped(ui: &mut egui::Ui, action: &mut QuartzAction) -> bool {
        let mut changed = false;

        let mut action_kind = match action {
            QuartzAction::Teleport { .. } => "Teleport",
            QuartzAction::ApplyMomentum { .. } => "ApplyMomentum",
            QuartzAction::SetMomentum { .. } => "SetMomentum",
            QuartzAction::SetResistance { .. } => "SetResistance",
            QuartzAction::SetGravity { .. } => "SetGravity",
            QuartzAction::SetRotation { .. } => "SetRotation",
            QuartzAction::SetPivot { .. } => "SetPivot",
            QuartzAction::AddRotation { .. } => "AddRotation",
            QuartzAction::ApplyRotation { .. } => "ApplyRotation",
            QuartzAction::SetSize { .. } => "SetSize",
            QuartzAction::SetCollisionLayer { .. } => "SetCollisionLayer",
            QuartzAction::SetCameraRelative { .. } => "SetCameraRelative",
            QuartzAction::SetRenderLayer { .. } => "SetRenderLayer",
            QuartzAction::Show { .. } => "Show",
            QuartzAction::Hide { .. } => "Hide",
            QuartzAction::Toggle { .. } => "Toggle",
            QuartzAction::Remove { .. } => "Remove",
            QuartzAction::AddTag { .. } => "AddTag",
            QuartzAction::RemoveTag { .. } => "RemoveTag",
            QuartzAction::SetAnimation { .. } => "SetAnimation",
            QuartzAction::SetZoom { .. } => "SetZoom",
            QuartzAction::SmoothZoom { .. } => "SmoothZoom",
            QuartzAction::RunPlugin { .. } => "RunPlugin",
            QuartzAction::Custom { .. } => "Custom",
            QuartzAction::CameraFlash { .. } => "CameraFlash",
            QuartzAction::CameraShake { .. } => "CameraShake",
            QuartzAction::CameraZoomPunch { .. } => "CameraZoomPunch",
            QuartzAction::Conditional { .. } => "Conditional",
            QuartzAction::Multi { .. } => "Multi",
        };

        egui::ComboBox::from_label("action kind")
            .selected_text(action_kind)
            .show_ui(ui, |ui| {
                changed |= ui.selectable_value(&mut action_kind, "Teleport", "Teleport").changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "ApplyMomentum", "ApplyMomentum")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetMomentum", "SetMomentum")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetResistance", "SetResistance")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetGravity", "SetGravity")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetRotation", "SetRotation")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetPivot", "SetPivot")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "AddRotation", "AddRotation")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "ApplyRotation", "ApplyRotation")
                    .changed();
                changed |= ui.selectable_value(&mut action_kind, "SetSize", "SetSize").changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetCollisionLayer", "SetCollisionLayer")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetCameraRelative", "SetCameraRelative")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetRenderLayer", "SetRenderLayer")
                    .changed();
                changed |= ui.selectable_value(&mut action_kind, "Show", "Show").changed();
                changed |= ui.selectable_value(&mut action_kind, "Hide", "Hide").changed();
                changed |= ui.selectable_value(&mut action_kind, "Toggle", "Toggle").changed();
                changed |= ui.selectable_value(&mut action_kind, "Remove", "Remove").changed();
                changed |= ui.selectable_value(&mut action_kind, "AddTag", "AddTag").changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "RemoveTag", "RemoveTag")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetAnimation", "SetAnimation")
                    .changed();
                changed |= ui.selectable_value(&mut action_kind, "SetZoom", "SetZoom").changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SmoothZoom", "SmoothZoom")
                    .changed();
                changed |= ui.selectable_value(&mut action_kind, "RunPlugin", "RunPlugin").changed();
                changed |= ui.selectable_value(&mut action_kind, "Custom", "Custom").changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "CameraFlash", "CameraFlash")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "CameraShake", "CameraShake")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "CameraZoomPunch", "CameraZoomPunch")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "Conditional", "Conditional")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "Multi", "Multi")
                    .changed();
            });

        let replacement = match action_kind {
            "Teleport" => Some(QuartzAction::Teleport {
                target: QuartzTargetRef::Name("player".to_owned()),
                location: QuartzLocationRef::At { x: 400.0, y: 300.0 },
            }),
            "ApplyMomentum" => Some(QuartzAction::ApplyMomentum {
                target: QuartzTargetRef::Name("player".to_owned()),
                mx: 0.0,
                my: -5.0,
            }),
            "SetMomentum" => Some(QuartzAction::SetMomentum {
                target: QuartzTargetRef::Name("player".to_owned()),
                mx: 0.0,
                my: -5.0,
            }),
            "SetResistance" => Some(QuartzAction::SetResistance {
                target: QuartzTargetRef::Name("player".to_owned()),
                rx: 0.0,
                ry: 0.0,
            }),
            "SetGravity" => Some(QuartzAction::SetGravity {
                target: QuartzTargetRef::Name("player".to_owned()),
                value: 0.6,
            }),
            "SetRotation" => Some(QuartzAction::SetRotation {
                target: QuartzTargetRef::Name("player".to_owned()),
                deg: 0.0,
            }),
            "SetPivot" => Some(QuartzAction::SetPivot {
                target: QuartzTargetRef::Name("player".to_owned()),
                x: 0.5,
                y: 0.5,
            }),
            "AddRotation" => Some(QuartzAction::AddRotation {
                target: QuartzTargetRef::Name("player".to_owned()),
                deg: 5.0,
            }),
            "ApplyRotation" => Some(QuartzAction::ApplyRotation {
                target: QuartzTargetRef::Name("player".to_owned()),
                deg: 5.0,
            }),
            "SetSize" => Some(QuartzAction::SetSize {
                target: QuartzTargetRef::Name("player".to_owned()),
                w: 64.0,
                h: 64.0,
            }),
            "SetCollisionLayer" => Some(QuartzAction::SetCollisionLayer {
                target: QuartzTargetRef::Name("player".to_owned()),
                layer: 1,
            }),
            "SetCameraRelative" => Some(QuartzAction::SetCameraRelative {
                target: QuartzTargetRef::Name("player".to_owned()),
                enabled: true,
            }),
            "SetRenderLayer" => Some(QuartzAction::SetRenderLayer {
                target: QuartzTargetRef::Name("player".to_owned()),
                layer: 0,
            }),
            "Show" => Some(QuartzAction::Show {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "Hide" => Some(QuartzAction::Hide {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "Toggle" => Some(QuartzAction::Toggle {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "Remove" => Some(QuartzAction::Remove {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "AddTag" => Some(QuartzAction::AddTag {
                target: QuartzTargetRef::Name("player".to_owned()),
                tag: "tag_name".to_owned(),
            }),
            "RemoveTag" => Some(QuartzAction::RemoveTag {
                target: QuartzTargetRef::Name("player".to_owned()),
                tag: "tag_name".to_owned(),
            }),
            "SetAnimation" => Some(QuartzAction::SetAnimation {
                target: QuartzTargetRef::Name("player".to_owned()),
                animation_asset: "assets/images/anim.gif".to_owned(),
                fps: 12.0,
            }),
            "SetZoom" => Some(QuartzAction::SetZoom { value: 1.0 }),
            "SmoothZoom" => Some(QuartzAction::SmoothZoom { value: 1.0 }),
            "RunPlugin" => Some(QuartzAction::RunPlugin {
                name: "plugin_name".to_owned(),
                data: "payload".to_owned(),
            }),
            "Custom" => Some(QuartzAction::Custom {
                name: "custom_action".to_owned(),
            }),
            "CameraFlash" => Some(QuartzAction::CameraFlash {
                duration_s: 0.2,
                intensity: 0.75,
            }),
            "CameraShake" => Some(QuartzAction::CameraShake {
                intensity: 8.0,
                duration_s: 0.25,
            }),
            "CameraZoomPunch" => Some(QuartzAction::CameraZoomPunch {
                amount: 0.08,
                duration_s: 0.2,
            }),
            "Conditional" => Some(QuartzAction::Conditional {
                condition: QuartzCondition::Always,
                if_true: Box::new(QuartzAction::Custom {
                    name: "if_true".to_owned(),
                }),
                if_false: Some(Box::new(QuartzAction::Custom {
                    name: "if_false".to_owned(),
                })),
            }),
            "Multi" => Some(QuartzAction::Multi {
                actions: vec![QuartzAction::Custom {
                    name: "step_action".to_owned(),
                }],
            }),
            _ => None,
        };

        if let Some(next) = replacement {
            if std::mem::discriminant(action) != std::mem::discriminant(&next) {
                *action = next;
                changed = true;
            }
        }

        match action {
            QuartzAction::Teleport { target, location } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                match location {
                    QuartzLocationRef::At { x, y } => {
                        changed |= ui.add(Slider::new(x, -8000.0..=8000.0).text("x")).changed();
                        changed |= ui.add(Slider::new(y, -8000.0..=8000.0).text("y")).changed();
                    }
                    QuartzLocationRef::AtTarget(loc_target) => {
                        changed |= Self::edit_target_ref(ui, "location target", loc_target);
                    }
                }
            }
            QuartzAction::ApplyMomentum { target, mx, my }
            | QuartzAction::SetMomentum { target, mx, my } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(mx, -100.0..=100.0).text("mx")).changed();
                changed |= ui.add(Slider::new(my, -100.0..=100.0).text("my")).changed();
            }
            QuartzAction::SetResistance { target, rx, ry } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(rx, 0.0..=20.0).text("rx")).changed();
                changed |= ui.add(Slider::new(ry, 0.0..=20.0).text("ry")).changed();
            }
            QuartzAction::SetGravity { target, value } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(value, -20.0..=20.0).text("value")).changed();
            }
            QuartzAction::SetPivot { target, x, y } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(x, 0.0..=1.0).text("pivot x")).changed();
                changed |= ui.add(Slider::new(y, 0.0..=1.0).text("pivot y")).changed();
            }
            QuartzAction::SetRotation { target, deg }
            | QuartzAction::AddRotation { target, deg }
            | QuartzAction::ApplyRotation { target, deg } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(deg, -720.0..=720.0).text("deg")).changed();
            }
            QuartzAction::SetSize { target, w, h } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(w, 1.0..=4000.0).text("w")).changed();
                changed |= ui.add(Slider::new(h, 1.0..=4000.0).text("h")).changed();
            }
            QuartzAction::SetCollisionLayer { target, layer } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(layer, 0..=64).text("layer")).changed();
            }
            QuartzAction::SetCameraRelative { target, enabled } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.checkbox(enabled, "enabled").changed();
            }
            QuartzAction::SetRenderLayer { target, layer } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(layer, -100..=100).text("layer")).changed();
            }
            QuartzAction::Show { target }
            | QuartzAction::Hide { target }
            | QuartzAction::Toggle { target }
            | QuartzAction::Remove { target } => {
                changed |= Self::edit_target_ref(ui, "target", target);
            }
            QuartzAction::AddTag { target, tag } | QuartzAction::RemoveTag { target, tag } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                ui.label("tag");
                changed |= ui.text_edit_singleline(tag).changed();
            }
            QuartzAction::SetAnimation {
                target,
                animation_asset,
                fps,
            } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                ui.label("animation asset path (relative to project root)");
                changed |= ui.text_edit_singleline(animation_asset).changed();
                changed |= ui.add(Slider::new(fps, 1.0..=60.0).text("fps")).changed();
            }
            QuartzAction::SetZoom { value } | QuartzAction::SmoothZoom { value } => {
                changed |= ui.add(Slider::new(value, 0.1..=6.0).text("value")).changed();
            }
            QuartzAction::RunPlugin { name, data } => {
                ui.label("plugin name");
                changed |= ui.text_edit_singleline(name).changed();
                ui.label("plugin data payload");
                changed |= ui.text_edit_singleline(data).changed();
            }
            QuartzAction::Custom { name } => {
                ui.label("custom action name");
                changed |= ui.text_edit_singleline(name).changed();
            }
            QuartzAction::CameraFlash {
                duration_s,
                intensity,
            } => {
                changed |= ui
                    .add(Slider::new(duration_s, 0.01..=10.0).text("duration_s"))
                    .changed();
                changed |= ui
                    .add(Slider::new(intensity, 0.0..=1.0).text("intensity"))
                    .changed();
            }
            QuartzAction::CameraShake {
                intensity,
                duration_s,
            } => {
                changed |= ui
                    .add(Slider::new(intensity, 0.0..=40.0).text("intensity"))
                    .changed();
                changed |= ui
                    .add(Slider::new(duration_s, 0.01..=10.0).text("duration_s"))
                    .changed();
            }
            QuartzAction::CameraZoomPunch { amount, duration_s } => {
                changed |= ui
                    .add(Slider::new(amount, 0.0..=1.5).text("amount"))
                    .changed();
                changed |= ui
                    .add(Slider::new(duration_s, 0.01..=10.0).text("duration_s"))
                    .changed();
            }
            QuartzAction::Conditional {
                condition,
                if_true,
                if_false,
            } => {
                ui.label("condition");
                changed |= Self::edit_condition(ui, condition);

                ui.separator();
                ui.label("if_true action");
                changed |= Self::edit_action(ui, if_true);

                let mut has_else = if_false.is_some();
                if ui.checkbox(&mut has_else, "has if_false action").changed() {
                    if has_else && if_false.is_none() {
                        *if_false = Some(Box::new(QuartzAction::Custom {
                            name: "if_false".to_owned(),
                        }));
                    } else if !has_else {
                        *if_false = None;
                    }
                    changed = true;
                }
                if let Some(if_false) = if_false.as_mut() {
                    ui.label("if_false action");
                    changed |= Self::edit_action(ui, if_false);
                }
            }
            QuartzAction::Multi { actions } => {
                ui.label("multi action sequence");
                for (idx, entry) in actions.iter_mut().enumerate() {
                    ui.collapsing(format!("step {}", idx + 1), |ui| {
                        changed |= Self::edit_action(ui, entry);
                    });
                }
                if ui.button("+ add step").clicked() {
                    actions.push(QuartzAction::Custom {
                        name: format!("step_{}", actions.len() + 1),
                    });
                    changed = true;
                }
                if !actions.is_empty() && ui.button("- remove last step").clicked() {
                    actions.pop();
                    changed = true;
                }
            }
        }

        changed
    }

}
