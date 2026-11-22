//! An object's effect pointing at one of the project's own shaders: the menu entries that choose
//! one, and its parameters as the shader names and ranges them.

use crate::core::object_effect::ObjectEffectSpec;
use crate::core::shader_asset::ProjectShaderAsset;

/// The inspector's part for a project shader.
pub(in crate::app) struct ProjectChoiceForm;

impl ProjectChoiceForm {
    /// Menu entries for the project's shaders, under the built-in kinds. True when one was chosen.
    pub fn menu(ui: &mut egui::Ui, spec: &mut ObjectEffectSpec, shaders: &[ProjectShaderAsset]) -> bool {
        if shaders.is_empty() {
            return false;
        }
        ui.separator();
        ui.label(egui::RichText::new("Project shaders").small().weak());
        let mut changed = false;
        for asset in shaders {
            let chosen = spec.project.as_ref().is_some_and(|choice| choice.shader == asset.name);
            if ui.selectable_label(chosen, &asset.name).clicked() && !chosen {
                spec.project = Some(asset.choice());
                changed = true;
            }
        }
        changed
    }

    /// The chosen shader's parameters, by name and range. True when one changed.
    pub fn params(ui: &mut egui::Ui, spec: &mut ObjectEffectSpec, shaders: &[ProjectShaderAsset]) -> bool {
        let Some(choice) = spec.project.as_mut() else { return false };
        let Some(asset) = shaders.iter().find(|asset| asset.name == choice.shader) else {
            ui.colored_label(crate::app::shell::ForgeTheme::BAD, format!("The project has no shader {:?}.", choice.shader));
            return false;
        };
        let mut changed = false;
        for (slot, param) in asset.params.iter().enumerate().take(choice.params.len()) {
            let range = param.min.min(param.max)..=param.max.max(param.min);
            changed |= ui.add(egui::Slider::new(&mut choice.params[slot], range).text(&param.name)).changed();
        }
        changed |= ui.add(egui::Slider::new(&mut choice.additive, 0.0..=1.0).text("additive")).on_hover_text("0 covers what is behind; 1 adds light").changed();
        changed
    }
}
