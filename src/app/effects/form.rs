//! The fields of a particle effect, grouped the way an artist thinks of it: when particles come
//! out, where from, how they fly, and how they look.

use quartz_project::particle_effect::{EffectFinding, FindingLevel, LifeKeyData, LookData, EmissionMode, EmitAreaKind, ObjectParticles, ParticleEffectAsset, ParticleShapeKind};

use crate::app::fields::Fields;

/// Edits one particle effect.
pub(in crate::app) struct EffectForm;

impl EffectForm {
    /// Show every field; true when something changed.
    pub fn show(ui: &mut egui::Ui, asset: &mut ParticleEffectAsset) -> bool {
        let mut changed = Fields::section(ui, "Release", true, |ui| Self::release(ui, asset));
        changed |= Fields::section(ui, "Start area", false, |ui| Self::area(ui, asset));
        changed |= Fields::section(ui, "Flight", true, |ui| Self::flight(ui, asset));
        changed |= Fields::section(ui, "Look", true, |ui| Self::look(ui, asset));
        changed |= Fields::section(ui, "Over life", !asset.look.keys.is_empty(), |ui| Self::life_keys(ui, &mut asset.look));
        changed
    }

    /// A cost check's findings, worst first, coloured by level.
    pub fn findings(ui: &mut egui::Ui, findings: &[EffectFinding]) {
        for finding in findings {
            let (color, mark) = match finding.level {
                FindingLevel::Block => (crate::app::shell::ForgeTheme::BAD, "✖ Not played: "),
                FindingLevel::Warn => (crate::app::shell::ForgeTheme::WARN, "▲ "),
                FindingLevel::Info => (crate::app::shell::ForgeTheme::MUTED, ""),
            };
            ui.add(egui::Label::new(egui::RichText::new(format!("{mark}{}", finding.message)).color(color)).wrap());
        }
    }

