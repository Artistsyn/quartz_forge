//! The object inspector: every setting of the selected object in collapsible sections, each a
//! grid of labelled fields with hover help. Uncommon sections (surfaces, planets, screen
//! anchoring, code file) start closed.

use std::collections::HashMap;
use std::path::Path;

use eframe::egui::{self, DragValue, RichText};

use super::fields::Fields;
use super::shell::ForgeTheme;
use super::{effect_editor, AssetPreviewTextures, QuartzForgeApp, COLLISION_LAYER_FLAGS};
use crate::core::quartz_domain::{
    ObjectPhysicsMaterialPreset, ObjectPhysicsMaterialSpec, ObjectTemplate, ObjectVisualAssetMode, QuartzGravityFalloff,
    QuartzObjectBlueprint, QuartzObjectCollisionMode, ShadowShapeSpec,
};

/// What a placed object is to the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Placed,
    SpawnTemplate,
    Background,
}

/// The editor state the inspector reads besides the object: project folder, status line and
/// the image previews already loaded.
struct InspectorContext<'a> {
    root: Option<&'a Path>,
    status: &'a mut String,
    previews: &'a mut HashMap<String, AssetPreviewTextures>,
    /// Plays the object's shader effect beside its fields.
    effect_preview: &'a mut super::effects::EffectPreview,
    /// The scene's size, which prices an effect's fill.
    scene_size: (f32, f32),
    /// The project's own shaders, which an object's effect may use.
    shaders: &'a [crate::core::shader_asset::ProjectShaderAsset],
}

impl QuartzForgeApp {
    pub(super) fn object_details(&mut self, ui: &mut egui::Ui) {
        let scene_index = self.project_state.active_scene_index;
        let effect_names: Vec<String> = self.project_state.manifest.particle_effects.iter().map(|asset| asset.name.clone()).collect();
        let Some(scene) = self.project_state.manifest.scenes.get_mut(scene_index) else { return };
        let canvas = scene.canvas.clone();
        let Some(blueprint) = scene.objects.get_mut(self.selected_object_index) else {
            ui.label("No object selected.");
            return;
        };
        let mut context = InspectorContext { root: self.project_root.as_deref(), status: &mut self.status_line, previews: &mut self.asset_preview_cache, effect_preview: &mut self.effects.preview, scene_size: (canvas.virtual_width, canvas.virtual_height), shaders: &self.project_state.manifest.shaders };
        let mut changed = Self::identity_section(ui, blueprint);
        changed |= Fields::section(ui, "Transform", true, |ui| Self::transform_fields(ui, blueprint));
        changed |= Fields::section(ui, "Appearance", true, |ui| Self::appearance_fields(ui, blueprint, &mut context));
        changed |= Fields::section(ui, "Particle Effects", !blueprint.particles.is_empty(), |ui| super::effects::EffectForm::attached(ui, &mut blueprint.particles, &effect_names));
        changed |= Fields::section(ui, "Lighting", false, |ui| Self::lighting_fields(ui, blueprint));
        changed |= Fields::section(ui, "Physics", true, |ui| Self::physics_fields(ui, blueprint));
        changed |= Fields::section(ui, "Collision", true, |ui| Self::collision_fields(ui, blueprint));
        changed |= Fields::section(ui, "Surface and Slopes", false, |ui| Self::surface_fields(ui, blueprint));
        changed |= Fields::section(ui, "Planet Gravity", false, |ui| Self::planet_fields(ui, blueprint));
        changed |= Fields::section(ui, "Screen and HUD", false, |ui| Self::screen_fields(ui, blueprint));
        changed |= Fields::section(ui, "Code File", false, |ui| {
            Self::source_file_picker(ui, "File", context.root, context.status, &mut blueprint.output_file)
        });
        if changed {
            Self::apply_background_snap_if_needed(blueprint, &canvas);
            self.touch_and_refresh_preview();
        }
    }

