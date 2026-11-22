//! The Effects tab's Shaders view: the project's own shader effects (`data/shaders`), designed as
//! a stack of layers or written in WGSL, drawn live by the engine's renderer with each layer's
//! thumbnail, checked the way the renderer compiles them, and measured for cost on their own
//! once an edit settles. Objects use one through their effect's menu.

mod choice;
mod driven;
mod layers;
mod measure;
mod thumbs;

use std::time::{Duration, Instant};

use quartz_project::particle_effect::EffectFinding;
use quartz_project::shader_asset::{ProjectShaderAsset, ShaderAuthoring, ShaderParam, PARAM_SLOTS};
use quartz_project::shader_engine::ShaderBridge;
use wgpu_canvas::{EffectColor, ShaderProblem};

use super::{Backdrop, EffectForm, ProjectLook};
use crate::app::shell::ForgeTheme;
use crate::app::QuartzForgeApp;
pub(in crate::app) use choice::ProjectChoiceForm;
use layers::LayerForm;
use measure::ShaderMeasure;
use thumbs::LayerThumbs;

/// The Shaders view's state.
pub(in crate::app) struct ShaderWorkshop {
    pub selected: Option<String>,
    /// The preview's parameter values (objects keep their own), and the shader they are for.
    preview: ([f32; PARAM_SLOTS], String),
    tint: [u8; 3],
    /// Scene pixels across the previewed quad.
    preview_size: f32,
    thumbs: LayerThumbs,
    measure: ShaderMeasure,
    /// The last compile check, by source hash.
    checked: Option<(u64, Result<(), ShaderProblem>)>,
    /// The source hash of the open shader and when it last changed, to measure once it settles.
    settled: Option<(u64, Instant)>,
}

/// A change picked from the shader list.
enum ListAction {
    Open(String),
    Duplicate(String),
    Delete(String),
}

impl ShaderWorkshop {
    const SETTLE: Duration = Duration::from_millis(1500);

    pub fn new() -> Self {
        Self {
            selected: None,
            preview: ([0.0; PARAM_SLOTS], String::new()),
            tint: [255, 170, 80],
            preview_size: 240.0,
            thumbs: LayerThumbs::new(),
            measure: ShaderMeasure::new(),
            checked: None,
            settled: None,
        }
    }

    /// Whether `asset` builds in the renderer, checked once per source.
    fn check(&mut self, asset: &ProjectShaderAsset) -> Result<(), ShaderProblem> {
        let hash = asset.source_hash();
        match &self.checked {
            Some((checked, outcome)) if *checked == hash => outcome.clone(),
            _ => {
                let outcome = ShaderBridge::validate(asset);
                self.checked = Some((hash, outcome.clone()));
                outcome
            }
        }
    }

    /// Start measuring `asset` once its source has stayed the same for a moment.
    fn measure_when_settled(&mut self, ctx: &egui::Context, asset: &ProjectShaderAsset) {
        if asset.current_weight().is_some() || self.measure.measuring().is_some() || self.measure.failed.is_some() {
            return;
        }
        let hash = asset.source_hash();
        match self.settled {
            Some((settled, at)) if settled == hash => {
                if at.elapsed() >= Self::SETTLE {
                    self.measure.start(asset);
                } else {
                    ctx.request_repaint_after(Self::SETTLE);
                }
            }
            _ => {
                self.settled = Some((hash, Instant::now()));
                ctx.request_repaint_after(Self::SETTLE);
            }
        }
    }

    /// The preview's values for `asset`, reset to its defaults when another shader opens.
    fn preview_params(&mut self, asset: &ProjectShaderAsset) -> &mut [f32; PARAM_SLOTS] {
        if self.preview.1 != asset.name {
            self.preview = (asset.defaults(), asset.name.clone());
        }
        &mut self.preview.0
    }
}

#[cfg(test)]
impl ShaderWorkshop {
    pub fn thumbnails_shown(&self) -> usize {
        self.thumbs.count()
    }

    pub fn measure_failure(&self) -> Option<&str> {
        self.measure.failed.as_deref()
    }
}

