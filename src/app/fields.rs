//! Inspector rows: a grid with a muted label on the left and the control on the right, hover
//! help on the label, and one `changed` flag for the whole grid.

use std::ops::RangeInclusive;

use eframe::egui::{self, Color32, DragValue, RichText, Slider};

use super::shell::ForgeTheme;

/// Rows of one grid: a muted label on the left, the control on the right.
pub(super) struct Fields<'a> {
    pub ui: &'a mut egui::Ui,
    pub changed: bool,
}

impl Fields<'_> {
    /// A collapsible section; returns what `body` returns (false while closed).
    pub fn section(ui: &mut egui::Ui, title: &str, open: bool, body: impl FnOnce(&mut egui::Ui) -> bool) -> bool {
        egui::CollapsingHeader::new(RichText::new(title).strong()).default_open(open).show(ui, body).body_returned.unwrap_or(false)
    }

    pub fn grid(ui: &mut egui::Ui, id: &str, rows: impl FnOnce(&mut Fields<'_>)) -> bool {
        egui::Grid::new(id)
            .num_columns(2)
            .spacing([12.0, 6.0])
            .min_col_width(104.0)
            .show(ui, |ui| {
                let mut fields = Fields { ui, changed: false };
                rows(&mut fields);
                fields.changed
            })
            .inner
    }

    pub fn label(&mut self, text: &str, hint: &str) {
        let response = self.ui.label(RichText::new(text).color(ForgeTheme::MUTED));
        if !hint.is_empty() {
            response.on_hover_text(hint);
        }
    }

    pub fn custom(&mut self, text: &str, hint: &str, control: impl FnOnce(&mut egui::Ui) -> bool) {
        self.label(text, hint);
        self.changed |= control(self.ui);
        self.ui.end_row();
    }

    pub fn number(&mut self, text: &str, hint: &str, value: &mut f32, speed: f64) {
        self.custom(text, hint, |ui| ui.add(DragValue::new(value).speed(speed).max_decimals(3)).changed());
    }

    pub fn ranged(&mut self, text: &str, hint: &str, value: &mut f32, range: RangeInclusive<f32>) {
        self.custom(text, hint, |ui| ui.add(Slider::new(value, range).clamping(egui::SliderClamping::Never)).changed());
    }

    pub fn pair(&mut self, text: &str, hint: &str, values: (&mut f32, &mut f32), speed: f64) {
        self.custom(text, hint, |ui| {
            ui.horizontal(|ui| {
                let first = ui.add(DragValue::new(values.0).speed(speed).prefix("X  ").max_decimals(3)).changed();
                first | ui.add(DragValue::new(values.1).speed(speed).prefix("Y  ").max_decimals(3)).changed()
            })
            .inner
        });
    }

    /// `pair` over a two-number array.
    pub fn pair_of(&mut self, text: &str, hint: &str, values: &mut [f32; 2], speed: f64) {
        let [first, second] = values;
        self.pair(text, hint, (first, second), speed);
    }

    pub fn check(&mut self, text: &str, hint: &str, value: &mut bool) {
        self.custom(text, hint, |ui| ui.checkbox(value, "").changed());
    }

    pub fn text(&mut self, text: &str, hint: &str, value: &mut String) {
        self.custom(text, hint, |ui| ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY)).changed());
    }

    pub fn color(&mut self, text: &str, rgba: &mut [u8; 4]) {
        self.custom(text, "", |ui| {
            let mut color = Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            let changed = ui.color_edit_button_srgba(&mut color).changed();
            *rgba = color.to_srgba_unmultiplied();
            changed
        });
    }

    pub fn choice<T: PartialEq + Copy>(&mut self, text: &str, hint: &str, value: &mut T, options: &[(T, &str)]) {
        let current = options.iter().find(|(option, _)| option == value).map_or("", |(_, name)| *name);
        let id = self.ui.id().with(text);
        self.custom(text, hint, |ui| {
            let mut changed = false;
            egui::ComboBox::from_id_salt(id).selected_text(current).show_ui(ui, |ui| {
                for (option, name) in options {
                    changed |= ui.selectable_value(value, *option, *name).changed();
                }
            });
            changed
        });
    }
}