    fn identity_section(ui: &mut egui::Ui, blueprint: &mut QuartzObjectBlueprint) -> bool {
        let mut role = match (blueprint.spawn_only, blueprint.is_background) {
            (true, _) => Role::SpawnTemplate,
            (_, true) => Role::Background,
            _ => Role::Placed,
        };
        let changed = Fields::grid(ui, "object_identity", |fields| {
            fields.text("Name", "Shown in the editor and used by events to find it", &mut blueprint.name);
            fields.text("ID", "The object's name in the game and in code; keep it unique", &mut blueprint.id);
            fields.custom("Tags", "Comma separated; events can target every object with a tag", |ui| Self::tags_field(ui, &mut blueprint.tags));
            let roles = [(Role::Placed, "Placed in the scene"), (Role::SpawnTemplate, "Spawn template"), (Role::Background, "Background")];
            fields.choice("Role", "Spawn templates are not placed; actions spawn copies of them", &mut role, &roles);
        });
        (blueprint.spawn_only, blueprint.is_background) = (role == Role::SpawnTemplate, role == Role::Background);
        ui.add_space(4.0);
        changed
    }

    fn tags_field(ui: &mut egui::Ui, tags: &mut Vec<String>) -> bool {
        let mut text = tags.join(", ");
        let changed = ui.add(egui::TextEdit::singleline(&mut text).hint_text("player, enemy").desired_width(f32::INFINITY)).changed();
        if changed {
            *tags = text.split(',').map(|tag| tag.trim().to_owned()).filter(|tag| !tag.is_empty()).collect();
        }
        changed
    }

    fn transform_fields(ui: &mut egui::Ui, blueprint: &mut QuartzObjectBlueprint) -> bool {
        let mut pinned = blueprint.advanced.is_camera_space_pinned();
        let changed = Fields::grid(ui, "object_transform", |fields| {
            fields.pair("Position", "Top-left corner in scene pixels", (&mut blueprint.x, &mut blueprint.y), 1.0);
            fields.pair("Size", "Width and height in scene pixels", (&mut blueprint.w, &mut blueprint.h), 1.0);
            fields.ranged("Rotation", "Degrees, clockwise", &mut blueprint.advanced.rotation_deg, -180.0..=180.0);
            fields.pair("Pivot", "The point it turns around: 0..1 across its width and height", (&mut blueprint.advanced.pivot_x, &mut blueprint.advanced.pivot_y), 0.01);
            fields.custom("Layer", "Higher layers draw on top", |ui| ui.add(DragValue::new(&mut blueprint.layer).range(-100..=100)).changed());
            fields.check("Lock position", "The scene view will not move or resize it", &mut blueprint.lock_transform);
            fields.choice("Space", "Screen objects stay where they are on screen as the camera moves (HUD)", &mut pinned, &[(false, "World"), (true, "Screen (pinned)")]);
        });
        if pinned != blueprint.advanced.is_camera_space_pinned() {
            blueprint.advanced.set_camera_space_pinned(pinned);
            return true;
        }
        changed
    }

    fn appearance_fields(ui: &mut egui::Ui, blueprint: &mut QuartzObjectBlueprint, context: &mut InspectorContext<'_>) -> bool {
        let mut changed = Fields::grid(ui, "object_appearance", |fields| {
            fields.choice("Shape", "", &mut blueprint.template, &[(ObjectTemplate::Rectangle, "Rectangle"), (ObjectTemplate::Circle, "Circle")]);
            fields.custom("Fill", "Drawn when there is no image", |ui| ui.color_edit_button_srgb(&mut blueprint.color_rgb).changed());
            let modes = [(ObjectVisualAssetMode::None, "Colour only"), (ObjectVisualAssetMode::StaticImage, "Image"), (ObjectVisualAssetMode::AnimatedSprite, "Animation (GIF)")];
            fields.choice("Visual", "", &mut blueprint.visual_asset_mode, &modes);
            if blueprint.visual_asset_mode == ObjectVisualAssetMode::AnimatedSprite {
                fields.ranged("Frame rate", "Animation frames per second", &mut blueprint.visual_asset_fps, 1.0..=60.0);
            }
        });
        if blueprint.visual_asset_mode != ObjectVisualAssetMode::None {
            changed |= Self::asset_picker(ui, context, blueprint);
        }
        changed |= Self::image_cache_fields(ui, blueprint);
        changed |= Self::glow_fields(ui, blueprint);
        ui.add_space(4.0);
        changed |= effect_editor::object_effect_editor(ui, blueprint, context.shaders);
        if let Some(spec) = &blueprint.effect {
            ui.checkbox(&mut context.effect_preview.shader_open, "Play the effect here").on_hover_text("Drawn by the engine's mega shader, exactly as in the game");
            if context.effect_preview.shader_open {
                context.effect_preview.show_shader(ui, spec, (blueprint.w, blueprint.h), context.shaders);
            }
            super::effects::EffectForm::findings(ui, &spec.check((blueprint.w, blueprint.h), context.scene_size, context.shaders));
        }
        changed
    }