impl QuartzForgeApp {
    pub(in crate::app) fn shaders_tab(&mut self, ui: &mut egui::Ui) {
        if self.effects.shaders.measure.collect(&mut self.project_state.manifest.shaders) {
            self.project_state.dirty = true;
        }
        if self.effects.shaders.measure.measuring().is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        let height = ui.available_height().max(320.0);
        let column = egui::Layout::top_down(egui::Align::Min);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(200.0, height), column, |ui| {
                ui.set_width(200.0);
                self.shader_list(ui);
            });
            ui.separator();
            let Some(index) = self.selected_shader_index() else {
                ui.vertical(|ui| {
                    ui.add_space(24.0);
                    ui.label("No shader open. Make one with New: layers are designed here; WGSL is written by hand against the engine's contract.");
                });
                return;
            };
            let form_width = (ui.available_width() * 0.5).max(340.0);
            ui.allocate_ui_with_layout(egui::vec2(form_width, height), column, |ui| {
                ui.set_width(form_width);
                egui::ScrollArea::vertical().id_salt("shader_form").auto_shrink([false, false]).show(ui, |ui| self.shader_form(ui, index));
            });
            ui.separator();
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), height), column, |ui| {
                egui::ScrollArea::vertical().id_salt("shader_preview").auto_shrink([false, false]).show(ui, |ui| self.shader_preview_column(ui, index));
            });
        });
    }

    fn selected_shader_index(&self) -> Option<usize> {
        let selected = self.effects.shaders.selected.as_ref()?;
        self.project_state.manifest.shaders.iter().position(|asset| &asset.name == selected)
    }

    fn shader_list(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("Shaders");
            ui.menu_button("New ⏵", |ui| {
                if ui.button("Layers").on_hover_text("Designed here, as a stack of patterns").clicked() {
                    self.add_shader(ProjectShaderAsset::layered("shader"));
                    ui.close();
                }
                if ui.button("WGSL").on_hover_text("Written by hand: one effect function against the engine's contract").clicked() {
                    self.add_shader(ProjectShaderAsset::handwritten("shader"));
                    ui.close();
                }
            });
        });
        ui.separator();
        let mut action = None;
        egui::ScrollArea::vertical().id_salt("shader_list").show(ui, |ui| {
            for asset in &self.project_state.manifest.shaders {
                let selected = self.effects.shaders.selected.as_deref() == Some(asset.name.as_str());
                let marker = if asset.blocking().is_empty() { "" } else { "✖ " };
                let response = ui.selectable_label(selected, format!("{marker}{}", asset.name));
                if response.clicked() {
                    action = Some(ListAction::Open(asset.name.clone()));
                }
                response.context_menu(|ui| {
                    if ui.button("Duplicate").clicked() {
                        action = Some(ListAction::Duplicate(asset.name.clone()));
                    }
                    if ui.button("Delete").clicked() {
                        action = Some(ListAction::Delete(asset.name.clone()));
                    }
                });
            }
        });
        if let Some(action) = action {
            self.apply_shader_action(action);
        }
    }

    fn apply_shader_action(&mut self, action: ListAction) {
        let shaders = &mut self.project_state.manifest.shaders;
        match action {
            ListAction::Open(name) => self.effects.shaders.selected = Some(name),
            ListAction::Duplicate(name) => {
                if let Some(copy) = shaders.iter().find(|asset| asset.name == name).cloned() {
                    self.add_shader(copy);
                }
            }
            ListAction::Delete(name) => {
                shaders.retain(|asset| asset.name != name);
                self.effects.shaders.selected = None;
                self.project_state.dirty = true;
            }
        }
    }

    /// Add `asset` under a name no other shader has, and open it.
    fn add_shader(&mut self, mut asset: ProjectShaderAsset) {
        let taken = |name: &str| self.project_state.manifest.shaders.iter().any(|existing| existing.name == name);
        let base = asset.name.clone();
        let mut count = 2;
        while taken(&asset.name) {
            asset.name = format!("{base}_{count}");
            count += 1;
        }
        self.effects.shaders.selected = Some(asset.name.clone());
        self.project_state.manifest.shaders.push(asset);
        self.project_state.dirty = true;
    }

    fn shader_form(&mut self, ui: &mut egui::Ui, index: usize) {
        self.shader_name_row(ui, index);
        let users = self.shader_users(&self.project_state.manifest.shaders[index].name);
        ui.label(egui::RichText::new(format!("Used by {users} object{}", if users == 1 { "" } else { "s" })).small().weak());
        let tint = self.effects.shaders.tint;
        let asset = &mut self.project_state.manifest.shaders[index];
        let mut changed = ui.add(egui::TextEdit::singleline(&mut asset.description).hint_text("What it is for").desired_width(f32::INFINITY)).changed();
        changed |= ui.add(egui::Slider::new(&mut asset.additive, 0.0..=1.0).text("additive")).on_hover_text("0 covers what is behind; 1 adds light. Objects start from this.").changed();
        changed |= ShaderForm::params(ui, &mut asset.params);
        ui.separator();
        let thumbs = self.effects.shaders.thumbs.textures(ui.ctx(), asset, tint).to_vec();
        changed |= ShaderForm::authoring(ui, asset, &thumbs, self.effects.shaders.checked.as_ref().and_then(|(_, outcome)| outcome.as_ref().err()));
        if changed {
            self.project_state.dirty = true;
        }
    }

    fn shader_name_row(&mut self, ui: &mut egui::Ui, index: usize) {
        let mut name = self.project_state.manifest.shaders[index].name.clone();
        ui.horizontal(|ui| {
            ui.label("Name");
            let edited = ui.add(egui::TextEdit::singleline(&mut name).desired_width(180.0)).lost_focus();
            let clash = self.project_state.manifest.shaders.iter().enumerate().any(|(other, asset)| other != index && asset.name == name);
            if edited && !clash && !name.trim().is_empty() && name != self.project_state.manifest.shaders[index].name {
                self.rename_shader(index, &name);
            }
            if clash {
                ui.colored_label(ForgeTheme::BAD, "another shader has this name");
            }
        });
    }

    /// Rename the shader and every object's choice of it.
    fn rename_shader(&mut self, index: usize, name: &str) {
        let manifest = &mut self.project_state.manifest;
        let old = std::mem::replace(&mut manifest.shaders[index].name, name.to_owned());
        let choices = manifest.scenes.iter_mut().flat_map(|scene| scene.objects.iter_mut()).filter_map(|blueprint| blueprint.effect.as_mut().and_then(|spec| spec.project.as_mut()));
        for choice in choices.filter(|choice| choice.shader == old) {
            choice.shader = name.to_owned();
        }
        self.effects.shaders.selected = Some(name.to_owned());
        self.project_state.dirty = true;
    }

    fn shader_users(&self, name: &str) -> usize {
        let objects = self.project_state.manifest.scenes.iter().flat_map(|scene| scene.objects.iter());
        objects.filter(|blueprint| blueprint.effect.as_ref().and_then(|spec| spec.project.as_ref()).is_some_and(|choice| choice.shader == name)).count()
    }

    fn shader_preview_column(&mut self, ui: &mut egui::Ui, index: usize) {
        let asset = self.project_state.manifest.shaders[index].clone();
        let workshop = &mut self.effects.shaders;
        let checked = workshop.check(&asset);
        if checked.is_ok() {
            workshop.measure_when_settled(ui.ctx(), &asset);
        }
        let mut choice = asset.choice();
        choice.params = *workshop.preview_params(&asset);
        let color = EffectColor::srgb8(workshop.tint[0], workshop.tint[1], workshop.tint[2]);
        let size = workshop.preview_size;
        self.effects.preview.show_project(ui, &ProjectLook { asset: &asset, choice: &choice, color }, (size, size));
        self.shader_preview_controls(ui, &asset);
        ui.separator();
        ui.strong("Check");
        let mut findings: Vec<EffectFinding> = asset.blocking();
        if let Err(problem) = &checked {
            let line = problem.line.map_or_else(String::new, |line| format!("line {line}: "));
            findings.push(EffectFinding::block(format!("{line}{}", problem.message)));
        }
        findings.extend(asset.warnings());
        if findings.is_empty() {
            ui.colored_label(ForgeTheme::GOOD, "Builds in the renderer.");
        }
        EffectForm::findings(ui, &findings);
        ui.separator();
        self.shader_cost(ui, &asset, checked.is_ok());
        if matches!(asset.authoring, ShaderAuthoring::Layers { .. }) {
            ui.collapsing("Generated WGSL", |ui| {
                ui.add(egui::TextEdit::multiline(&mut asset.wgsl()).code_editor().desired_width(f32::INFINITY).interactive(false));
            });
        }
    }

    fn shader_preview_controls(&mut self, ui: &mut egui::Ui, asset: &ProjectShaderAsset) {
        let workshop = &mut self.effects.shaders;
        let preview = &mut self.effects.preview;
        ui.horizontal_wrapped(|ui| {
            ui.label("Tint");
            ui.color_edit_button_srgb(&mut workshop.tint);
            for (backdrop, label) in Backdrop::ALL {
                ui.selectable_value(&mut preview.backdrop, backdrop, label);
            }
        });
        ui.add(egui::Slider::new(&mut workshop.preview_size, 16.0..=1440.0).logarithmic(true).text("size in scene pixels"));
        let params = workshop.preview_params(asset);
        for (slot, param) in asset.params.iter().enumerate().take(PARAM_SLOTS) {
            let range = param.min.min(param.max)..=param.max.max(param.min);
            ui.add(egui::Slider::new(&mut params[slot], range).text(&param.name));
        }
    }

    fn shader_cost(&mut self, ui: &mut egui::Ui, asset: &ProjectShaderAsset, builds: bool) {
        ui.strong("Cost");
        let measure = &mut self.effects.shaders.measure;
        match (asset.current_weight(), measure.measuring()) {
            (_, Some(name)) if name == asset.name => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Measuring on this GPU…");
                });
            }
            (Some(weight), _) => {
                ui.label(format!("{weight:.2}× a plain sprite per pixel"));
                if let Some(measured) = &asset.measured {
                    ui.label(egui::RichText::new(&measured.measured_on).small().weak());
                }
            }
            (None, _) if !builds => {
                ui.label("Measured once the shader builds.");
            }
            (None, _) => {
                ui.label("Not measured for this source yet; it is measured when edits pause.");
            }
        }
        if let Some(error) = &measure.failed {
            ui.colored_label(ForgeTheme::WARN, format!("The measurement failed: {error}"));
        }
        if builds && measure.measuring().is_none() && ui.button("Measure again").clicked() {
            measure.start(asset);
        }
    }
}