    /// The effects an object plays: each picked from the project's effects, with an offset from
    /// the object's centre. True when something changed.
    pub fn attached(ui: &mut egui::Ui, attached: &mut Vec<ObjectParticles>, names: &[String]) -> bool {
        let mut changed = false;
        let mut remove = None;
        for (index, entry) in attached.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let known = names.contains(&entry.effect);
                egui::ComboBox::from_id_salt(("object_particles", index)).selected_text(if entry.effect.is_empty() { "choose…" } else { &entry.effect }).show_ui(ui, |ui| {
                    for name in names {
                        changed |= ui.selectable_value(&mut entry.effect, name.clone(), name).changed();
                    }
                });
                changed |= ui.add(egui::DragValue::new(&mut entry.offset[0]).prefix("x ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut entry.offset[1]).prefix("y ")).changed();
                if ui.small_button("✖").on_hover_text("Stop playing this effect on the object").clicked() {
                    remove = Some(index);
                }
                if !known && !entry.effect.is_empty() {
                    ui.colored_label(crate::app::shell::ForgeTheme::BAD, "no such effect");
                }
            });
        }
        if let Some(index) = remove {
            attached.remove(index);
            changed = true;
        }
        if names.is_empty() {
            ui.label("The project has no effects yet; make one in the Effects tab.");
        } else if ui.button("+ Play an effect on this object").clicked() {
            attached.push(ObjectParticles { effect: names[0].clone(), offset: [0.0, 0.0] });
            changed = true;
        }
        changed
    }

    fn release(ui: &mut egui::Ui, asset: &mut ParticleEffectAsset) -> bool {
        let emission = &mut asset.emission;
        Fields::grid(ui, "effect_release", |fields| {
            fields.choice("Mode", "A stream releases steadily; a burst releases many at once", &mut emission.mode, &[(EmissionMode::Stream, "Stream"), (EmissionMode::Burst, "Burst")]);
            if emission.mode == EmissionMode::Stream {
                fields.number("Rate", "Particles a second", &mut emission.rate, 1.0);
                Self::optional(fields, "Stops after", "Seconds before it stops releasing; off runs for ever", &mut emission.duration, 1.0);
            } else {
                fields.custom("Count", "Particles in each burst", |ui| ui.add(egui::DragValue::new(&mut emission.count).range(0..=100_000)).changed());
                Self::optional(fields, "Repeats every", "Seconds between bursts; off bursts once", &mut emission.every, 0.5);
            }
            fields.pair_of("Lifetime", "Each particle lives a random time in this range, seconds", &mut emission.lifetime, 0.02);
            fields.custom("Seed", "Change to get a different random pattern", |ui| ui.add(egui::DragValue::new(&mut asset.seed)).changed());
        })
    }

    fn area(ui: &mut egui::Ui, asset: &mut ParticleEffectAsset) -> bool {
        let area = &mut asset.area;
        Fields::grid(ui, "effect_area", |fields| {
            let kinds = [(EmitAreaKind::Point, "Point"), (EmitAreaKind::Circle, "Circle"), (EmitAreaKind::Box, "Box"), (EmitAreaKind::Line, "Line")];
            fields.choice("Shape", "Where in the emitter a particle starts", &mut area.kind, &kinds);
            match area.kind {
                EmitAreaKind::Point => {}
                EmitAreaKind::Circle => fields.number("Radius", "Scene pixels", &mut area.size[0], 1.0),
                EmitAreaKind::Box => fields.pair_of("Size", "Width and height, scene pixels", &mut area.size, 1.0),
                EmitAreaKind::Line => fields.number("Length", "Scene pixels, across", &mut area.size[0], 1.0),
            }
        })
    }

    fn flight(ui: &mut egui::Ui, asset: &mut ParticleEffectAsset) -> bool {
        let motion = &mut asset.motion;
        Fields::grid(ui, "effect_flight", |fields| {
            fields.pair_of("Speed", "Launch speed range, scene pixels a second", &mut motion.speed, 1.0);
            fields.ranged("Direction", "Degrees: 0 right, 90 down, -90 up", &mut motion.direction, -180.0..=180.0);
            fields.ranged("Spread", "Width of the launch cone in degrees (360: every way)", &mut motion.spread, 0.0..=360.0);
            fields.pair_of("Gravity", "Acceleration, scene pixels a second squared (positive y falls)", &mut motion.gravity, 2.0);
            fields.ranged("Drag", "How fast particles lose speed, per second", &mut motion.drag, 0.0..=10.0);
            fields.pair_of("Spin", "Turn rate range, degrees a second", &mut motion.spin, 2.0);
            fields.ranged("Follow", "1 carries particles along with a moving object; 0 leaves them behind as a trail", &mut motion.follow, 0.0..=1.0);
        })
    }

    fn look(ui: &mut egui::Ui, asset: &mut ParticleEffectAsset) -> bool {
        let look = &mut asset.look;
        Fields::grid(ui, "effect_look", |fields| {
            let shapes = [
                (ParticleShapeKind::SoftDot, "Soft dot"),
                (ParticleShapeKind::Disc, "Disc"),
                (ParticleShapeKind::Square, "Square"),
                (ParticleShapeKind::Ring, "Ring"),
                (ParticleShapeKind::Star, "Star"),
                (ParticleShapeKind::Texture, "Image"),
            ];
            fields.choice("Shape", "Drawn shapes batch with every other effect; an image draws on its own", &mut look.shape, &shapes);
            if look.shape == ParticleShapeKind::Texture {
                fields.text("Image", "Project-relative path of the particle picture", &mut look.image);
            }
            fields.pair_of("Size", "Diameter at birth and at death, scene pixels", &mut look.size, 0.5);
            fields.ranged("Size jitter", "Random size change per particle", &mut look.size_jitter, 0.0..=1.0);
            fields.color("Colour at birth", &mut look.color_start);
            fields.color("Colour at death", &mut look.color_end);
            fields.pair_of("Fade in / out", "Shares of the life spent fading in and out", &mut look.fade, 0.01);
            fields.ranged("Additive", "0 covers what is behind; 1 adds light (fire, sparks, magic)", &mut look.additive, 0.0..=1.0);
            fields.ranged("Stretch", "Lengthens particles along their flight (sparks, rain), seconds of travel", &mut look.stretch, 0.0..=0.2);
        })
    }

    /// Up to two keys between birth and death, each setting the colour, the size or both.
    fn life_keys(ui: &mut egui::Ui, look: &mut LookData) -> bool {
        ui.small("Moments between birth and death that set the colour, the size or both; the look runs in straight lines through them. Two at most: the shader reads four keys in all.");
        let mut changed = false;
        let mut removed = None;
        for (index, key) in look.keys.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                ui.separator();
                changed |= Fields::grid(ui, "life_key", |fields| {
                    fields.custom("At", "Share of the life: 0 birth, 1 death", |ui| ui.add(egui::Slider::new(&mut key.at, 0.0..=1.0).clamping(egui::SliderClamping::Always)).changed());
                    fields.custom("Colour", "Off: on the line between its neighbours", |ui| Self::optional_color(ui, &mut key.color));
                    fields.custom("Size", "Off: on the line between its neighbours", |ui| Self::optional_size(ui, &mut key.size));
                });
                if ui.small_button("Remove this key").clicked() {
                    removed = Some(index);
                }
            });
        }
        if let Some(index) = removed {
            look.keys.remove(index);
            changed = true;
        }
        if look.keys.len() < 2 && ui.button("+ Key over life").clicked() {
            let at = if look.keys.is_empty() { 0.5 } else { 0.75 };
            let middle = std::array::from_fn(|lane| ((u16::from(look.color_start[lane]) + u16::from(look.color_end[lane])) / 2) as u8);
            look.keys.push(LifeKeyData { at, color: Some(middle), size: None });
            changed = true;
        }
        changed
    }

    fn optional_color(ui: &mut egui::Ui, color: &mut Option<[u8; 4]>) -> bool {
        ui.horizontal(|ui| {
            let mut on = color.is_some();
            let mut changed = ui.checkbox(&mut on, "").changed();
            if changed {
                *color = on.then_some(color.unwrap_or([255, 255, 255, 255]));
            }
            if let Some(rgba) = color {
                let mut picked = egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
                if ui.color_edit_button_srgba(&mut picked).changed() {
                    *rgba = picked.to_srgba_unmultiplied();
                    changed = true;
                }
            }
            changed
        })
        .inner
    }

    fn optional_size(ui: &mut egui::Ui, size: &mut Option<f32>) -> bool {
        ui.horizontal(|ui| {
            let mut on = size.is_some();
            let mut changed = ui.checkbox(&mut on, "").changed();
            if changed {
                *size = on.then_some(size.unwrap_or(16.0));
            }
            if let Some(diameter) = size {
                changed |= ui.add(egui::DragValue::new(diameter).speed(0.5).range(0.0..=4096.0).suffix(" px")).changed();
            }
            changed
        })
        .inner
    }

    fn optional(fields: &mut Fields<'_>, text: &str, hint: &str, value: &mut Option<f32>, fallback: f32) {
        fields.custom(text, hint, |ui| {
            let mut on = value.is_some();
            let mut changed = ui.checkbox(&mut on, "").changed();
            if changed {
                *value = on.then_some(value.unwrap_or(fallback));
            }
            if let Some(seconds) = value {
                changed |= ui.add(egui::DragValue::new(seconds).speed(0.05).range(0.0..=3600.0).suffix(" s")).changed();
            }
            changed
        });
    }
}