    fn image_cache_fields(ui: &mut egui::Ui, blueprint: &mut QuartzObjectBlueprint) -> bool {
        if blueprint.visual_asset_mode != ObjectVisualAssetMode::StaticImage {
            return false;
        }
        Fields::grid(ui, "object_image_cache", |fields| {
            fields.check("Cache image", "Decode the image once and share it between objects", &mut blueprint.visual_asset_use_canvas_cache);
            if blueprint.visual_asset_use_canvas_cache {
                fields.text("Cache key", "Blank uses the image path", &mut blueprint.visual_asset_cache_key);
                fields.check("Key by size", "Keep one cached copy per object size", &mut blueprint.visual_asset_size_aware_cache);
            }
        })
    }

    fn glow_fields(ui: &mut egui::Ui, blueprint: &mut QuartzObjectBlueprint) -> bool {
        let advanced = &mut blueprint.advanced;
        Fields::grid(ui, "object_glow", |fields| {
            fields.check("Tint", "Multiply the image by a colour", &mut advanced.tint_enabled);
            if advanced.tint_enabled {
                fields.color("Tint colour", &mut advanced.tint_rgba);
            }
            fields.check("Glow", "An outline of light around the object", &mut advanced.glow_enabled);
            if advanced.glow_enabled {
                fields.color("Glow colour", &mut advanced.glow_rgba);
                fields.ranged("Glow width", "Pixels", &mut advanced.glow_width, 0.0..=64.0);
                fields.check("Boxed glow", "Stroke the glow as a rectangle (UI frames); off follows the image's visible pixels", &mut advanced.glow_boxed);
                fields.check("Bloom", "The glow is light: unlit and bloomed; off draws a flat highlight", &mut advanced.glow_bloom);
            }
        })
    }

    fn lighting_fields(ui: &mut egui::Ui, blueprint: &mut QuartzObjectBlueprint) -> bool {
        Fields::grid(ui, "object_lighting", |fields| {
            fields.check("Unlit", "Not affected by scene lights; good for large backgrounds", &mut blueprint.unlit);
            fields.check("Casts shadow", "Blocks light and casts shadows", &mut blueprint.casts_shadow);
            if blueprint.casts_shadow {
                let shapes: Vec<(ShadowShapeSpec, &str)> = ShadowShapeSpec::ALL.iter().map(|shape| (*shape, shape.label())).collect();
                fields.choice("Shadow shape", "Box: the rectangle; Circle: the inscribed disc; Pixel outline: the image's own silhouette", &mut blueprint.shadow_shape, &shapes);
            }
        })
    }

    fn physics_fields(ui: &mut egui::Ui, blueprint: &mut QuartzObjectBlueprint) -> bool {
        let advanced = &mut blueprint.advanced;
        let mut preset = advanced.material.preset;
        let mut changed = Fields::grid(ui, "object_physics", |fields| {
            fields.ranged("Gravity", "Added to downward speed every frame; 0 floats", &mut advanced.gravity, -5.0..=5.0);
            fields.pair("Resistance", "Speed kept each frame: 1 keeps all (no friction), 0.9 slows, 0 stops at once", (&mut advanced.resistance_x, &mut advanced.resistance_y), 0.01);
            fields.pair("Start speed", "Momentum when the scene starts, pixels per frame", (&mut advanced.momentum_x, &mut advanced.momentum_y), 0.1);
            fields.ranged("Spin resistance", "Turning speed kept each frame", &mut advanced.rotation_resistance, 0.0..=1.0);
            let presets: Vec<(ObjectPhysicsMaterialPreset, &str)> = ObjectPhysicsMaterialPreset::ALL.iter().map(|preset| (*preset, preset.as_str())).collect();
            fields.choice("Material", "How it bounces, slides and weighs when bodies touch", &mut preset, &presets);
        });
        if preset != advanced.material.preset {
            advanced.material = ObjectPhysicsMaterialSpec::resolved_defaults(preset);
            changed = true;
        }
        if advanced.material.preset == ObjectPhysicsMaterialPreset::Custom {
            changed |= Fields::grid(ui, "object_material", |fields| {
                fields.ranged("Elasticity", "Bounce: 0 none, 1 keeps all its speed", &mut advanced.material.elasticity, 0.0..=1.5);
                fields.ranged("Friction", "", &mut advanced.material.friction, 0.0..=2.0);
                fields.ranged("Density", "Mass per area", &mut advanced.material.density, 0.0..=10.0);
            });
        }
        changed
    }