/// The shader's own fields: parameters and how it is written.
struct ShaderForm;

impl ShaderForm {
    fn params(ui: &mut egui::Ui, params: &mut Vec<ShaderParam>) -> bool {
        ui.strong("Parameters");
        ui.label(egui::RichText::new("Numbers each object sets; layers read them through \"param\".").small().weak());
        let mut changed = false;
        let mut removed = None;
        for (slot, param) in params.iter_mut().enumerate() {
            ui.push_id(("param", slot), |ui| {
                ui.horizontal(|ui| {
                    changed |= ui.add(egui::TextEdit::singleline(&mut param.name).desired_width(100.0)).changed();
                    changed |= ui.add(egui::DragValue::new(&mut param.default).speed(0.01).prefix("default ")).changed();
                    changed |= ui.add(egui::DragValue::new(&mut param.min).speed(0.01).prefix("min ")).changed();
                    changed |= ui.add(egui::DragValue::new(&mut param.max).speed(0.01).prefix("max ")).changed();
                    if ui.small_button("✖").clicked() {
                        removed = Some(slot);
                    }
                });
            });
        }
        if let Some(slot) = removed {
            params.remove(slot);
            changed = true;
        }
        if params.len() < PARAM_SLOTS && ui.small_button("+ Parameter").clicked() {
            params.push(ShaderParam { name: format!("param{}", params.len()), default: 1.0, min: 0.0, max: 1.0 });
            changed = true;
        }
        changed
    }

