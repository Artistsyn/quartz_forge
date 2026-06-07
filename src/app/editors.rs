use eframe::egui::{self, Slider};

use super::QuartzForgeApp;
use super::EditorSuggestions;
use crate::core::quartz_domain::{
    ObjectPhysicsMaterialPreset, ObjectPhysicsMaterialSpec, QuartzAction, QuartzCondition,
    QuartzExpr, QuartzExprKind, QuartzLocationRef, QuartzMathOp, QuartzTargetRef,
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

    pub(super) fn edit_action(
        ui: &mut egui::Ui,
        action: &mut QuartzAction,
        suggestions: &EditorSuggestions,
    ) -> bool {
        // Nested action editors reuse many labels; scope each call by pointer to avoid ID collisions.
        let action_id = action as *const QuartzAction as usize;
        let mut changed = false;
        ui.push_id(action_id, |ui| {
            changed = Self::edit_action_scoped(ui, action, suggestions);
        });
        changed
    }

    fn edit_action_scoped(
        ui: &mut egui::Ui,
        action: &mut QuartzAction,
        suggestions: &EditorSuggestions,
    ) -> bool {
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
            QuartzAction::PlaySound { .. } => "PlaySound",
            QuartzAction::SetZoom { .. } => "SetZoom",
            QuartzAction::SmoothZoom { .. } => "SmoothZoom",
            QuartzAction::PluginCall { .. } => "PluginCall",
            QuartzAction::RunPlugin { .. } => "RunPlugin",
            QuartzAction::Expr { .. } => "Expr",
            QuartzAction::Custom { .. } => "Custom",
            QuartzAction::CameraFlash { .. } => "CameraFlash",
            QuartzAction::CameraShake { .. } => "CameraShake",
            QuartzAction::CameraZoomPunch { .. } => "CameraZoomPunch",
            QuartzAction::SetMaterial { .. } => "SetMaterial",
            QuartzAction::SetDensity { .. } => "SetDensity",
            QuartzAction::SetElasticity { .. } => "SetElasticity",
            QuartzAction::SetFriction { .. } => "SetFriction",
            QuartzAction::ApplyForce { .. } => "ApplyForce",
            QuartzAction::ApplyImpulse { .. } => "ApplyImpulse",
            QuartzAction::SetPosition { .. } => "SetPosition",
            QuartzAction::FreezeBody { .. } => "FreezeBody",
            QuartzAction::UnfreezeBody { .. } => "UnfreezeBody",
            QuartzAction::WakeBody { .. } => "WakeBody",
            QuartzAction::SetPhysicsQuality { .. } => "SetPhysicsQuality",
            QuartzAction::SetCollisionMode { .. } => "SetCollisionMode",
            QuartzAction::SetSlope { .. } => "SetSlope",
            QuartzAction::SetSurfaceNormal { .. } => "SetSurfaceNormal",
            QuartzAction::TransferMomentum { .. } => "TransferMomentum",
            QuartzAction::Spawn { .. } => "Spawn",
            QuartzAction::EnableCrystalline { .. } => "EnableCrystalline",
            QuartzAction::DisableCrystalline { .. } => "DisableCrystalline",
            QuartzAction::SetGravityStrength { .. } => "SetGravityStrength",
            QuartzAction::SetPlanetRadius { .. } => "SetPlanetRadius",
            QuartzAction::SetGravityTarget { .. } => "SetGravityTarget",
            QuartzAction::SetGravityInfluenceMult { .. } => "SetGravityInfluenceMult",
            QuartzAction::SetGravityFalloff { .. } => "SetGravityFalloff",
            QuartzAction::SetGravityAllSources { .. } => "SetGravityAllSources",
            QuartzAction::SetAlignToSlope { .. } => "SetAlignToSlope",
            QuartzAction::SetAlignToSlopeSpeed { .. } => "SetAlignToSlopeSpeed",
            QuartzAction::SpawnEmitter { .. } => "SpawnEmitter",
            QuartzAction::RemoveEmitter { .. } => "RemoveEmitter",
            QuartzAction::AttachEmitter { .. } => "AttachEmitter",
            QuartzAction::DetachEmitter { .. } => "DetachEmitter",
            QuartzAction::SetEmitterRate { .. } => "SetEmitterRate",
            QuartzAction::SetEmitterLifetime { .. } => "SetEmitterLifetime",
            QuartzAction::SetEmitterVelocity { .. } => "SetEmitterVelocity",
            QuartzAction::SetEmitterSpread { .. } => "SetEmitterSpread",
            QuartzAction::SetEmitterSize { .. } => "SetEmitterSize",
            QuartzAction::SetEmitterColor { .. } => "SetEmitterColor",
            QuartzAction::SetEmitterGravityScale { .. } => "SetEmitterGravityScale",
            QuartzAction::SetEmitterCollision { .. } => "SetEmitterCollision",
            QuartzAction::SetEmitterRenderLayer { .. } => "SetEmitterRenderLayer",
            QuartzAction::SetEmitterSizeEnd { .. } => "SetEmitterSizeEnd",
            QuartzAction::SetEmitterColorEnd { .. } => "SetEmitterColorEnd",
            QuartzAction::SetEmitterShape { .. } => "SetEmitterShape",
            QuartzAction::SetEmitterAlignToVelocity { .. } => "SetEmitterAlignToVelocity",
            QuartzAction::SetEmitterInterpolatePosition { .. } => "SetEmitterInterpolatePosition",
            QuartzAction::AddZoom { .. } => "AddZoom",
            QuartzAction::SmoothZoomAt { .. } => "SmoothZoomAt",
            QuartzAction::CameraFlashWith { .. } => "CameraFlashWith",
            QuartzAction::SetGlow { .. } => "SetGlow",
            QuartzAction::ClearGlow { .. } => "ClearGlow",
            QuartzAction::SetTint { .. } => "SetTint",
            QuartzAction::ClearTint { .. } => "ClearTint",
            QuartzAction::SetVar { .. } => "SetVar",
            QuartzAction::ModVar { .. } => "ModVar",
            QuartzAction::SpawnObject { .. } => "SpawnObject",
            QuartzAction::SetText { .. } => "SetText",
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
                changed |= ui
                    .selectable_value(&mut action_kind, "PlaySound", "PlaySound")
                    .changed();
                changed |= ui.selectable_value(&mut action_kind, "SetZoom", "SetZoom").changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SmoothZoom", "SmoothZoom")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "PluginCall", "PluginCall")
                    .changed();
                changed |= ui.selectable_value(&mut action_kind, "RunPlugin", "RunPlugin").changed();
                changed |= ui.selectable_value(&mut action_kind, "Expr", "Expr").changed();
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
                    .selectable_value(&mut action_kind, "SetMaterial", "SetMaterial")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetDensity", "SetDensity")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetElasticity", "SetElasticity")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetFriction", "SetFriction")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "ApplyForce", "ApplyForce")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "ApplyImpulse", "ApplyImpulse")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetPosition", "SetPosition")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "FreezeBody", "FreezeBody")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "UnfreezeBody", "UnfreezeBody")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "WakeBody", "WakeBody")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetPhysicsQuality", "SetPhysicsQuality")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetCollisionMode", "SetCollisionMode")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetSlope", "SetSlope")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetSurfaceNormal", "SetSurfaceNormal")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "TransferMomentum", "TransferMomentum")
                    .changed();
                changed |= ui.selectable_value(&mut action_kind, "Spawn", "Spawn").changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "EnableCrystalline",
                        "EnableCrystalline",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "DisableCrystalline",
                        "DisableCrystalline",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetGravityStrength",
                        "SetGravityStrength",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetPlanetRadius",
                        "SetPlanetRadius",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetGravityTarget",
                        "SetGravityTarget",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetGravityInfluenceMult",
                        "SetGravityInfluenceMult",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetGravityFalloff",
                        "SetGravityFalloff",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetGravityAllSources",
                        "SetGravityAllSources",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetAlignToSlope",
                        "SetAlignToSlope",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetAlignToSlopeSpeed",
                        "SetAlignToSlopeSpeed",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SpawnEmitter", "SpawnEmitter")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "RemoveEmitter", "RemoveEmitter")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "AttachEmitter", "AttachEmitter")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "DetachEmitter", "DetachEmitter")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetEmitterRate", "SetEmitterRate")
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetEmitterLifetime",
                        "SetEmitterLifetime",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetEmitterVelocity",
                        "SetEmitterVelocity",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetEmitterSpread", "SetEmitterSpread")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetEmitterSize", "SetEmitterSize")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetEmitterColor", "SetEmitterColor")
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetEmitterGravityScale",
                        "SetEmitterGravityScale",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetEmitterCollision",
                        "SetEmitterCollision",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetEmitterRenderLayer",
                        "SetEmitterRenderLayer",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetEmitterSizeEnd",
                        "SetEmitterSizeEnd",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetEmitterColorEnd",
                        "SetEmitterColorEnd",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetEmitterShape", "SetEmitterShape")
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetEmitterAlignToVelocity",
                        "SetEmitterAlignToVelocity",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut action_kind,
                        "SetEmitterInterpolatePosition",
                        "SetEmitterInterpolatePosition",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "AddZoom", "AddZoom")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SmoothZoomAt", "SmoothZoomAt")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "CameraFlashWith", "CameraFlashWith")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetGlow", "SetGlow")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "ClearGlow", "ClearGlow")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SetTint", "SetTint")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "ClearTint", "ClearTint")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "Conditional", "Conditional")
                    .changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "Multi", "Multi")
                    .changed();
                changed |= ui.selectable_value(&mut action_kind, "SetVar", "SetVar").changed();
                changed |= ui.selectable_value(&mut action_kind, "ModVar", "ModVar").changed();
                changed |= ui
                    .selectable_value(&mut action_kind, "SpawnObject", "SpawnObject")
                    .changed();
                changed |= ui.selectable_value(&mut action_kind, "SetText", "SetText").changed();
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
            "PlaySound" => Some(QuartzAction::PlaySound {
                path: "assets/audio/sfx.ogg".to_owned(),
                volume: 0.25,
                looping: false,
            }),
            "SetZoom" => Some(QuartzAction::SetZoom { value: 1.0 }),
            "SmoothZoom" => Some(QuartzAction::SmoothZoom { value: 1.0 }),
            "PluginCall" => Some(QuartzAction::PluginCall {
                name: "plugin_name".to_owned(),
                payload: "payload".to_owned(),
            }),
            "RunPlugin" => Some(QuartzAction::RunPlugin {
                name: "plugin_name".to_owned(),
                data: "payload".to_owned(),
            }),
            "Expr" => Some(QuartzAction::Expr {
                raw: "player_x > 0.0".to_owned(),
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
            "SetMaterial" => Some(QuartzAction::SetMaterial {
                target: QuartzTargetRef::Name("player".to_owned()),
                material: ObjectPhysicsMaterialSpec::resolved_defaults(
                    ObjectPhysicsMaterialPreset::Default,
                ),
            }),
            "SetDensity" => Some(QuartzAction::SetDensity {
                target: QuartzTargetRef::Name("player".to_owned()),
                value: 1.0,
            }),
            "SetElasticity" => Some(QuartzAction::SetElasticity {
                target: QuartzTargetRef::Name("player".to_owned()),
                value: 0.5,
            }),
            "SetFriction" => Some(QuartzAction::SetFriction {
                target: QuartzTargetRef::Name("player".to_owned()),
                value: 0.5,
            }),
            "ApplyForce" => Some(QuartzAction::ApplyForce {
                target: QuartzTargetRef::Name("player".to_owned()),
                fx: 0.0,
                fy: -10.0,
            }),
            "ApplyImpulse" => Some(QuartzAction::ApplyImpulse {
                target: QuartzTargetRef::Name("player".to_owned()),
                ix: 0.0,
                iy: -12.0,
            }),
            "SetPosition" => Some(QuartzAction::SetPosition {
                target: QuartzTargetRef::Name("player".to_owned()),
                x: 400.0,
                y: 300.0,
            }),
            "FreezeBody" => Some(QuartzAction::FreezeBody {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "UnfreezeBody" => Some(QuartzAction::UnfreezeBody {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "WakeBody" => Some(QuartzAction::WakeBody {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "SetPhysicsQuality" => Some(QuartzAction::SetPhysicsQuality {
                quality: "Medium".to_owned(),
            }),
            "SetCollisionMode" => Some(QuartzAction::SetCollisionMode {
                target: QuartzTargetRef::Name("player".to_owned()),
                mode: "Solid".to_owned(),
            }),
            "SetSlope" => Some(QuartzAction::SetSlope {
                target: QuartzTargetRef::Name("player".to_owned()),
                left_offset: -8.0,
                right_offset: 8.0,
                auto_rotate: true,
            }),
            "SetSurfaceNormal" => Some(QuartzAction::SetSurfaceNormal {
                target: QuartzTargetRef::Name("player".to_owned()),
                nx: 0.0,
                ny: -1.0,
            }),
            "TransferMomentum" => Some(QuartzAction::TransferMomentum {
                from: QuartzTargetRef::Name("player".to_owned()),
                to: QuartzTargetRef::Name("crate".to_owned()),
                scale: 1.0,
            }),
            "Spawn" => Some(QuartzAction::Spawn {
                template_id: "enemy".to_owned(),
                location: QuartzLocationRef::At { x: 400.0, y: 300.0 },
            }),
            "EnableCrystalline" => Some(QuartzAction::EnableCrystalline),
            "DisableCrystalline" => Some(QuartzAction::DisableCrystalline),
            "SetGravityStrength" => Some(QuartzAction::SetGravityStrength {
                target: QuartzTargetRef::Name("player".to_owned()),
                value: 9.8,
            }),
            "SetPlanetRadius" => Some(QuartzAction::SetPlanetRadius {
                target: QuartzTargetRef::Name("player".to_owned()),
                value: 128.0,
            }),
            "SetGravityTarget" => Some(QuartzAction::SetGravityTarget {
                target: QuartzTargetRef::Name("player".to_owned()),
                tag: "planet".to_owned(),
            }),
            "SetGravityInfluenceMult" => Some(QuartzAction::SetGravityInfluenceMult {
                target: QuartzTargetRef::Name("player".to_owned()),
                value: 1.0,
            }),
            "SetGravityFalloff" => Some(QuartzAction::SetGravityFalloff {
                target: QuartzTargetRef::Name("player".to_owned()),
                falloff: "Linear".to_owned(),
            }),
            "SetGravityAllSources" => Some(QuartzAction::SetGravityAllSources {
                target: QuartzTargetRef::Name("player".to_owned()),
                enabled: true,
            }),
            "SetAlignToSlope" => Some(QuartzAction::SetAlignToSlope {
                target: QuartzTargetRef::Name("player".to_owned()),
                enabled: true,
            }),
            "SetAlignToSlopeSpeed" => Some(QuartzAction::SetAlignToSlopeSpeed {
                target: QuartzTargetRef::Name("player".to_owned()),
                value: 1.0,
            }),
            "SpawnEmitter" => Some(QuartzAction::SpawnEmitter {
                name: "sparkles".to_owned(),
            }),
            "RemoveEmitter" => Some(QuartzAction::RemoveEmitter {
                name: "sparkles".to_owned(),
            }),
            "AttachEmitter" => Some(QuartzAction::AttachEmitter {
                emitter_name: "sparkles".to_owned(),
                target: QuartzTargetRef::Name("player".to_owned()),
                location: None,
            }),
            "DetachEmitter" => Some(QuartzAction::DetachEmitter {
                emitter_name: "sparkles".to_owned(),
            }),
            "SetEmitterRate" => Some(QuartzAction::SetEmitterRate {
                name: "sparkles".to_owned(),
                value: 20.0,
            }),
            "SetEmitterLifetime" => Some(QuartzAction::SetEmitterLifetime {
                name: "sparkles".to_owned(),
                value: 1.0,
            }),
            "SetEmitterVelocity" => Some(QuartzAction::SetEmitterVelocity {
                name: "sparkles".to_owned(),
                x: 0.0,
                y: -40.0,
            }),
            "SetEmitterSpread" => Some(QuartzAction::SetEmitterSpread {
                name: "sparkles".to_owned(),
                x: 20.0,
                y: 20.0,
            }),
            "SetEmitterSize" => Some(QuartzAction::SetEmitterSize {
                name: "sparkles".to_owned(),
                value: 6.0,
            }),
            "SetEmitterColor" => Some(QuartzAction::SetEmitterColor {
                name: "sparkles".to_owned(),
                rgba: [255, 220, 120, 255],
            }),
            "SetEmitterGravityScale" => Some(QuartzAction::SetEmitterGravityScale {
                name: "sparkles".to_owned(),
                value: 1.0,
            }),
            "SetEmitterCollision" => Some(QuartzAction::SetEmitterCollision {
                name: "sparkles".to_owned(),
                mode: "None".to_owned(),
            }),
            "SetEmitterRenderLayer" => Some(QuartzAction::SetEmitterRenderLayer {
                name: "sparkles".to_owned(),
                value: 1,
            }),
            "SetEmitterSizeEnd" => Some(QuartzAction::SetEmitterSizeEnd {
                name: "sparkles".to_owned(),
                value: 0.0,
            }),
            "SetEmitterColorEnd" => Some(QuartzAction::SetEmitterColorEnd {
                name: "sparkles".to_owned(),
                rgba: Some([255, 180, 80, 0]),
            }),
            "SetEmitterShape" => Some(QuartzAction::SetEmitterShape {
                name: "sparkles".to_owned(),
                shape: "Circle".to_owned(),
            }),
            "SetEmitterAlignToVelocity" => Some(QuartzAction::SetEmitterAlignToVelocity {
                name: "sparkles".to_owned(),
                enabled: true,
            }),
            "SetEmitterInterpolatePosition" => {
                Some(QuartzAction::SetEmitterInterpolatePosition {
                    name: "sparkles".to_owned(),
                    enabled: true,
                })
            }
            "AddZoom" => Some(QuartzAction::AddZoom { value: 0.1 }),
            "SmoothZoomAt" => Some(QuartzAction::SmoothZoomAt { delta: 0.08 }),
            "CameraFlashWith" => Some(QuartzAction::CameraFlashWith {
                color_rgba: [255, 255, 255, 255],
                duration_s: 0.2,
                mode: "Add".to_owned(),
                ease: "OutQuad".to_owned(),
                intensity: 1.0,
                freeze_frame_s: 0.0,
            }),
            "SetGlow" => Some(QuartzAction::SetGlow {
                target: QuartzTargetRef::Name("player".to_owned()),
                color_rgb: [120, 220, 255],
                width: 3.0,
            }),
            "ClearGlow" => Some(QuartzAction::ClearGlow {
                target: QuartzTargetRef::Name("player".to_owned()),
            }),
            "SetTint" => Some(QuartzAction::SetTint {
                target: QuartzTargetRef::Name("player".to_owned()),
                color_rgba: [255, 180, 180, 255],
            }),
            "ClearTint" => Some(QuartzAction::ClearTint {
                target: QuartzTargetRef::Name("player".to_owned()),
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
            "SetVar" => Some(QuartzAction::SetVar {
                name: "my_var".to_owned(),
                value: QuartzExpr::default(),
            }),
            "ModVar" => Some(QuartzAction::ModVar {
                name: "score".to_owned(),
                op: QuartzMathOp::Add,
                operand: QuartzExpr { kind: QuartzExprKind::I32, raw: "1".to_owned() },
            }),
            "SpawnObject" => Some(QuartzAction::SpawnObject {
                template_id: "enemy".to_owned(),
                location: QuartzLocationRef::At { x: 400.0, y: 300.0 },
            }),
            "SetText" => Some(QuartzAction::SetText {
                target: QuartzTargetRef::Name("label".to_owned()),
                content: "Hello".to_owned(),
                font_size: 24.0,
                color_rgb: [255, 255, 255],
                font_asset_path: String::new(),
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
            QuartzAction::EnableCrystalline | QuartzAction::DisableCrystalline => {}
            QuartzAction::SetGravityStrength { target, value }
            | QuartzAction::SetPlanetRadius { target, value }
            | QuartzAction::SetGravityInfluenceMult { target, value }
            | QuartzAction::SetAlignToSlopeSpeed { target, value } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(value, 0.0..=100.0).text("value")).changed();
            }
            QuartzAction::SetGravityTarget { target, tag } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                ui.label("gravity tag");
                changed |= ui.text_edit_singleline(tag).changed();
            }
            QuartzAction::SetGravityFalloff { target, falloff } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                ui.label("gravity falloff");
                changed |= ui.text_edit_singleline(falloff).changed();
            }
            QuartzAction::SetGravityAllSources { target, enabled }
            | QuartzAction::SetAlignToSlope { target, enabled } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.checkbox(enabled, "enabled").changed();
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
            QuartzAction::PlaySound {
                path,
                volume,
                looping,
            } => {
                ui.label("sound asset path (relative to project root)");
                changed |= ui.text_edit_singleline(path).changed();
                changed |= ui.add(Slider::new(volume, 0.0..=1.0).text("volume")).changed();
                changed |= ui.checkbox(looping, "looping").changed();
            }
            QuartzAction::SetZoom { value } | QuartzAction::SmoothZoom { value } => {
                changed |= ui.add(Slider::new(value, 0.1..=6.0).text("value")).changed();
            }
            QuartzAction::PluginCall { name, payload } => {
                ui.label("plugin name");
                changed |= ui.text_edit_singleline(name).changed();
                ui.label("plugin payload");
                changed |= ui.text_edit_singleline(payload).changed();
            }
            QuartzAction::RunPlugin { name, data } => {
                ui.label("plugin name");
                changed |= ui.text_edit_singleline(name).changed();
                ui.label("plugin data payload");
                changed |= ui.text_edit_singleline(data).changed();
            }
            QuartzAction::Expr { raw } => {
                ui.label("action expr source");
                changed |= ui.text_edit_singleline(raw).changed();
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
            QuartzAction::SetMaterial { target, material } => {
                changed |= Self::edit_target_ref(ui, "target", target);

                let mut preset = material.preset;
                egui::ComboBox::from_label("material preset")
                    .selected_text(preset.as_str())
                    .show_ui(ui, |ui| {
                        changed |= ui
                            .selectable_value(
                                &mut preset,
                                ObjectPhysicsMaterialPreset::Default,
                                "Default",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut preset,
                                ObjectPhysicsMaterialPreset::Rubber,
                                "Rubber",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(&mut preset, ObjectPhysicsMaterialPreset::Ice, "Ice")
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut preset,
                                ObjectPhysicsMaterialPreset::Metal,
                                "Metal",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut preset,
                                ObjectPhysicsMaterialPreset::Wood,
                                "Wood",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut preset,
                                ObjectPhysicsMaterialPreset::Stone,
                                "Stone",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut preset,
                                ObjectPhysicsMaterialPreset::Bouncy,
                                "Bouncy",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut preset,
                                ObjectPhysicsMaterialPreset::Sticky,
                                "Sticky",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut preset,
                                ObjectPhysicsMaterialPreset::Glass,
                                "Glass",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut preset,
                                ObjectPhysicsMaterialPreset::Feather,
                                "Feather",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut preset,
                                ObjectPhysicsMaterialPreset::Custom,
                                "Custom",
                            )
                            .changed();
                    });
                if preset != material.preset {
                    *material = ObjectPhysicsMaterialSpec::resolved_defaults(preset);
                    changed = true;
                }

                changed |= ui
                    .add(Slider::new(&mut material.elasticity, 0.0..=2.0).text("elasticity"))
                    .changed();
                changed |= ui
                    .add(Slider::new(&mut material.friction, 0.0..=2.0).text("friction"))
                    .changed();
                changed |= ui
                    .add(Slider::new(&mut material.density, 0.0..=20.0).text("density"))
                    .changed();
            }
            QuartzAction::SetDensity { target, value }
            | QuartzAction::SetElasticity { target, value }
            | QuartzAction::SetFriction { target, value } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(value, 0.0..=20.0).text("value")).changed();
            }
            QuartzAction::ApplyForce { target, fx, fy } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(fx, -5000.0..=5000.0).text("fx")).changed();
                changed |= ui.add(Slider::new(fy, -5000.0..=5000.0).text("fy")).changed();
            }
            QuartzAction::ApplyImpulse { target, ix, iy } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(ix, -5000.0..=5000.0).text("ix")).changed();
                changed |= ui.add(Slider::new(iy, -5000.0..=5000.0).text("iy")).changed();
            }
            QuartzAction::SetPosition { target, x, y } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(x, -8000.0..=8000.0).text("x")).changed();
                changed |= ui.add(Slider::new(y, -8000.0..=8000.0).text("y")).changed();
            }
            QuartzAction::FreezeBody { target }
            | QuartzAction::UnfreezeBody { target }
            | QuartzAction::WakeBody { target } => {
                changed |= Self::edit_target_ref(ui, "target", target);
            }
            QuartzAction::SetPhysicsQuality { quality } => {
                ui.label("quality (Low/Medium/High)");
                changed |= ui.text_edit_singleline(quality).changed();
            }
            QuartzAction::SetCollisionMode { target, mode } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                ui.label("mode (Solid/Sensor/Disabled)");
                changed |= ui.text_edit_singleline(mode).changed();
            }
            QuartzAction::SetSlope {
                target,
                left_offset,
                right_offset,
                auto_rotate,
            } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui
                    .add(Slider::new(left_offset, -1000.0..=1000.0).text("left_offset"))
                    .changed();
                changed |= ui
                    .add(Slider::new(right_offset, -1000.0..=1000.0).text("right_offset"))
                    .changed();
                changed |= ui.checkbox(auto_rotate, "auto_rotate").changed();
            }
            QuartzAction::SetSurfaceNormal { target, nx, ny } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(nx, -2.0..=2.0).text("nx")).changed();
                changed |= ui.add(Slider::new(ny, -2.0..=2.0).text("ny")).changed();
            }
            QuartzAction::TransferMomentum { from, to, scale } => {
                changed |= Self::edit_target_ref(ui, "from", from);
                changed |= Self::edit_target_ref(ui, "to", to);
                changed |= ui.add(Slider::new(scale, 0.0..=5.0).text("scale")).changed();
            }
            QuartzAction::SpawnEmitter { name }
            | QuartzAction::RemoveEmitter { name } => {
                ui.label("emitter name");
                changed |= ui.text_edit_singleline(name).changed();
            }
            QuartzAction::AttachEmitter {
                emitter_name,
                target,
                location,
            } => {
                ui.label("emitter name");
                changed |= ui.text_edit_singleline(emitter_name).changed();
                changed |= Self::edit_target_ref(ui, "target", target);
                let mut use_target_location = matches!(location, Some(QuartzLocationRef::AtTarget(_)));
                if ui.checkbox(&mut use_target_location, "use target location override").changed() {
                    if use_target_location {
                        *location = Some(QuartzLocationRef::AtTarget(QuartzTargetRef::Name(
                            "player".to_owned(),
                        )));
                    } else {
                        *location = None;
                    }
                    changed = true;
                }
                if let Some(QuartzLocationRef::AtTarget(t)) = location {
                    changed |= Self::edit_target_ref(ui, "location target", t);
                }
            }
            QuartzAction::DetachEmitter { emitter_name } => {
                ui.label("emitter name");
                changed |= ui.text_edit_singleline(emitter_name).changed();
            }
            QuartzAction::SetEmitterRate { name, value }
            | QuartzAction::SetEmitterLifetime { name, value }
            | QuartzAction::SetEmitterSize { name, value }
            | QuartzAction::SetEmitterGravityScale { name, value }
            | QuartzAction::SetEmitterSizeEnd { name, value } => {
                ui.label("emitter name");
                changed |= ui.text_edit_singleline(name).changed();
                changed |= ui.add(Slider::new(value, -1000.0..=1000.0).text("value")).changed();
            }
            QuartzAction::SetEmitterVelocity { name, x, y }
            | QuartzAction::SetEmitterSpread { name, x, y } => {
                ui.label("emitter name");
                changed |= ui.text_edit_singleline(name).changed();
                changed |= ui.add(Slider::new(x, -1000.0..=1000.0).text("x")).changed();
                changed |= ui.add(Slider::new(y, -1000.0..=1000.0).text("y")).changed();
            }
            QuartzAction::SetEmitterColor { name, rgba } => {
                ui.label("emitter name");
                changed |= ui.text_edit_singleline(name).changed();
                let mut r = rgba[0] as f32;
                let mut g = rgba[1] as f32;
                let mut b = rgba[2] as f32;
                let mut a = rgba[3] as f32;
                if ui.add(Slider::new(&mut r, 0.0..=255.0).text("R")).changed() {
                    rgba[0] = r as u8;
                    changed = true;
                }
                if ui.add(Slider::new(&mut g, 0.0..=255.0).text("G")).changed() {
                    rgba[1] = g as u8;
                    changed = true;
                }
                if ui.add(Slider::new(&mut b, 0.0..=255.0).text("B")).changed() {
                    rgba[2] = b as u8;
                    changed = true;
                }
                if ui.add(Slider::new(&mut a, 0.0..=255.0).text("A")).changed() {
                    rgba[3] = a as u8;
                    changed = true;
                }
            }
            QuartzAction::SetEmitterCollision { name, mode }
            | QuartzAction::SetEmitterShape { name, shape: mode } => {
                ui.label("emitter name");
                changed |= ui.text_edit_singleline(name).changed();
                changed |= ui.text_edit_singleline(mode).changed();
            }
            QuartzAction::SetEmitterRenderLayer { name, value } => {
                ui.label("emitter name");
                changed |= ui.text_edit_singleline(name).changed();
                changed |= ui.add(Slider::new(value, -100..=100).text("value")).changed();
            }
            QuartzAction::SetEmitterColorEnd { name, rgba } => {
                ui.label("emitter name");
                changed |= ui.text_edit_singleline(name).changed();
                let mut has_color = rgba.is_some();
                if ui.checkbox(&mut has_color, "set end color").changed() {
                    if has_color && rgba.is_none() {
                        *rgba = Some([255, 255, 255, 0]);
                    }
                    if !has_color {
                        *rgba = None;
                    }
                    changed = true;
                }
                if let Some(rgba) = rgba.as_mut() {
                    let mut r = rgba[0] as f32;
                    let mut g = rgba[1] as f32;
                    let mut b = rgba[2] as f32;
                    let mut a = rgba[3] as f32;
                    if ui.add(Slider::new(&mut r, 0.0..=255.0).text("R")).changed() {
                        rgba[0] = r as u8;
                        changed = true;
                    }
                    if ui.add(Slider::new(&mut g, 0.0..=255.0).text("G")).changed() {
                        rgba[1] = g as u8;
                        changed = true;
                    }
                    if ui.add(Slider::new(&mut b, 0.0..=255.0).text("B")).changed() {
                        rgba[2] = b as u8;
                        changed = true;
                    }
                    if ui.add(Slider::new(&mut a, 0.0..=255.0).text("A")).changed() {
                        rgba[3] = a as u8;
                        changed = true;
                    }
                }
            }
            QuartzAction::SetEmitterAlignToVelocity { name, enabled }
            | QuartzAction::SetEmitterInterpolatePosition { name, enabled } => {
                ui.label("emitter name");
                changed |= ui.text_edit_singleline(name).changed();
                changed |= ui.checkbox(enabled, "enabled").changed();
            }
            QuartzAction::AddZoom { value } => {
                changed |= ui.add(Slider::new(value, -2.0..=2.0).text("value")).changed();
            }
            QuartzAction::SmoothZoomAt { delta } => {
                changed |= ui.add(Slider::new(delta, -2.0..=2.0).text("delta")).changed();
            }
            QuartzAction::CameraFlashWith {
                color_rgba,
                duration_s,
                mode,
                ease,
                intensity,
                freeze_frame_s,
            } => {
                changed |= ui
                    .add(Slider::new(duration_s, 0.01..=10.0).text("duration_s"))
                    .changed();
                changed |= ui
                    .add(Slider::new(intensity, 0.0..=2.0).text("intensity"))
                    .changed();
                changed |= ui
                    .add(Slider::new(freeze_frame_s, 0.0..=0.5).text("freeze_frame_s"))
                    .changed();
                ui.label("mode");
                changed |= ui.text_edit_singleline(mode).changed();
                ui.label("ease");
                changed |= ui.text_edit_singleline(ease).changed();
                let mut r = color_rgba[0] as f32;
                let mut g = color_rgba[1] as f32;
                let mut b = color_rgba[2] as f32;
                let mut a = color_rgba[3] as f32;
                if ui.add(Slider::new(&mut r, 0.0..=255.0).text("R")).changed() {
                    color_rgba[0] = r as u8;
                    changed = true;
                }
                if ui.add(Slider::new(&mut g, 0.0..=255.0).text("G")).changed() {
                    color_rgba[1] = g as u8;
                    changed = true;
                }
                if ui.add(Slider::new(&mut b, 0.0..=255.0).text("B")).changed() {
                    color_rgba[2] = b as u8;
                    changed = true;
                }
                if ui.add(Slider::new(&mut a, 0.0..=255.0).text("A")).changed() {
                    color_rgba[3] = a as u8;
                    changed = true;
                }
            }
            QuartzAction::SetGlow {
                target,
                color_rgb,
                width,
            } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                changed |= ui.add(Slider::new(width, 0.0..=20.0).text("width")).changed();
                let mut r = color_rgb[0] as f32;
                let mut g = color_rgb[1] as f32;
                let mut b = color_rgb[2] as f32;
                if ui.add(Slider::new(&mut r, 0.0..=255.0).text("R")).changed() {
                    color_rgb[0] = r as u8;
                    changed = true;
                }
                if ui.add(Slider::new(&mut g, 0.0..=255.0).text("G")).changed() {
                    color_rgb[1] = g as u8;
                    changed = true;
                }
                if ui.add(Slider::new(&mut b, 0.0..=255.0).text("B")).changed() {
                    color_rgb[2] = b as u8;
                    changed = true;
                }
            }
            QuartzAction::ClearGlow { target } => {
                changed |= Self::edit_target_ref(ui, "target", target);
            }
            QuartzAction::SetTint { target, color_rgba } => {
                changed |= Self::edit_target_ref(ui, "target", target);
                let mut r = color_rgba[0] as f32;
                let mut g = color_rgba[1] as f32;
                let mut b = color_rgba[2] as f32;
                let mut a = color_rgba[3] as f32;
                if ui.add(Slider::new(&mut r, 0.0..=255.0).text("R")).changed() {
                    color_rgba[0] = r as u8;
                    changed = true;
                }
                if ui.add(Slider::new(&mut g, 0.0..=255.0).text("G")).changed() {
                    color_rgba[1] = g as u8;
                    changed = true;
                }
                if ui.add(Slider::new(&mut b, 0.0..=255.0).text("B")).changed() {
                    color_rgba[2] = b as u8;
                    changed = true;
                }
                if ui.add(Slider::new(&mut a, 0.0..=255.0).text("A")).changed() {
                    color_rgba[3] = a as u8;
                    changed = true;
                }
            }
            QuartzAction::ClearTint { target } => {
                changed |= Self::edit_target_ref(ui, "target", target);
            }
            QuartzAction::Conditional {
                condition,
                if_true,
                if_false,
            } => {
                ui.label("condition");
                changed |= Self::edit_condition(ui, condition, suggestions);

                ui.separator();
                ui.label("if_true action");
                changed |= Self::edit_action(ui, if_true, suggestions);

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
                    changed |= Self::edit_action(ui, if_false, suggestions);
                }
            }
            QuartzAction::Multi { actions } => {
                ui.label("multi action sequence");
                for (idx, entry) in actions.iter_mut().enumerate() {
                    ui.collapsing(format!("step {}", idx + 1), |ui| {
                        changed |= Self::edit_action(ui, entry, suggestions);
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
            QuartzAction::SetVar { name, value } => {
                ui.label("variable name");
                changed |= Self::suggestion_text_edit(
                    ui,
                    "action_set_var_name",
                    name,
                    &suggestions.variable_names,
                );
                Self::edit_quartz_expr(ui, "value", value, &mut changed, suggestions);
            }
            QuartzAction::ModVar { name, op, operand } => {
                ui.label("variable name");
                changed |= Self::suggestion_text_edit(
                    ui,
                    "action_mod_var_name",
                    name,
                    &suggestions.variable_names,
                );
                let mut op_kind = match op {
                    QuartzMathOp::Add => "+=",
                    QuartzMathOp::Sub => "-=",
                    QuartzMathOp::Mul => "*=",
                    QuartzMathOp::Div => "/=",
                };
                egui::ComboBox::from_label("op")
                    .selected_text(op_kind)
                    .show_ui(ui, |ui| {
                        changed |= ui.selectable_value(&mut op_kind, "+=", "+=").changed();
                        changed |= ui.selectable_value(&mut op_kind, "-=", "-=").changed();
                        changed |= ui.selectable_value(&mut op_kind, "*=", "*=").changed();
                        changed |= ui.selectable_value(&mut op_kind, "/=", "/=").changed();
                    });
                let next_op = match op_kind {
                    "+=" => QuartzMathOp::Add,
                    "-=" => QuartzMathOp::Sub,
                    "*=" => QuartzMathOp::Mul,
                    "/=" => QuartzMathOp::Div,
                    _ => QuartzMathOp::Add,
                };
                if *op != next_op { *op = next_op; changed = true; }
                Self::edit_quartz_expr(ui, "operand", operand, &mut changed, suggestions);
            }
            QuartzAction::Spawn { template_id, location }
            | QuartzAction::SpawnObject { template_id, location } => {
                ui.label("template object id (must be a spawn-only object)");
                changed |= ui.text_edit_singleline(template_id).changed();
                match location {
                    QuartzLocationRef::At { x, y } => {
                        changed |= ui.add(Slider::new(x, -8000.0..=8000.0).text("x")).changed();
                        changed |= ui.add(Slider::new(y, -8000.0..=8000.0).text("y")).changed();
                    }
                    QuartzLocationRef::AtTarget(t) => {
                        changed |= Self::edit_target_ref(ui, "at target", t);
                    }
                }
            }
            QuartzAction::SetText { target, content, font_size, color_rgb, font_asset_path } => {
                ui.label("SetText emits Text::new + Span::new. Optional font path enables cached include_bytes font loading.");
                changed |= Self::edit_target_ref(ui, "target", target);
                ui.label("text content");
                changed |= ui.text_edit_singleline(content).changed();
                changed |= ui.add(Slider::new(font_size, 6.0..=128.0).text("font_size")).changed();
                ui.label("font asset path (optional, relative to project root)");
                changed |= ui.text_edit_singleline(font_asset_path).changed();
                ui.label("color RGB");
                let mut r = color_rgb[0] as f32;
                let mut g = color_rgb[1] as f32;
                let mut b = color_rgb[2] as f32;
                if ui.add(Slider::new(&mut r, 0.0..=255.0).text("R")).changed() {
                    color_rgb[0] = r as u8; changed = true;
                }
                if ui.add(Slider::new(&mut g, 0.0..=255.0).text("G")).changed() {
                    color_rgb[1] = g as u8; changed = true;
                }
                if ui.add(Slider::new(&mut b, 0.0..=255.0).text("B")).changed() {
                    color_rgb[2] = b as u8; changed = true;
                }
            }
        }

        if let Err(message) = Self::validate_physics_action_ranges(action) {
            ui.colored_label(egui::Color32::from_rgb(220, 70, 70), message);
        }

        if let Some(message) = Self::setposition_semantic_warning(action) {
            ui.colored_label(egui::Color32::from_rgb(220, 170, 70), message);
        }

        changed
    }

    fn setposition_semantic_warning(action: &QuartzAction) -> Option<&'static str> {
        match action {
            QuartzAction::SetPosition { .. } => Some(
                "SetPosition resets momentum semantics at runtime; use Teleport or ApplyMomentum/SetMomentum for movement intent.",
            ),
            _ => None,
        }
    }

    fn validate_physics_action_ranges(action: &QuartzAction) -> Result<(), String> {
        match action {
            QuartzAction::SetMaterial { material, .. } => {
                if material.density < 0.0 {
                    return Err("material density must be >= 0".to_owned());
                }
                if !(0.0..=2.0).contains(&material.elasticity) {
                    return Err("material elasticity must be within [0, 2]".to_owned());
                }
                if !(0.0..=2.0).contains(&material.friction) {
                    return Err("material friction must be within [0, 2]".to_owned());
                }
            }
            QuartzAction::SetDensity { value, .. } => {
                if *value < 0.0 {
                    return Err("density must be >= 0".to_owned());
                }
            }
            QuartzAction::SetElasticity { value, .. } => {
                if !(0.0..=2.0).contains(value) {
                    return Err("elasticity must be within [0, 2]".to_owned());
                }
            }
            QuartzAction::SetFriction { value, .. } => {
                if !(0.0..=2.0).contains(value) {
                    return Err("friction must be within [0, 2]".to_owned());
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn edit_quartz_expr(
        ui: &mut egui::Ui,
        label: &str,
        expr: &mut QuartzExpr,
        changed: &mut bool,
        suggestions: &EditorSuggestions,
    ) {
        ui.label(label);
        let mut kind_str = match expr.kind {
            QuartzExprKind::F32 => "f32",
            QuartzExprKind::I32 => "i32",
            QuartzExprKind::Bool => "bool",
            QuartzExprKind::Str => "str",
            QuartzExprKind::Var => "var",
        };
        egui::ComboBox::from_label(format!("{label} type"))
            .selected_text(kind_str)
            .show_ui(ui, |ui| {
                *changed |= ui.selectable_value(&mut kind_str, "f32", "f32").changed();
                *changed |= ui.selectable_value(&mut kind_str, "i32", "i32").changed();
                *changed |= ui.selectable_value(&mut kind_str, "bool", "bool").changed();
                *changed |= ui.selectable_value(&mut kind_str, "str", "str").changed();
                *changed |= ui.selectable_value(&mut kind_str, "var", "var (variable name)").changed();
            });
        let next_kind = match kind_str {
            "f32"  => QuartzExprKind::F32,
            "i32"  => QuartzExprKind::I32,
            "bool" => QuartzExprKind::Bool,
            "str"  => QuartzExprKind::Str,
            _      => QuartzExprKind::Var,
        };
        if expr.kind != next_kind { expr.kind = next_kind; *changed = true; }
        let options = if expr.kind == QuartzExprKind::Var {
            &suggestions.variable_names
        } else {
            &suggestions.expression_names
        };
        *changed |= Self::suggestion_text_edit(
            ui,
            format!("expr_raw_{label}"),
            &mut expr.raw,
            options,
        );
    }

}

#[cfg(test)]
mod tests {
    use super::QuartzForgeApp;
    use crate::core::quartz_domain::{
        ObjectPhysicsMaterialPreset, ObjectPhysicsMaterialSpec, QuartzAction, QuartzTargetRef,
    };

    #[test]
    fn action_editor_builds_physics_material_variants() {
        let actions = vec![
            QuartzAction::SetMaterial {
                target: QuartzTargetRef::Name("player".to_owned()),
                material: ObjectPhysicsMaterialSpec::resolved_defaults(
                    ObjectPhysicsMaterialPreset::Default,
                ),
            },
            QuartzAction::SetDensity {
                target: QuartzTargetRef::Name("player".to_owned()),
                value: 1.0,
            },
            QuartzAction::SetElasticity {
                target: QuartzTargetRef::Name("player".to_owned()),
                value: 0.5,
            },
            QuartzAction::SetFriction {
                target: QuartzTargetRef::Name("player".to_owned()),
                value: 0.5,
            },
            QuartzAction::ApplyForce {
                target: QuartzTargetRef::Name("player".to_owned()),
                fx: 1.0,
                fy: 2.0,
            },
            QuartzAction::ApplyImpulse {
                target: QuartzTargetRef::Name("player".to_owned()),
                ix: 1.0,
                iy: 2.0,
            },
            QuartzAction::FreezeBody {
                target: QuartzTargetRef::Name("player".to_owned()),
            },
            QuartzAction::UnfreezeBody {
                target: QuartzTargetRef::Name("player".to_owned()),
            },
            QuartzAction::WakeBody {
                target: QuartzTargetRef::Name("player".to_owned()),
            },
            QuartzAction::SetPhysicsQuality {
                quality: "Medium".to_owned(),
            },
            QuartzAction::SetCollisionMode {
                target: QuartzTargetRef::Name("player".to_owned()),
                mode: "Solid".to_owned(),
            },
            QuartzAction::SetSlope {
                target: QuartzTargetRef::Name("player".to_owned()),
                left_offset: -4.0,
                right_offset: 4.0,
                auto_rotate: true,
            },
            QuartzAction::SetSurfaceNormal {
                target: QuartzTargetRef::Name("player".to_owned()),
                nx: 0.0,
                ny: -1.0,
            },
            QuartzAction::TransferMomentum {
                from: QuartzTargetRef::Name("player".to_owned()),
                to: QuartzTargetRef::Name("crate".to_owned()),
                scale: 1.0,
            },
        ];

        assert_eq!(actions.len(), 14);
    }

    #[test]
    fn action_editor_validates_physics_ranges() {
        let invalid_density = QuartzAction::SetDensity {
            target: QuartzTargetRef::Name("player".to_owned()),
            value: -1.0,
        };
        assert!(QuartzForgeApp::validate_physics_action_ranges(&invalid_density).is_err());

        let invalid_material = QuartzAction::SetMaterial {
            target: QuartzTargetRef::Name("player".to_owned()),
            material: ObjectPhysicsMaterialSpec {
                preset: ObjectPhysicsMaterialPreset::Custom,
                elasticity: 3.0,
                friction: 0.5,
                density: 1.0,
            },
        };
        assert!(QuartzForgeApp::validate_physics_action_ranges(&invalid_material).is_err());

        let valid = QuartzAction::SetFriction {
            target: QuartzTargetRef::Name("player".to_owned()),
            value: 0.8,
        };
        assert!(QuartzForgeApp::validate_physics_action_ranges(&valid).is_ok());
    }

    #[test]
    fn setposition_action_emits_momentum_zero_warning() {
        let action = QuartzAction::SetPosition {
            target: QuartzTargetRef::Name("player".to_owned()),
            x: 100.0,
            y: 200.0,
        };
        let warning = QuartzForgeApp::setposition_semantic_warning(&action);
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("SetPosition resets momentum"));
    }

    #[test]
    fn action_editor_builds_emitter_variants() {
        let actions = vec![
            QuartzAction::SpawnEmitter {
                name: "sparkles".to_owned(),
            },
            QuartzAction::RemoveEmitter {
                name: "sparkles".to_owned(),
            },
            QuartzAction::AttachEmitter {
                emitter_name: "sparkles".to_owned(),
                target: QuartzTargetRef::Name("player".to_owned()),
                location: None,
            },
            QuartzAction::DetachEmitter {
                emitter_name: "sparkles".to_owned(),
            },
            QuartzAction::SetEmitterRate {
                name: "sparkles".to_owned(),
                value: 20.0,
            },
            QuartzAction::SetEmitterLifetime {
                name: "sparkles".to_owned(),
                value: 1.0,
            },
            QuartzAction::SetEmitterVelocity {
                name: "sparkles".to_owned(),
                x: 0.0,
                y: -40.0,
            },
            QuartzAction::SetEmitterSpread {
                name: "sparkles".to_owned(),
                x: 20.0,
                y: 20.0,
            },
            QuartzAction::SetEmitterSize {
                name: "sparkles".to_owned(),
                value: 6.0,
            },
            QuartzAction::SetEmitterColor {
                name: "sparkles".to_owned(),
                rgba: [255, 220, 120, 255],
            },
            QuartzAction::SetEmitterGravityScale {
                name: "sparkles".to_owned(),
                value: 1.0,
            },
            QuartzAction::SetEmitterCollision {
                name: "sparkles".to_owned(),
                mode: "None".to_owned(),
            },
            QuartzAction::SetEmitterRenderLayer {
                name: "sparkles".to_owned(),
                value: 1,
            },
            QuartzAction::SetEmitterSizeEnd {
                name: "sparkles".to_owned(),
                value: 0.0,
            },
            QuartzAction::SetEmitterColorEnd {
                name: "sparkles".to_owned(),
                rgba: Some([255, 180, 80, 0]),
            },
            QuartzAction::SetEmitterShape {
                name: "sparkles".to_owned(),
                shape: "Circle".to_owned(),
            },
            QuartzAction::SetEmitterAlignToVelocity {
                name: "sparkles".to_owned(),
                enabled: true,
            },
            QuartzAction::SetEmitterInterpolatePosition {
                name: "sparkles".to_owned(),
                enabled: true,
            },
        ];

        assert_eq!(actions.len(), 18);
    }

    #[test]
    fn action_editor_builds_camera_effect_variants() {
        let actions = vec![
            QuartzAction::AddZoom { value: 0.1 },
            QuartzAction::SmoothZoomAt { delta: 0.08 },
            QuartzAction::CameraFlashWith {
                color_rgba: [255, 255, 255, 255],
                duration_s: 0.2,
                mode: "Add".to_owned(),
                ease: "OutQuad".to_owned(),
                intensity: 1.0,
                freeze_frame_s: 0.0,
            },
            QuartzAction::SetGlow {
                target: QuartzTargetRef::Name("player".to_owned()),
                color_rgb: [120, 220, 255],
                width: 3.0,
            },
            QuartzAction::ClearGlow {
                target: QuartzTargetRef::Name("player".to_owned()),
            },
            QuartzAction::SetTint {
                target: QuartzTargetRef::Name("player".to_owned()),
                color_rgba: [255, 180, 180, 255],
            },
            QuartzAction::ClearTint {
                target: QuartzTargetRef::Name("player".to_owned()),
            },
        ];

        assert_eq!(actions.len(), 7);
    }
}