    fn collision_fields(ui: &mut egui::Ui, blueprint: &mut QuartzObjectBlueprint) -> bool {
        let advanced = &mut blueprint.advanced;
        let mut mode = advanced.collision_mode;
        let modes = [
            (QuartzObjectCollisionMode::Auto, "Automatic"),
            (QuartzObjectCollisionMode::NonPlatform, "Passes through"),
            (QuartzObjectCollisionMode::Surface, "Surface"),
            (QuartzObjectCollisionMode::SolidRectangle, "Solid box"),
            (QuartzObjectCollisionMode::SolidCircle, "Solid circle"),
        ];
        let mut changed = Fields::grid(ui, "object_collision", |fields| {
            fields.choice("Collision", "How other objects meet it", &mut mode, &modes);
            if mode == QuartzObjectCollisionMode::SolidCircle {
                fields.number("Circle radius", "0 uses half the width", &mut advanced.collision_circle_radius, 1.0);
            }
            fields.check("Static body", "Never moved by collisions (floors, walls)", &mut advanced.is_platform);
            fields.custom("Exists on", "The layers this object is on", |ui| Self::layer_bits(ui, &mut advanced.collision_layer));
            fields.custom("Collides with", "The layers it meets; others pass through", |ui| Self::layer_bits(ui, &mut advanced.collision_mask));
            fields.check("Clip drawing", "Draw only inside a rectangle", &mut advanced.clip_enabled);
            if advanced.clip_enabled {
                fields.check("Clip origin", "Set the clip rectangle's corner", &mut advanced.clip_origin_enabled);
                if advanced.clip_origin_enabled {
                    fields.pair("Origin", "", (&mut advanced.clip_origin_x, &mut advanced.clip_origin_y), 1.0);
                }
                fields.check("Clip size", "Set the clip rectangle's size", &mut advanced.clip_size_enabled);
                if advanced.clip_size_enabled {
                    fields.pair("Size", "", (&mut advanced.clip_size_w, &mut advanced.clip_size_h), 1.0);
                }
            }
        });
        if mode != advanced.collision_mode {
            advanced.collision_mode = mode;
            match mode {
                QuartzObjectCollisionMode::NonPlatform => advanced.is_platform = false,
                QuartzObjectCollisionMode::Auto => {}
                QuartzObjectCollisionMode::Surface | QuartzObjectCollisionMode::SolidRectangle | QuartzObjectCollisionMode::SolidCircle => advanced.is_platform = true,
            }
            changed = true;
        }
        changed
    }

