//! One number of a layer: its value, and a small menu for what moves it (time, a wave, one of the
//! shader's parameters, or a layer below), with that driver's own numbers beside it.

use std::ops::RangeInclusive;

use crate::core::shader_asset::ShaderParam;
use crate::core::shader_layers::{Driven, Driver};

/// What a number may be driven by where it is edited.
pub(in crate::app) struct DriveChoices<'a> {
    pub params: &'a [ShaderParam],
    /// Names of the shown layers below the one being edited, by index.
    pub layers_below: Vec<(usize, String)>,
}

/// The editor for one driven number.
pub(in crate::app) struct DrivenField;

impl DrivenField {
    /// `driven`'s row; `range` is where its value usually lies (typing past it is allowed).
    pub fn show(ui: &mut egui::Ui, id: (usize, &str), driven: &mut Driven, range: RangeInclusive<f32>, choices: &DriveChoices<'_>) -> bool {
        ui.push_id(id, |ui| {
            ui.horizontal(|ui| {
                let speed = f64::from((range.end() - range.start()).abs() / 200.0).max(0.001);
                let mut changed = ui.add(egui::DragValue::new(&mut driven.value).speed(speed).max_decimals(3)).changed();
                changed |= Self::driver_menu(ui, driven, choices);
                changed |= Self::driver_numbers(ui, driven, choices);
                changed
            })
            .inner
        })
        .inner
    }

    fn driver_menu(ui: &mut egui::Ui, driven: &mut Driven, choices: &DriveChoices<'_>) -> bool {
        let current = Self::label(&driven.by, choices);
        let mut chosen = None;
        egui::ComboBox::from_id_salt("driver").width(84.0).selected_text(current).show_ui(ui, |ui| {
            let options = [
                ("fixed", Driver::Fixed),
                ("time", Driver::Time { rate: 1.0 }),
                ("wave", Driver::Wave { amount: 0.2, speed: 3.0 }),
            ];
            for (label, driver) in options {
                if ui.selectable_label(std::mem::discriminant(&driven.by) == std::mem::discriminant(&driver), label).clicked() {
                    chosen = Some(driver);
                }
            }
            for (slot, param) in choices.params.iter().enumerate() {
                let slot = u8::try_from(slot).unwrap_or(u8::MAX);
                if ui.selectable_label(matches!(driven.by, Driver::Param { slot: current, .. } if current == slot), format!("param: {}", param.name)).clicked() {
                    chosen = Some(Driver::Param { slot, amount: 1.0 });
                }
            }
            for (layer, name) in &choices.layers_below {
                if ui.selectable_label(matches!(driven.by, Driver::Layer { layer: current, .. } if current == *layer), format!("layer: {name}")).clicked() {
                    chosen = Some(Driver::Layer { layer: *layer, amount: 0.5 });
                }
            }
        });
        let Some(driver) = chosen else { return false };
        driven.by = driver;
        true
    }

    fn driver_numbers(ui: &mut egui::Ui, driven: &mut Driven, choices: &DriveChoices<'_>) -> bool {
        let missing = match driven.by {
            Driver::Param { slot, .. } => usize::from(slot) >= choices.params.len(),
            Driver::Layer { layer, .. } => !choices.layers_below.iter().any(|(index, _)| *index == layer),
            _ => false,
        };
        if missing {
            ui.colored_label(crate::app::shell::ForgeTheme::WARN, "reads 0");
        }
        let number = |ui: &mut egui::Ui, value: &mut f32, prefix: &str| ui.add(egui::DragValue::new(value).speed(0.01).max_decimals(3).prefix(prefix)).changed();
        match &mut driven.by {
            Driver::Fixed => false,
            Driver::Time { rate } => number(ui, rate, "per s "),
            Driver::Wave { amount, speed } => {
                let swing = number(ui, amount, "± ");
                swing | number(ui, speed, "speed ")
            }
            Driver::Param { amount, .. } | Driver::Layer { amount, .. } => number(ui, amount, "× "),
        }
    }

    fn label(driver: &Driver, choices: &DriveChoices<'_>) -> String {
        match driver {
            Driver::Fixed => "fixed".to_owned(),
            Driver::Time { .. } => "time".to_owned(),
            Driver::Wave { .. } => "wave".to_owned(),
            Driver::Param { slot, .. } => choices.params.get(usize::from(*slot)).map_or_else(|| format!("param {slot}"), |param| param.name.clone()),
            Driver::Layer { layer, .. } => choices.layers_below.iter().find(|(index, _)| index == layer).map_or_else(|| format!("layer {layer}"), |(_, name)| name.clone()),
        }
    }
}
