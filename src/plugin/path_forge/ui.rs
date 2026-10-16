//! The PathForge plugin's panel in the scene editor: the preview, walk / walk on / choose /
//! intro / restart, the hour, and the journey's stops.

use super::{Branch, PathForgePreview};
use crate::core::project::PathForgeBackground;
use std::path::PathBuf;

/// The panel's state, kept by the editor between frames.
pub struct Panel {
    preview: Option<PathForgePreview>,
    texture: Option<egui::TextureHandle>,
    walking: bool,
    last: Option<std::time::Instant>,
}

impl Default for Panel {
    fn default() -> Self { Panel { preview: None, texture: None, walking: true, last: None } }
}

impl Panel {
    /// Let go of the preview (the scene shown has no PathForge background).
    pub fn clear(&mut self) { self.preview = None; }

    /// The preview open now, if any.
    pub fn preview(&self) -> Option<&PathForgePreview> { self.preview.as_ref() }

    /// Whether the preview has drawn a frame.
    pub fn drew(&self) -> bool { self.texture.is_some() }

    /// The preview of `pf` (a scene's PathForge background), its controls and what it says.
    pub fn show(&mut self, ui: &mut egui::Ui, pf: &PathForgeBackground, root: Option<PathBuf>) {
        if let Err(e) = super::runtime_library() {
            if pf.mode == crate::core::project::PathForgeMode::Live {
                ui.label("PathForge preview:");
                ui.colored_label(egui::Color32::from_rgb(230, 170, 80), e);
                return;
            }
        }
        if !self.preview.as_ref().is_some_and(|p| p.is_for(pf, root.as_deref())) {
            self.preview = Some(PathForgePreview::open(pf, root.as_deref()));
            self.last = None;
        }
        let now = std::time::Instant::now();
        let dt = if self.walking { self.last.map_or(0.0, |t| now.duration_since(t).as_secs_f32()).min(0.1) } else { 0.0 };
        self.last = Some(now);
        let Some(p) = self.preview.as_mut() else { return };
        // A preview-sized frame in the render size's shape.
        let h = 240u32;
        let w = ((h as f32 * pf.size[0] as f32 / pf.size[1].max(1) as f32).round() as u32).max(2);
        if let Some((fw, fh, rgba)) = p.frame(dt, pf.speed, w, h) {
            let img = egui::ColorImage::from_rgba_unmultiplied([fw as usize, fh as usize], &rgba);
            match &mut self.texture {
                Some(t) => t.set(img, egui::TextureOptions::NEAREST),
                None => self.texture = Some(ui.ctx().load_texture("path_forge_preview", img, egui::TextureOptions::NEAREST)),
            }
        }
        ui.label("PathForge preview:");
        if let Some(e) = &p.error { ui.colored_label(egui::Color32::from_rgb(230, 90, 80), e); }
        if let Some(t) = &self.texture {
            let size = t.size_vec2();
            let k = (ui.available_width().min(320.0) / size.x).min(360.0 / size.y);
            ui.add(egui::Image::new(t).fit_to_exact_size(size * k));
        }
        let state = p.state();
        ui.horizontal(|ui| {
            if ui.button(if self.walking { "⏸ stand" } else { "▶ walk" }).clicked() { self.walking = !self.walking; }
            if state.is_some() {
                if ui.button("walk on (next)").on_hover_text("What RunPlugin path_forge \"next\" does: the transition or fork to the next stop").clicked() { p.next(); }
                let fork = state.as_ref().is_some_and(|s| s.choose_within.is_some());
                if ui.add_enabled(fork, egui::Button::new("choose left")).clicked() { p.choose(Branch::Left); }
                if ui.add_enabled(fork, egui::Button::new("choose right")).clicked() { p.choose(Branch::Right); }
            }
            if ui.small_button("✈ intro").on_hover_text("What RunPlugin path_forge \"intro\" does: the scene's camera flight in (camera.moves.fly_in) again").clicked() { p.restart_intro(); }
            if ui.small_button("⟲ restart").clicked() { p.restart(pf, root.as_deref()); }
        });
        if let Some(s) = &state {
            ui.small(format!(
                "{}{} · {:.1} m{}{}",
                s.stop.as_deref().map(|x| format!("stop {x} · ")).unwrap_or_default(),
                s.scene,
                s.distance,
                s.progress.map(|x| format!(" · walking on {:.0}%", x * 100.0)).unwrap_or_default(),
                s.choose_within.map(|x| format!(" · choose within {x:.0} m")).unwrap_or_default(),
            ));
        }
        if let Some(d) = p.day() {
            ui.horizontal(|ui| {
                let (h, m) = (d.hour.floor() as u32, ((d.hour.fract()) * 60.0) as u32);
                ui.small(format!("{} {h:02}:{m:02} · {} · daylight {:.0}%", if d.is_day { "☀" } else { "☾" }, d.phase, d.daylight * 100.0));
                let mut v = d.hour;
                if ui.add(egui::Slider::new(&mut v, 0.0..=24.0).show_value(false)).on_hover_text("Preview only: what RunPlugin path_forge time:<hour> does").changed() {
                    p.set_hour(Some(v));
                }
            });
        }
        if let Some(n) = &p.note { ui.small(n.clone()); }
        if !p.stops.is_empty() {
            egui::CollapsingHeader::new(format!("journey: {} stops", p.stops.len())).id_salt("pf_stops").show(ui, |ui| {
                for s in &p.stops { ui.small(s.clone()); }
            });
        }
        if self.walking { ui.ctx().request_repaint(); }
    }
}