    fn authoring(ui: &mut egui::Ui, asset: &mut ProjectShaderAsset, thumbs: &[egui::TextureHandle], problem: Option<&ShaderProblem>) -> bool {
        let ProjectShaderAsset { params, authoring, wgsl_source, .. } = asset;
        match authoring {
            ShaderAuthoring::Layers { stack } => {
                let mut convert = false;
                ui.horizontal(|ui| {
                    ui.strong("Layers");
                    ui.label(egui::RichText::new("top is drawn last").small().weak());
                    ui.menu_button("Convert to WGSL ⏵", |ui| {
                        ui.label("The layers are replaced by their WGSL, to edit by hand. It cannot be turned back into layers.");
                        convert = ui.button("Convert").clicked();
                    });
                });
                let changed = LayerForm::show(ui, stack, params, thumbs);
                if convert {
                    *wgsl_source = stack.wgsl();
                    *authoring = ShaderAuthoring::Wgsl;
                }
                changed || convert
            }
            ShaderAuthoring::Wgsl => Self::code(ui, wgsl_source, problem),
        }
    }

    fn code(ui: &mut egui::Ui, source: &mut String, problem: Option<&ShaderProblem>) -> bool {
        ui.strong("WGSL");
        ui.label(egui::RichText::new("fn effect(input: EffectInput) -> vec4<f32>: linear colour and straight alpha. No bindings or entry points.").small().weak());
        if let Some(problem) = problem {
            let line = problem.line.map_or_else(String::new, |line| format!("line {line}: "));
            ui.colored_label(ForgeTheme::BAD, format!("{line}{}", problem.message));
        }
        let changed = ui.add(egui::TextEdit::multiline(source).code_editor().desired_rows(22).desired_width(f32::INFINITY)).changed();
        ui.collapsing("The contract: EffectInput and the fx_ helpers", |ui| {
            ui.add(egui::TextEdit::multiline(&mut ShaderBridge::prelude()).code_editor().desired_width(f32::INFINITY).interactive(false));
        });
        changed
    }
}
