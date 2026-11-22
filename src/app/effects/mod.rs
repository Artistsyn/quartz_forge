//! The Effects tab: the project's particle effects and its own shaders, listed, edited and played
//! live by the engine's renderer, with the cost check beside them. An effect the check blocks is
//! marked in the list and is not played by the game until it is fixed.

mod form;
mod preview;
mod scene_overlay;
mod shaders;

use std::sync::Arc;

use quartz_project::particle_effect::{ParticleEffectAsset, ParticleShapeKind};
use quartz_project::particle_engine::ParticleBridge;

use super::QuartzForgeApp;
use super::shell::ForgeTheme;
pub(in crate::app) use form::EffectForm;
pub(in crate::app) use preview::{Backdrop, EffectPreview, ProjectLook};
pub(in crate::app) use scene_overlay::{SceneEffects, ScreenPlace};
pub(in crate::app) use shaders::{ProjectChoiceForm, ShaderWorkshop};

/// What the tab shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum EffectsMode {
    Particles,
    Shaders,
}

/// The tab's own state: which effect is open and its preview.
pub(in crate::app) struct EffectsWorkshop {
    pub selected: Option<String>,
    pub preview: EffectPreview,
    /// The scene view's effects, drawn by the engine.
    pub scene: SceneEffects,
    /// The image path the preview's picture was loaded from.
    loaded_image: String,
    pub mode: EffectsMode,
    pub shaders: ShaderWorkshop,
}

impl EffectsWorkshop {
    pub fn new() -> Self {
        Self { selected: None, preview: EffectPreview::new(), scene: SceneEffects::new(), loaded_image: String::new(), mode: EffectsMode::Particles, shaders: ShaderWorkshop::new() }
    }
}

