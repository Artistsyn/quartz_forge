//! Inspector section for an object's typed shader effect
//! (`QuartzObjectBlueprint::effect`, see `core::object_effect`).

use eframe::egui::{self, Color32, Slider};

use crate::core::object_effect::{
    EffectKind, ImpactSideSpec, MarkerModeSpec, ObjectEffectSpec, ScreenEdgeSpec, VFX_FLAG_NAMES,
};
use crate::core::quartz_domain::QuartzObjectBlueprint;

/// Returns true if anything changed.
pub(super) fn object_effect_editor(ui: &mut egui::Ui, object: &mut QuartzObjectBlueprint) -> bool {
    let mut changed = false;
    let mut on = object.effect.is_some();
    if ui
        .checkbox(&mut on, "shader effect")
        .on_hover_text(
            "A typed engine effect attached to this object: it follows the object's \
             position and rotation and draws at its depth. One per object.",
        )
        .changed()
    {
        object.effect = on.then(ObjectEffectSpec::default);
        changed = true;
    }
    let id = object.id.clone();
    let Some(spec) = object.effect.as_mut() else { return changed };

    ui.indent(("effect", id.as_str()), |ui| {
        egui::ComboBox::from_id_salt(("effect_kind", id.as_str()))
            .selected_text(spec.kind.label())
            .show_ui(ui, |ui| {
                for kind in EffectKind::ALL {
                    changed |= ui.selectable_value(&mut spec.kind, kind, kind.label()).changed();
                }
            });
        if spec.kind.is_strip() {
            ui.small("Runs along the object's +x from its left edge; rotate the object to aim it.");
        }

        ui.horizontal(|ui| {
            let mut c = Color32::from_rgb(spec.rgb[0], spec.rgb[1], spec.rgb[2]);
            if ui.color_edit_button_srgba(&mut c).changed() {
                spec.rgb = [c.r(), c.g(), c.b()];
                changed = true;
            }
            ui.label("colour (sRGB, as the game shows it)");
        });
        changed |= ui
            .add(Slider::new(&mut spec.scale[0], 0.05..=8.0).text("width × object"))
            .changed();
        changed |= ui
            .add(Slider::new(&mut spec.scale[1], 0.05..=8.0).text("height × object"))
            .changed();
        changed |= ui
            .add(Slider::new(&mut spec.amount, 0.0..=1.0).text(spec.kind.amount_field()))
            .changed();

        let kind = spec.kind;
        if kind.takes("progress") {
            changed |= ui.add(Slider::new(&mut spec.progress, 0.0..=1.0).text("progress")).changed();
        }
        if kind.takes("snap") {
            let mut snapping = spec.snap.is_some();
            if ui.checkbox(&mut snapping, "snapping").changed() {
                spec.snap = snapping.then_some(0.0);
                changed = true;
            }
            if let Some(t) = spec.snap.as_mut() {
                changed |= ui.add(Slider::new(t, 0.0..=1.0).text("snap progress")).changed();
            }
        }
        if kind.takes("edge") {
            egui::ComboBox::from_id_salt(("effect_edge", id.as_str()))
                .selected_text(spec.edge.name())
                .show_ui(ui, |ui| {
                    for e in ScreenEdgeSpec::ALL {
                        changed |= ui.selectable_value(&mut spec.edge, e, e.name()).changed();
                    }
                });
        }
        if kind.takes("mode") {
            egui::ComboBox::from_id_salt(("effect_mode", id.as_str()))
                .selected_text(spec.mode.name())
                .show_ui(ui, |ui| {
                    for m in MarkerModeSpec::ALL {
                        changed |= ui.selectable_value(&mut spec.mode, m, m.name()).changed();
                    }
                });
        }
        if kind.takes("side") {
            egui::ComboBox::from_id_salt(("effect_side", id.as_str()))
                .selected_text(spec.side.name())
                .show_ui(ui, |ui| {
                    for s in ImpactSideSpec::ALL {
                        changed |= ui.selectable_value(&mut spec.side, s, s.name()).changed();
                    }
                });
        }
        if kind.takes("levels") {
            for (i, l) in spec.levels.iter_mut().enumerate() {
                changed |= ui.add(Slider::new(l, 0.0..=1.0).text(format!("quadrant {i}"))).changed();
            }
        }
        if kind.takes("flags") {
            ui.label("looks (combine freely)");
            ui.horizontal_wrapped(|ui| {
                for name in VFX_FLAG_NAMES {
                    let mut set = spec.flags.iter().any(|f| f == name);
                    if ui.checkbox(&mut set, name.to_lowercase()).changed() {
                        spec.flags.retain(|f| f != name);
                        if set {
                            spec.flags.push(name.to_owned());
                        }
                        changed = true;
                    }
                }
            });
        }
        ui.small("Previewed as its bounds in the scene view; the shader itself renders in-game.");
    });
    changed
}
