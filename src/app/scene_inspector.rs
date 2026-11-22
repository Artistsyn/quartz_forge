//! Scene settings: name and kind, stage size, camera, starting variables, physics engine,
//! object pools, plugins and background, as collapsible sections of labelled fields.

use eframe::egui::{self, DragValue, RichText};

use super::fields::Fields;
use super::shell::{DockTab, ForgeTheme};
use super::QuartzForgeApp;
use crate::core::project::{PluginRegistration, PoolBlueprint, SceneDocument, SceneKind};
use crate::core::quartz_domain::{
    CanvasOrientation, CrystallineConfigProfile, CrystallineQuality, QuartzExpr, QuartzExprKind, QuartzTargetRef, SceneCanvasSpec, SceneVariable,
};

/// First-party plugins a scene can register with a tick.
const KNOWN_PLUGINS: [&str; 4] = ["TerrainCollisionPlugin", "GrapplePlugin", "BackgroundPlugin", "SaveGamePlugin"];

/// Stage sizes offered as one-click presets: (label, width, height).
const STAGE_PRESETS: [(&str, f32, f32); 4] =
    [("4K landscape", 3840.0, 2160.0), ("4K portrait", 2160.0, 3840.0), ("1080p landscape", 1920.0, 1080.0), ("1080p portrait", 1080.0, 1920.0)];

impl QuartzForgeApp {
    pub(super) fn inspector_panel(&mut self, ui: &mut egui::Ui) {
        let scene_index = self.project_state.active_scene_index;
        let Some(scene) = self.project_state.manifest.scenes.get_mut(scene_index) else {
            ui.label("No scene.");
            return;
        };
        let mut changed = Self::scene_identity(ui, scene);
        changed |= Fields::section(ui, "Stage", true, |ui| Self::stage_fields(ui, scene));
        changed |= Fields::section(ui, "Camera", true, |ui| Self::camera_fields(ui, scene));
        changed |= Fields::section(ui, "Starting Variables", true, |ui| Self::variable_rows(ui, &mut scene.variables));
        changed |= Fields::section(ui, "Physics Engine", false, |ui| Self::engine_fields(ui, scene));
        changed |= Fields::section(ui, "Object Pools", false, |ui| Self::pool_rows(ui, scene));
        changed |= Fields::section(ui, "Plugins", false, |ui| Self::plugin_rows(ui, &mut scene.required_plugins));
        let background = Self::background_summary(scene);
        let mut open_background = false;
        changed |= Fields::section(ui, "Background", false, |ui| {
            ui.label(background);
            open_background = ui.button("Edit Background").clicked();
            false
        });
        let root = self.project_root.clone();
        changed |= Fields::section(ui, "Code File", false, |ui| {
            Self::source_file_picker(ui, "File", root.as_deref(), &mut self.status_line, &mut scene.source_file)
        });
        if open_background {
            self.show_dock_tab(DockTab::Background);
        }
        if changed {
            self.touch_and_refresh_preview();
        }
    }

    fn scene_identity(ui: &mut egui::Ui, scene: &mut SceneDocument) -> bool {
        let kinds = [(SceneKind::Game, "Game"), (SceneKind::Ui, "Menu / UI"), (SceneKind::Cinematic, "Cinematic"), (SceneKind::Test, "Test")];
        let changed = Fields::grid(ui, "scene_identity", |fields| {
            fields.text("Name", "", &mut scene.name);
            fields.choice("Kind", "", &mut scene.kind, &kinds);
            fields.custom("Notes", "For you and agents: what this scene is for", |ui| {
                ui.add(egui::TextEdit::multiline(&mut scene.notes).desired_rows(2).desired_width(f32::INFINITY)).changed()
            });
        });
        ui.add_space(4.0);
        changed
    }

    fn stage_fields(ui: &mut egui::Ui, scene: &mut SceneDocument) -> bool {
        let canvas = &mut scene.canvas;
        let (width, height) = (canvas.virtual_width, canvas.virtual_height);
        let orientations = [(CanvasOrientation::Landscape, "Landscape"), (CanvasOrientation::Portrait, "Portrait")];
        let mut changed = Fields::grid(ui, "scene_stage", |fields| {
            fields.pair("Size", "The scene's coordinate space in pixels; the game scales it to any screen", (&mut canvas.virtual_width, &mut canvas.virtual_height), 4.0);
            fields.choice("Orientation", "", &mut canvas.orientation, &orientations);
            fields.custom("Presets", "", |ui| Self::stage_presets(ui, canvas));
            fields.check("Background cells", "Show the background tile grid", &mut canvas.show_background_cells);
            fields.check("Snap backgrounds", "Background objects snap to the nearest cell", &mut canvas.snap_background_objects_to_cells);
        });
        let resized = (canvas.virtual_width, canvas.virtual_height) != (width, height);
        if resized && !canvas.camera_size_customized {
            (canvas.camera_width, canvas.camera_height) = (canvas.virtual_width, canvas.virtual_height);
        }
        changed |= resized;
        changed
    }