    /// A layer mask as a dropdown of named layers with checkboxes.
    fn layer_bits(ui: &mut egui::Ui, bits: &mut u32) -> bool {
        let names: Vec<&str> = COLLISION_LAYER_FLAGS.iter().filter(|(_, bit)| *bits & bit != 0).map(|(name, _)| *name).collect();
        let summary = match (*bits, names.len()) {
            (0, _) => "None".to_owned(),
            (u32::MAX, _) => "All".to_owned(),
            (_, 0) => format!("bits {bits}"),
            _ => names.join(", ").to_lowercase(),
        };
        let mut changed = false;
        egui::ComboBox::from_id_salt(ui.id().with(bits as *const u32 as usize)).selected_text(summary).show_ui(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("All").clicked() {
                    *bits = u32::MAX;
                    changed = true;
                }
                if ui.button("None").clicked() {
                    *bits = 0;
                    changed = true;
                }
            });
            for (name, bit) in COLLISION_LAYER_FLAGS {
                let mut on = *bits & bit != 0;
                if ui.checkbox(&mut on, name.to_lowercase()).changed() {
                    *bits = if on { *bits | bit } else { *bits & !bit };
                    changed = true;
                }
            }
        });
        changed
    }

    fn surface_fields(ui: &mut egui::Ui, blueprint: &mut QuartzObjectBlueprint) -> bool {
        let advanced = &mut blueprint.advanced;
        Fields::grid(ui, "object_surface", |fields| {
            fields.check("One-way", "Can be jumped through from below", &mut advanced.one_way);
            fields.check("Slope", "The top edge slopes between two heights", &mut advanced.slope_enabled);
            if advanced.slope_enabled {
                fields.pair("Edge offsets", "Left and right top-edge heights, pixels", (&mut advanced.slope_left_offset, &mut advanced.slope_right_offset), 1.0);
                fields.check("Turn with slope", "Rotate the object to match the slope", &mut advanced.slope_auto_rotation);
            }
            fields.check("Conveyor", "Moves what stands on it", &mut advanced.surface_velocity_enabled);
            if advanced.surface_velocity_enabled {
                fields.number("Conveyor speed", "Pixels per frame, sideways", &mut advanced.surface_velocity_x, 0.1);
            }
            fields.check("Custom normal", "The direction the surface faces", &mut advanced.surface_normal_enabled);
            if advanced.surface_normal_enabled {
                fields.pair("Normal", "", (&mut advanced.surface_normal_x, &mut advanced.surface_normal_y), 0.01);
            }
            fields.check("Align to slopes", "Turn to match the ground it stands on", &mut advanced.align_to_slope);
            if advanced.align_to_slope {
                fields.ranged("Align speed", "", &mut advanced.align_to_slope_speed, 0.0..=64.0);
            }
        })
    }

    fn planet_fields(ui: &mut egui::Ui, blueprint: &mut QuartzObjectBlueprint) -> bool {
        let advanced = &mut blueprint.advanced;
        let falloffs = [(QuartzGravityFalloff::Constant, "Constant"), (QuartzGravityFalloff::Linear, "Linear"), (QuartzGravityFalloff::InverseSquare, "Inverse square")];
        Fields::grid(ui, "object_planet", |fields| {
            fields.check("Planet body", "A round body others can stand on all around", &mut advanced.planet_enabled);
            if advanced.planet_enabled {
                fields.number("Planet radius", "", &mut advanced.planet_radius, 1.0);
            }
            fields.check("Pulled by tag", "Fall toward objects with this tag instead of down", &mut advanced.gravity_target_enabled);
            if advanced.gravity_target_enabled {
                fields.text("Target tag", "", &mut advanced.gravity_target_tag);
            }
            fields.ranged("Pull strength", "", &mut advanced.gravity_strength, 0.0..=128.0);
            fields.ranged("Reach", "How far the pull extends, as a multiple of the size", &mut advanced.gravity_influence_mult, 0.01..=100.0);
            fields.choice("Falloff", "How the pull weakens with distance", &mut advanced.gravity_falloff, &falloffs);
            fields.check("All sources", "Feel every pulling body, not only the nearest", &mut advanced.gravity_all_sources);
            fields.check("Pull identity", "Only bodies with this identity pull it", &mut advanced.gravity_identity_enabled);
            if advanced.gravity_identity_enabled {
                fields.text("Identity", "", &mut advanced.gravity_identity);
            }
            fields.check("Stand upright", "Turn so its feet point at the planet", &mut advanced.auto_align);
            if advanced.auto_align {
                fields.ranged("Turn speed", "", &mut advanced.auto_align_speed, 0.0..=64.0);
                fields.ranged("Threshold", "", &mut advanced.auto_align_threshold, 0.0..=1.0);
                fields.ranged("Min depth", "", &mut advanced.auto_align_min_depth, 0.0..=1.0);
            }
        })
    }

    fn screen_fields(ui: &mut egui::Ui, blueprint: &mut QuartzObjectBlueprint) -> bool {
        let advanced = &mut blueprint.advanced;
        let changed = Fields::grid(ui, "object_screen", |fields| {
            fields.check("Pin to screen", "Hold a place on screen, anchored to a corner or edge", &mut advanced.screen_pin_enabled);
            if advanced.screen_pin_enabled {
                fields.pair("Anchor", "0..1 across and down the screen", (&mut advanced.screen_pin_anchor_x, &mut advanced.screen_pin_anchor_y), 0.01);
                fields.pair("Offset", "Pixels from the anchor", (&mut advanced.screen_pin_offset_x, &mut advanced.screen_pin_offset_y), 1.0);
            } else {
                fields.check("Screen space", "Positioned in screen pixels, not the world", &mut advanced.screen_space);
                if !advanced.screen_space {
                    fields.check("Ignore zoom", "Keep its size when the camera zooms", &mut advanced.ignore_zoom);
                }
            }
        });
        if advanced.screen_pin_enabled {
            advanced.screen_space = false;
        }
        if advanced.screen_pin_enabled || advanced.screen_space {
            advanced.ignore_zoom = true;
        }
        changed
    }

    /// The object's image: a thumbnail, a gallery of the project's images, and import.
    fn asset_picker(ui: &mut egui::Ui, context: &mut InspectorContext<'_>, blueprint: &mut QuartzObjectBlueprint) -> bool {
        let mut chosen = None;
        ui.horizontal(|ui| {
            Self::thumbnail(ui, context, &blueprint.visual_asset_path, 56.0);
            ui.vertical(|ui| {
                let name = if blueprint.visual_asset_path.is_empty() { "No image chosen" } else { blueprint.visual_asset_path.as_str() };
                ui.add(egui::Label::new(RichText::new(name).small()).truncate());
                ui.horizontal(|ui| {
                    ui.menu_button("Choose…", |ui| chosen = Self::asset_gallery(ui, context));
                    if ui.button("Import…").on_hover_text("Copy an image file into the project's assets").clicked() {
                        chosen = Self::import_image(context);
                    }
                    if !blueprint.visual_asset_path.is_empty() && ui.button("Clear").clicked() {
                        chosen = Some(String::new());
                    }
                });
            });
        });
        let Some(path) = chosen else { return false };
        blueprint.visual_asset_path = path;
        true
    }

    fn asset_gallery(ui: &mut egui::Ui, context: &mut InspectorContext<'_>) -> Option<String> {
        let files = context.root.map(Self::collect_asset_files).unwrap_or_default();
        if files.is_empty() {
            ui.label(RichText::new("No images in the project yet. Use Import….").color(ForgeTheme::MUTED));
            return None;
        }
        let mut chosen = None;
        egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
            egui::Grid::new("asset_gallery").spacing([8.0, 8.0]).show(ui, |ui| {
                for (index, file) in files.iter().enumerate() {
                    let label = Path::new(file).file_name().map_or_else(|| file.clone(), |name| name.to_string_lossy().into_owned());
                    let clicked = ui.vertical(|ui| {
                        let picked = Self::thumbnail(ui, context, file, 72.0).interact(egui::Sense::click()).clicked();
                        ui.add(egui::Label::new(RichText::new(label).small()).truncate());
                        picked
                    });
                    if clicked.inner {
                        chosen = Some(file.clone());
                        ui.close();
                    }
                    if index % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
        });
        chosen
    }

    fn thumbnail(ui: &mut egui::Ui, context: &mut InspectorContext<'_>, relative: &str, side: f32) -> egui::Response {
        let size = egui::vec2(side, side);
        let texture = context.root.filter(|_| !relative.is_empty()).and_then(|root| {
            let path = root.join(relative);
            let key = path.to_string_lossy().replace('\\', "/");
            if !context.previews.contains_key(&key) {
                let loaded = Self::load_static_texture(ui.ctx(), &path)?;
                context.previews.insert(key.clone(), loaded);
            }
            match context.previews.get(&key)? {
                AssetPreviewTextures::Static(texture) => Some(texture.id()),
                AssetPreviewTextures::Animated(frames) => frames.first().map(egui::TextureHandle::id),
            }
        });
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
        ui.painter().rect_filled(rect, 4.0, ForgeTheme::CANVAS);
        if let Some(texture) = texture {
            egui::Image::new((texture, size)).maintain_aspect_ratio(true).paint_at(ui, rect.shrink(3.0));
        }
        ui.painter().rect_stroke(rect, 4.0, egui::Stroke::new(1.0, ForgeTheme::BORDER), egui::StrokeKind::Inside);
        response
    }

    fn import_image(context: &mut InspectorContext<'_>) -> Option<String> {
        let Some(root) = context.root else {
            *context.status = "Open or create a project before importing images.".to_owned();
            return None;
        };
        let file = rfd::FileDialog::new().set_title("Import an image").add_filter("Image or GIF", &["png", "jpg", "jpeg", "bmp", "webp", "gif"]).pick_file()?;
        Self::import_asset_into_project(root, &file, context.status)
    }
}