impl QuartzForgeApp {
    pub(in crate::app) fn effects_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.effects.mode, EffectsMode::Particles, "Particles");
            ui.selectable_value(&mut self.effects.mode, EffectsMode::Shaders, "Shaders").on_hover_text("The project's own shader effects, which objects can use");
        });
        ui.separator();
        match self.effects.mode {
            EffectsMode::Particles => self.particles_view(ui),
            EffectsMode::Shaders => self.shaders_tab(ui),
        }
    }

    fn particles_view(&mut self, ui: &mut egui::Ui) {
        let height = ui.available_height().max(320.0);
        let column = egui::Layout::top_down(egui::Align::Min);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(200.0, height), column, |ui| {
                ui.set_width(200.0);
                self.effect_list(ui);
            });
            ui.separator();
            let Some(index) = self.selected_effect_index() else {
                ui.vertical(|ui| {
                    ui.add_space(24.0);
                    ui.label("No effect open. Make one with New, or start from a preset.");
                });
                return;
            };
            let form_width = (ui.available_width() * 0.42).max(320.0);
            ui.allocate_ui_with_layout(egui::vec2(form_width, height), column, |ui| {
                ui.set_width(form_width);
                egui::ScrollArea::vertical().id_salt("effect_form").auto_shrink([false, false]).show(ui, |ui| {
                    if EffectForm::show(ui, &mut self.project_state.manifest.particle_effects[index]) {
                        self.project_state.dirty = true;
                    }
                });
            });
            ui.separator();
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), height), column, |ui| {
                egui::ScrollArea::vertical().id_salt("effect_preview").auto_shrink([false, false]).show(ui, |ui| self.effect_preview_column(ui, index));
            });
        });
    }

    fn selected_effect_index(&self) -> Option<usize> {
        let selected = self.effects.selected.as_ref()?;
        self.project_state.manifest.particle_effects.iter().position(|asset| &asset.name == selected)
    }

    fn effect_list(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("Effects");
            if ui.button("New").on_hover_text("A new effect from the default stream").clicked() {
                self.add_effect(ParticleEffectAsset::new("effect"));
            }
            ui.menu_button("Preset ⏵", |ui| {
                for preset in ParticleBridge::presets() {
                    if ui.button(&preset.name).clicked() {
                        self.add_effect(preset);
                        ui.close();
                    }
                }
            });
        });
        ui.separator();
        let mut action = None;
        egui::ScrollArea::vertical().id_salt("effect_list").show(ui, |ui| {
            for asset in &self.project_state.manifest.particle_effects {
                let selected = self.effects.selected.as_deref() == Some(asset.name.as_str());
                let marker = if asset.is_blocked() { "✖ " } else { "" };
                let response = ui.selectable_label(selected, format!("{marker}{}", asset.name));
                if response.clicked() {
                    action = Some(("open", asset.name.clone()));
                }
                response.context_menu(|ui| {
                    if ui.button("Duplicate").clicked() {
                        action = Some(("duplicate", asset.name.clone()));
                    }
                    if ui.button("Delete").clicked() {
                        action = Some(("delete", asset.name.clone()));
                    }
                });
            }
        });
        if let Some((verb, name)) = action {
            self.apply_effect_action(verb, &name);
        }
    }

    fn apply_effect_action(&mut self, verb: &str, name: &str) {
        let effects = &mut self.project_state.manifest.particle_effects;
        let Some(index) = effects.iter().position(|asset| asset.name == name) else { return };
        match verb {
            "duplicate" => {
                let copy = effects[index].clone();
                self.add_effect(copy);
            }
            "delete" => {
                effects.remove(index);
                self.effects.selected = None;
                self.project_state.dirty = true;
            }
            _ => {
                self.effects.selected = Some(name.to_owned());
                self.effects.preview.restart();
            }
        }
    }

    /// Add `asset` under a name no other effect has, and open it.
    fn add_effect(&mut self, mut asset: ParticleEffectAsset) {
        let taken = |name: &str| self.project_state.manifest.particle_effects.iter().any(|existing| existing.name == name);
        let base = asset.name.clone();
        let mut count = 2;
        while taken(&asset.name) {
            asset.name = format!("{base}_{count}");
            count += 1;
        }
        self.effects.selected = Some(asset.name.clone());
        self.effects.preview.restart();
        self.project_state.manifest.particle_effects.push(asset);
        self.project_state.dirty = true;
    }

    fn effect_preview_column(&mut self, ui: &mut egui::Ui, index: usize) {
        let asset = self.project_state.manifest.particle_effects[index].clone();
        self.effect_name_row(ui, index);
        self.sync_preview_image(&asset);
        let step = ui.input(|input| input.stable_dt);
        self.effects.preview.advance(&asset, step);
        if self.effects.preview.playing {
            ui.ctx().request_repaint();
        }
        let side_by_side = ui.available_width() > 640.0;
        let picture_width = if side_by_side { (ui.available_width() * 0.55).min(EffectPreview::SIZE.0 as f32) } else { ui.available_width() };
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(picture_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                self.effects.preview.show(ui, &asset);
                if !side_by_side {
                    self.preview_controls(ui);
                }
            });
            if side_by_side {
                ui.vertical(|ui| {
                    self.preview_controls(ui);
                    ui.separator();
                    self.effect_findings(ui, &asset);
                });
            }
        });
        if !side_by_side {
            ui.separator();
            self.effect_findings(ui, &asset);
        }
    }

    fn effect_name_row(&mut self, ui: &mut egui::Ui, index: usize) {
        let mut name = self.project_state.manifest.particle_effects[index].name.clone();
        ui.horizontal(|ui| {
            ui.label("Name");
            let edited = ui.add(egui::TextEdit::singleline(&mut name).desired_width(180.0)).lost_focus();
            let clash = self.project_state.manifest.particle_effects.iter().enumerate().any(|(other, asset)| other != index && asset.name == name);
            if edited && !clash && !name.trim().is_empty() {
                self.project_state.manifest.particle_effects[index].name.clone_from(&name);
                self.effects.selected = Some(name.clone());
                self.project_state.dirty = true;
            }
            if clash {
                ui.colored_label(ForgeTheme::BAD, "another effect has this name");
            }
        });
    }

    fn preview_controls(&mut self, ui: &mut egui::Ui) {
        let preview = &mut self.effects.preview;
        ui.horizontal_wrapped(|ui| {
            let label = if preview.playing { "⏸ Pause" } else { "▶ Play" };
            if ui.button(label).clicked() {
                preview.playing = !preview.playing;
            }
            if ui.button("⟲ Restart").clicked() {
                preview.restart();
            }
            ui.label(format!("{:.2} s", preview.age));
            for (backdrop, label) in Backdrop::ALL {
                ui.selectable_value(&mut preview.backdrop, backdrop, label);
            }
        });
        ui.add(egui::Slider::new(&mut preview.scene_span, 60.0..=1440.0).logarithmic(true).text("scene pixels shown"));
    }

    fn effect_findings(&self, ui: &mut egui::Ui, asset: &ParticleEffectAsset) {
        let scene_size = self.project_state.manifest.scenes.get(self.project_state.active_scene_index).map_or((1280.0, 720.0), |scene| (scene.canvas.virtual_width, scene.canvas.virtual_height));
        ui.strong("Cost check");
        EffectForm::findings(ui, &asset.check(scene_size));
    }

    /// Load the effect's picture for the preview when the Texture shape names a new one.
    fn sync_preview_image(&mut self, asset: &ParticleEffectAsset) {
        let wanted = if asset.look.shape == ParticleShapeKind::Texture { asset.look.image.trim().to_owned() } else { String::new() };
        if wanted == self.effects.loaded_image {
            return;
        }
        self.effects.loaded_image.clone_from(&wanted);
        let path = self.project_root.as_ref().map(|root| root.join(&wanted));
        self.effects.preview.image = path.filter(|_| !wanted.is_empty()).and_then(|path| image::open(path).ok()).map(|picture| Arc::new(picture.to_rgba8()));
    }
}