    fn stage_presets(ui: &mut egui::Ui, canvas: &mut SceneCanvasSpec) -> bool {
        let mut picked = None;
        ui.menu_button("Choose…", |ui| {
            picked = STAGE_PRESETS.iter().find(|(label, _, _)| ui.button(*label).clicked()).copied();
        });
        let Some((_, width, height)) = picked else { return false };
        (canvas.virtual_width, canvas.virtual_height) = (width, height);
        canvas.orientation = if width >= height { CanvasOrientation::Landscape } else { CanvasOrientation::Portrait };
        true
    }

    fn camera_fields(ui: &mut egui::Ui, scene: &mut SceneDocument) -> bool {
        let names: Vec<String> = scene.objects.iter().filter(|blueprint| !blueprint.spawn_only).map(|blueprint| blueprint.id.clone()).collect();
        let camera = &mut scene.camera;
        let canvas = &mut scene.canvas;
        let (camera_width, camera_height) = (canvas.camera_width, canvas.camera_height);
        let changed = Fields::grid(ui, "scene_camera", |fields| {
            fields.check("Follow", "The camera keeps an object in view", &mut camera.follow_enabled);
            if camera.follow_enabled {
                fields.custom("Target", "", |ui| Self::target_picker(ui, &mut camera.follow_target, &names));
            }
            fields.ranged("Start zoom", "1 shows the stage at its size", &mut camera.initial_zoom, 0.1..=8.0);
            if (camera.initial_zoom - 1.0).abs() > f32::EPSILON {
                fields.check("Ease in", "Zoom smoothly at start instead of at once", &mut camera.smooth_initial_zoom);
            }
            fields.check("Show frame", "Draw the camera's view in the scene view", &mut canvas.show_camera_frame);
            fields.pair("Position", "", (&mut canvas.camera_x, &mut canvas.camera_y), 1.0);
            fields.pair("View size", "", (&mut canvas.camera_width, &mut canvas.camera_height), 4.0);
        });
        if (canvas.camera_width, canvas.camera_height) != (camera_width, camera_height) {
            canvas.camera_size_customized = true;
        }
        changed
    }

    /// An object picker, with the option to target every object with a tag.
    fn target_picker(ui: &mut egui::Ui, target: &mut QuartzTargetRef, names: &[String]) -> bool {
        let shown = match target {
            QuartzTargetRef::Name(name) | QuartzTargetRef::Id(name) => name.clone(),
            QuartzTargetRef::Tag(tag) => format!("tag: {tag}"),
        };
        let mut changed = false;
        egui::ComboBox::from_id_salt("camera_follow_target").selected_text(shown).show_ui(ui, |ui| {
            for name in names {
                changed |= ui.selectable_value(target, QuartzTargetRef::Name(name.clone()), name).changed();
            }
        });
        if let QuartzTargetRef::Tag(tag) = target {
            changed |= ui.add(egui::TextEdit::singleline(tag).desired_width(80.0)).changed();
        } else if ui.small_button("by tag").on_hover_text("Follow the first object with a tag").clicked() {
            *target = QuartzTargetRef::Tag("player".to_owned());
            changed = true;
        }
        changed
    }

