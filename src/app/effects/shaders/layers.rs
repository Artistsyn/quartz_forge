//! The layer stack editor: layers listed top first, as a painter sees a stack, each a card with
//! its thumbnail (the stack drawn up to it), visibility, name, blend mode and order, and inside it
//! the pattern, warps, shaping, colour and opacity.

use crate::app::fields::Fields;
use crate::core::shader_asset::ShaderParam;
use crate::core::shader_layers::{BlendMode, LayerColor, LayerStack, Pattern, RampStop, ShaderLayer, Shaping, Warp};

use super::driven::{DriveChoices, DrivenField};

/// The layer stack editor.
pub(in crate::app) struct LayerForm;

/// Where a layer's card sits in the stack, and its thumbnail.
#[derive(Clone, Copy)]
struct Card<'a> {
    index: usize,
    last: usize,
    thumb: Option<&'a egui::TextureHandle>,
}

/// A change to the stack's order, applied after the cards are drawn.
enum StackEdit {
    Raise(usize),
    Lower(usize),
    Copy(usize),
    Remove(usize),
}

impl LayerForm {
    /// Every layer, top first; true when anything changed.
    pub fn show(ui: &mut egui::Ui, stack: &mut LayerStack, params: &[ShaderParam], thumbs: &[egui::TextureHandle]) -> bool {
        let mut changed = Self::add_menu(ui, stack);
        let mut edit = None;
        for index in (0..stack.layers.len()).rev() {
            let choices = DriveChoices { params, layers_below: stack.layers[..index].iter().enumerate().filter(|(_, below)| below.enabled).map(|(below, layer)| (below, layer.name.clone())).collect() };
            let last = stack.layers.len() - 1;
            ui.push_id(("layer", index), |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    changed |= Self::header(ui, &Card { index, last, thumb: thumbs.get(index) }, &mut stack.layers[index], &mut edit);
                    egui::CollapsingHeader::new("Pattern, warps, shaping, colour").id_salt("body").default_open(index == last).show(ui, |ui| {
                        changed |= Self::body(ui, index, &mut stack.layers[index], &choices);
                    });
                });
            });
        }
        if let Some(edit) = edit {
            match edit {
                StackEdit::Raise(index) => stack.move_layer(index, index + 1),
                StackEdit::Lower(index) => stack.move_layer(index, index.saturating_sub(1)),
                StackEdit::Copy(index) => stack.duplicate_layer(index),
                StackEdit::Remove(index) => stack.remove_layer(index),
            }
            changed = true;
        }
        changed
    }

    fn add_menu(ui: &mut egui::Ui, stack: &mut LayerStack) -> bool {
        let mut added = None;
        ui.menu_button("+ Layer", |ui| {
            for (label, pattern) in Pattern::every() {
                if ui.button(label).clicked() {
                    added = Some(ShaderLayer::new(label, pattern));
                    ui.close();
                }
            }
        });
        let Some(layer) = added else { return false };
        stack.layers.push(layer);
        true
    }

    fn header(ui: &mut egui::Ui, card: &Card<'_>, layer: &mut ShaderLayer, edit: &mut Option<StackEdit>) -> bool {
        let Card { index, last, thumb } = *card;
        ui.horizontal(|ui| {
            match thumb {
                Some(texture) => ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(44.0, 44.0))),
                None => ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover()).1,
            }
            .on_hover_text("The stack up to this layer");
            let mut changed = ui.checkbox(&mut layer.enabled, "").on_hover_text("Shown").changed();
            changed |= ui.add(egui::TextEdit::singleline(&mut layer.name).desired_width(110.0)).changed();
            egui::ComboBox::from_id_salt("blend").width(86.0).selected_text(BlendMode::ALL.iter().find(|(mode, _)| *mode == layer.blend).map_or("", |(_, name)| name)).show_ui(ui, |ui| {
                for (mode, name) in BlendMode::ALL {
                    changed |= ui.selectable_value(&mut layer.blend, mode, name).changed();
                }
            });
            if index < last && ui.small_button("⬆").on_hover_text("Raise").clicked() {
                *edit = Some(StackEdit::Raise(index));
            }
            if index > 0 && ui.small_button("⬇").on_hover_text("Lower").clicked() {
                *edit = Some(StackEdit::Lower(index));
            }
            if ui.small_button("Copy").clicked() {
                *edit = Some(StackEdit::Copy(index));
            }
            if ui.small_button("✖").on_hover_text("Delete the layer").clicked() {
                *edit = Some(StackEdit::Remove(index));
            }
            changed
        })
        .inner
    }

    fn body(ui: &mut egui::Ui, index: usize, layer: &mut ShaderLayer, choices: &DriveChoices<'_>) -> bool {
        let mut changed = Self::pattern(ui, index, &mut layer.pattern, choices);
        changed |= Self::warps(ui, index, &mut layer.warps, choices);
        changed |= Self::shaping(ui, index, &mut layer.shaping, choices);
        changed |= Self::color(ui, index, &mut layer.color);
        changed |= Fields::grid(ui, &format!("layer_{index}_opacity"), |fields| {
            fields.custom("Opacity", "How strongly the layer goes on", |ui| DrivenField::show(ui, (index, "opacity"), &mut layer.opacity, 0.0..=1.0, choices));
        });
        changed
    }

    fn pattern(ui: &mut egui::Ui, index: usize, pattern: &mut Pattern, choices: &DriveChoices<'_>) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.strong("Pattern");
            egui::ComboBox::from_id_salt("pattern").selected_text(pattern.label()).show_ui(ui, |ui| {
                for (label, kind) in Pattern::every() {
                    if ui.selectable_label(pattern.label() == label, label).clicked() && pattern.label() != label {
                        *pattern = kind;
                        changed = true;
                    }
                }
            });
        });
        changed |= Fields::grid(ui, &format!("layer_{index}_pattern"), |fields| {
            if let Pattern::Noise { octaves, .. } = pattern {
                fields.custom("Octaves", "Detail layers; each costs one more noise read", |ui| ui.add(egui::DragValue::new(octaves).range(1..=8)).changed());
            }
            for (label, driven, range) in pattern.rows() {
                fields.custom(label, "", |ui| DrivenField::show(ui, (index, label), driven, range, choices));
            }
        });
        changed
    }

    fn warps(ui: &mut egui::Ui, index: usize, warps: &mut Vec<Warp>, choices: &DriveChoices<'_>) -> bool {
        let mut changed = false;
        let mut removed = None;
        for (position, warp) in warps.iter_mut().enumerate() {
            let (edited, remove) = ui.push_id(("warp", position), |ui| Self::warp(ui, (index, position), warp, choices)).inner;
            changed |= edited;
            if remove {
                removed = Some(position);
            }
        }
        if let Some(position) = removed {
            warps.remove(position);
            changed = true;
        }
        ui.menu_button("+ Warp", |ui| {
            for (label, warp) in Warp::every() {
                if ui.button(label).clicked() {
                    warps.push(warp);
                    changed = true;
                    ui.close();
                }
            }
        });
        changed
    }

    /// One warp of layer `at.0` at position `at.1`: (changed, delete asked).
    fn warp(ui: &mut egui::Ui, at: (usize, usize), warp: &mut Warp, choices: &DriveChoices<'_>) -> (bool, bool) {
        let remove = ui.horizontal(|ui| {
            ui.label(format!("Warp: {}", warp.label()));
            ui.small_button("✖").clicked()
        }).inner;
        let mut changed = false;
        if let Warp::ByLayer { layer, .. } = warp {
            changed |= Self::layer_pick(ui, layer, choices);
        }
        changed |= Fields::grid(ui, &format!("layer_{}_warp_{}", at.0, at.1), |fields| {
            for (label, driven, range) in warp.rows() {
                fields.custom(label, "", |ui| DrivenField::show(ui, (at.0 * 100 + at.1, label), driven, range, choices));
            }
        });
        (changed, remove)
    }

    fn shaping(ui: &mut egui::Ui, index: usize, shaping: &mut Vec<Shaping>, choices: &DriveChoices<'_>) -> bool {
        let mut changed = false;
        let mut removed = None;
        for (position, step) in shaping.iter_mut().enumerate() {
            let (edited, remove) = ui.push_id(("shaping", position), |ui| Self::shaping_step(ui, (index, position), step, choices)).inner;
            changed |= edited;
            if remove {
                removed = Some(position);
            }
        }
        if let Some(position) = removed {
            shaping.remove(position);
            changed = true;
        }
        ui.menu_button("+ Shaping", |ui| {
            for (label, step) in Shaping::every() {
                if ui.button(label).clicked() {
                    shaping.push(step);
                    changed = true;
                    ui.close();
                }
            }
        });
        changed
    }

    fn shaping_step(ui: &mut egui::Ui, at: (usize, usize), step: &mut Shaping, choices: &DriveChoices<'_>) -> (bool, bool) {
        let remove = ui.horizontal(|ui| {
            ui.label(format!("Shaping: {}", step.label()));
            ui.small_button("✖").clicked()
        }).inner;
        let mut changed = false;
        if let Shaping::MaskBy { layer } = step {
            changed |= Self::layer_pick(ui, layer, choices);
        }
        changed |= Fields::grid(ui, &format!("layer_{}_shaping_{}", at.0, at.1), |fields| {
            for (label, driven, range) in step.rows() {
                fields.custom(label, "", |ui| DrivenField::show(ui, (at.0 * 100 + 50 + at.1, label), driven, range, choices));
            }
        });
        (changed, remove)
    }

    /// Choose which layer below a warp or mask reads.
    fn layer_pick(ui: &mut egui::Ui, layer: &mut usize, choices: &DriveChoices<'_>) -> bool {
        let shown = choices.layers_below.iter().find(|(index, _)| index == layer).map_or_else(|| "no layer below (reads 0)".to_owned(), |(_, name)| name.clone());
        let mut changed = false;
        egui::ComboBox::from_id_salt("reads").selected_text(format!("reads {shown}")).show_ui(ui, |ui| {
            for (index, name) in &choices.layers_below {
                changed |= ui.selectable_value(layer, *index, name).changed();
            }
        });
        changed
    }

    fn color(ui: &mut egui::Ui, index: usize, color: &mut LayerColor) -> bool {
        let kinds = [("Effect colour", LayerColor::Tint), ("Solid", LayerColor::Solid { rgb: [255, 200, 120] }), ("Ramp by value", Self::default_ramp()), ("Object image", LayerColor::Image)];
        let current = kinds.iter().position(|(_, kind)| std::mem::discriminant(kind) == std::mem::discriminant(color)).unwrap_or(0);
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.strong("Colour");
            egui::ComboBox::from_id_salt("color").selected_text(kinds[current].0).show_ui(ui, |ui| {
                for (position, (label, kind)) in kinds.iter().enumerate() {
                    if ui.selectable_label(position == current, *label).clicked() && position != current {
                        *color = kind.clone();
                        changed = true;
                    }
                }
            });
            if let LayerColor::Solid { rgb } = color {
                changed |= ui.color_edit_button_srgb(rgb).changed();
            }
        });
        if let LayerColor::Ramp { stops } = color {
            changed |= Self::ramp(ui, index, stops);
        }
        changed
    }

    fn ramp(ui: &mut egui::Ui, index: usize, stops: &mut Vec<RampStop>) -> bool {
        let mut changed = false;
        let mut removed = None;
        for (position, stop) in stops.iter_mut().enumerate() {
            ui.push_id(("stop", index, position), |ui| {
                ui.horizontal(|ui| {
                    changed |= ui.add(egui::Slider::new(&mut stop.at, 0.0..=1.0).text("at")).changed();
                    changed |= ui.color_edit_button_srgb(&mut stop.rgb).changed();
                    if ui.small_button("✖").clicked() {
                        removed = Some(position);
                    }
                });
            });
        }
        if let Some(position) = removed {
            stops.remove(position);
            changed = true;
        }
        if ui.small_button("+ Stop").clicked() {
            stops.push(RampStop { at: 1.0, rgb: [255, 255, 255] });
            changed = true;
        }
        changed
    }

    fn default_ramp() -> LayerColor {
        LayerColor::Ramp { stops: vec![RampStop { at: 0.0, rgb: [40, 0, 0] }, RampStop { at: 0.5, rgb: [255, 100, 20] }, RampStop { at: 1.0, rgb: [255, 240, 180] }] }
    }
}