    fn variable_rows(ui: &mut egui::Ui, variables: &mut Vec<SceneVariable>) -> bool {
        ui.label(RichText::new("Set when the scene starts; actions and conditions read and change them.").small().color(ForgeTheme::MUTED));
        let kinds = [(QuartzExprKind::I32, "Whole number"), (QuartzExprKind::F32, "Number"), (QuartzExprKind::Bool, "Yes / no"), (QuartzExprKind::Str, "Text")];
        let mut changed = false;
        let mut removed = None;
        for (index, declared) in variables.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut declared.name).hint_text("name").desired_width(90.0)).changed();
                let current = kinds.iter().find(|(kind, _)| *kind == declared.value.kind).map_or("", |(_, label)| *label);
                egui::ComboBox::from_id_salt(("variable_kind", index)).selected_text(current).width(96.0).show_ui(ui, |ui| {
                    for (kind, label) in &kinds {
                        changed |= ui.selectable_value(&mut declared.value.kind, kind.clone(), *label).changed();
                    }
                });
                changed |= ui.add(egui::TextEdit::singleline(&mut declared.value.raw).desired_width(70.0)).changed();
                if ui.small_button("✕").on_hover_text("Remove").clicked() {
                    removed = Some(index);
                }
            });
        }
        if let Some(index) = removed {
            variables.remove(index);
            changed = true;
        }
        if ui.button("Add Variable").clicked() {
            let name = (1..).map(|n| format!("var_{n}")).find(|name| !variables.iter().any(|declared| &declared.name == name)).unwrap_or_default();
            variables.push(SceneVariable { name, value: QuartzExpr { kind: QuartzExprKind::I32, raw: "0".to_owned() } });
            changed = true;
        }
        changed
    }

    fn engine_fields(ui: &mut egui::Ui, scene: &mut SceneDocument) -> bool {
        let canvas = &mut scene.canvas;
        let profiles = [
            (CrystallineConfigProfile::Platformer, "Platformer"),
            (CrystallineConfigProfile::Floaty, "Floaty"),
            (CrystallineConfigProfile::Realistic, "Realistic"),
            (CrystallineConfigProfile::Arcade, "Arcade"),
        ];
        let qualities = [(CrystallineQuality::Low, "Low"), (CrystallineQuality::Medium, "Medium"), (CrystallineQuality::High, "High")];
        Fields::grid(ui, "scene_engine", |fields| {
            fields.check("Crystalline", "Rigid-body physics with real contacts, stacking and materials", &mut canvas.crystalline_enabled);
            if canvas.crystalline_enabled {
                fields.choice("Feel", "", &mut canvas.crystalline_profile, &profiles);
                fields.choice("Quality", "More solver steps: steadier stacks, more CPU", &mut canvas.crystalline_quality, &qualities);
            }
        })
    }

    fn pool_rows(ui: &mut egui::Ui, scene: &mut SceneDocument) -> bool {
        ui.label(RichText::new("Copies of a spawn template made when the scene starts, reused instead of created each time.").small().color(ForgeTheme::MUTED));
        let templates: Vec<(String, f32)> = scene.objects.iter().filter(|blueprint| blueprint.spawn_only).map(|blueprint| (blueprint.id.clone(), blueprint.advanced.gravity)).collect();
        let mut changed = false;
        let mut removed = None;
        for (index, pool) in scene.pools.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut pool.pool_tag).hint_text("tag").desired_width(80.0)).changed();
                egui::ComboBox::from_id_salt(("pool_template", index)).selected_text(&pool.template_object_id).width(100.0).show_ui(ui, |ui| {
                    for (id, _) in &templates {
                        changed |= ui.selectable_value(&mut pool.template_object_id, id.clone(), id).changed();
                    }
                });
                changed |= ui.add(DragValue::new(&mut pool.count).range(1..=4096).prefix("× ")).changed();
                if ui.small_button("✕").on_hover_text("Remove").clicked() {
                    removed = Some(index);
                }
            });
            match templates.iter().find(|(id, _)| *id == pool.template_object_id) {
                None => {
                    ui.colored_label(ForgeTheme::BAD, "Choose a spawn template from this scene.");
                }
                Some((_, gravity)) if *gravity != 0.0 => {
                    ui.colored_label(ForgeTheme::WARN, "This template has gravity: parked copies can gather speed before they are used.");
                }
                Some(_) => {}
            }
        }
        if let Some(index) = removed {
            scene.pools.remove(index);
            changed = true;
        }
        if ui.add_enabled(!templates.is_empty(), egui::Button::new("Add Pool")).on_disabled_hover_text("Add a spawn template first").clicked() {
            let template = templates.first().map(|(id, _)| id.clone()).unwrap_or_default();
            scene.pools.push(PoolBlueprint { pool_tag: format!("pool_{}", scene.pools.len() + 1), template_object_id: template, count: 32 });
            changed = true;
        }
        changed
    }

    fn plugin_rows(ui: &mut egui::Ui, plugins: &mut Vec<PluginRegistration>) -> bool {
        ui.label(RichText::new("A plugin runs only in scenes that register it.").small().color(ForgeTheme::MUTED));
        let mut changed = false;
        for type_name in KNOWN_PLUGINS {
            let mut enabled = plugins.iter().any(|registration| registration.type_name == type_name);
            let dispatch = PluginRegistration::known_dispatch_name(type_name).unwrap_or("?");
            if ui.checkbox(&mut enabled, type_name).on_hover_text(format!("Actions call it as \"{dispatch}\"")).changed() {
                if enabled {
                    plugins.push(PluginRegistration::new(type_name));
                } else {
                    plugins.retain(|registration| registration.type_name != type_name);
                }
                changed = true;
            }
        }
        let mut removed = None;
        for (index, registration) in plugins.iter_mut().enumerate().filter(|(_, registration)| PluginRegistration::known_use_path(&registration.type_name).is_none()) {
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut registration.init_expr).desired_width(200.0).font(egui::TextStyle::Monospace)).changed();
                if ui.small_button("✕").clicked() {
                    removed = Some(index);
                }
            });
        }
        if let Some(index) = removed {
            plugins.remove(index);
            changed = true;
        }
        if ui.button("Add Custom Plugin").clicked() {
            plugins.push(PluginRegistration { type_name: "MyPlugin".to_owned(), init_expr: "my_crate::MyPlugin::new()".to_owned() });
            changed = true;
        }
        changed
    }

    fn background_summary(scene: &SceneDocument) -> String {
        if let Some(path_forge) = scene.background.path_forge_active() {
            format!("PathForge {:?}: {}", path_forge.mode, path_forge.source)
        } else if scene.background.enabled {
            format!("{} layer(s)", scene.background.layers.len())
        } else {
            "None".to_owned()
        }
    }
}
