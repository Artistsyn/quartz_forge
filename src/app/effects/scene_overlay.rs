//! The scene view's effects: every shader effect and particle effect in view, drawn by the
//! engine's renderer (headless) into one transparent picture laid over the editor's drawing,
//! so the scene view shows what the game draws rather than outlines. The picture lies over
//! every object; in the game an effect draws at its object's depth.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use quartz_project::effect_engine::EffectBridge;
use quartz_project::object_effect::ObjectEffectSpec;
use quartz_project::particle_effect::{ParticleEffectAsset, ParticleShapeKind};
use quartz_project::particle_engine::ParticleBridge;
use wgpu_canvas::{Area, HeadlessRenderer, Item, ParticlePlacement, ParticleSprite, RgbaImage};

/// Where an object is on screen.
#[derive(Clone, Copy, Debug)]
pub(in crate::app) struct ScreenPlace {
    /// Its unturned box, in points.
    pub rect: egui::Rect,
    pub rotation_deg: f32,
    /// Points per scene pixel.
    pub zoom: f32,
}

/// The renderer, its picture, and what goes in it this frame.
pub(in crate::app) struct SceneEffects {
    renderer: Option<HeadlessRenderer>,
    renderer_size: (u32, u32),
    failed: Option<String>,
    texture: Option<egui::TextureHandle>,
    items: Vec<(Area, Item)>,
    view: egui::Rect,
    /// Points the renderer's frame spans (its pixels over `pixel_scale`).
    frame: (f32, f32),
    images: HashMap<String, Option<Arc<RgbaImage>>>,
    clock: f32,
    last_frame: Option<Instant>,
    /// Whether the scene view draws effects (off: their outlines).
    pub enabled: bool,
}

impl SceneEffects {
    /// Renderer sizes step by this, so resizing the panel rarely makes a new renderer.
    const STEP: u32 = 256;
    /// The longest side rendered; a bigger view is drawn smaller and stretched.
    const MAX_SIDE: f32 = 2048.0;

    pub fn new() -> Self {
        Self {
            renderer: None,
            renderer_size: (0, 0),
            failed: None,
            texture: None,
            items: Vec::new(),
            view: egui::Rect::NOTHING,
            frame: (1.0, 1.0),
            images: HashMap::new(),
            clock: 0.0,
            last_frame: None,
            enabled: true,
        }
    }

    /// Whether effects are drawn this frame (on, and a renderer can be made).
    pub fn is_drawing(&self) -> bool {
        self.enabled && self.failed.is_none()
    }

    /// Start a frame for the scene view at `view`.
    pub fn begin(&mut self, view: egui::Rect) {
        self.items.clear();
        self.view = view;
        let now = Instant::now();
        let step = self.last_frame.map_or(0.0, |last| now.duration_since(last).as_secs_f32().min(0.1));
        self.last_frame = Some(now);
        self.clock += step;
        let scale = (Self::MAX_SIDE / view.width().max(view.height()).max(1.0)).min(1.0);
        let wanted = (Self::stepped(view.width() * scale), Self::stepped(view.height() * scale));
        if wanted.0 > self.renderer_size.0 || wanted.1 > self.renderer_size.1 {
            self.renderer = None;
            self.renderer_size = (wanted.0.max(self.renderer_size.0), wanted.1.max(self.renderer_size.1));
        }
        self.frame = (self.renderer_size.0 as f32 / scale, self.renderer_size.1 as f32 / scale);
    }

    /// An object's shader effect, centred on it and turned with it.
    pub fn add_shader(&mut self, spec: &ObjectEffectSpec, place: ScreenPlace) {
        let size = EffectBridge::size(spec, (place.rect.width(), place.rect.height()));
        let mut sprite = EffectBridge::effect(spec, size).sprite(EffectBridge::color(spec));
        sprite.instance.world_position = self.uv(place.rect.center());
        sprite.instance.scale = (size.0 / self.frame.0, size.1 / self.frame.1);
        sprite.instance.rotation = place.rotation_deg.to_radians();
        self.items.push((Self::whole(), Item::MegaShaderSprite(sprite)));
    }

    /// A particle effect played on an object, `offset` scene pixels from its centre.
    pub fn add_particles(&mut self, asset: &ParticleEffectAsset, offset: [f32; 2], place: ScreenPlace, root: Option<&Path>) {
        let turn = place.rotation_deg.to_radians();
        let (sin, cos) = turn.sin_cos();
        let reach = egui::vec2(offset[0] * cos - offset[1] * sin, offset[0] * sin + offset[1] * cos) * place.zoom;
        let emitter = ParticleBridge::emitter(asset);
        let age = emitter.finished_after().map_or(self.clock, |end| self.clock % (end + 0.5));
        let placement = ParticlePlacement { origin: self.uv(place.rect.center() + reach), units: place.zoom / self.frame.1, rotation: turn, velocity: (0.0, 0.0), z_index: 0 };
        let image = self.image(asset, root);
        self.items.push((Self::whole(), Item::Particles(ParticleSprite { emitter, placement, age, image })));
    }

    /// Render this frame's effects and lay them over the view; nothing when there are none.
    pub fn paint(&mut self, ctx: &egui::Context, painter: &egui::Painter) {
        if !self.is_drawing() || self.items.is_empty() {
            return;
        }
        let items = std::mem::take(&mut self.items);
        let Some(renderer) = self.renderer() else { return };
        let (width, height, pixels) = renderer.render(items);
        let picture = egui::ColorImage::from_rgba_premultiplied([width as usize, height as usize], &pixels);
        let texture = match &mut self.texture {
            Some(texture) => {
                texture.set(picture, egui::TextureOptions::LINEAR);
                texture.clone()
            }
            None => self.texture.insert(ctx.load_texture("scene_effects", picture, egui::TextureOptions::LINEAR)).clone(),
        };
        let shown = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(self.view.width() / self.frame.0, self.view.height() / self.frame.1));
        painter.image(texture.id(), self.view, shown, egui::Color32::WHITE);
        ctx.request_repaint();
    }

    /// Why effects cannot be drawn here, once a renderer failed.
    pub fn failure(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    fn renderer(&mut self) -> Option<&mut HeadlessRenderer> {
        if self.renderer.is_none() {
            match pollster::block_on(HeadlessRenderer::new(self.renderer_size.0, self.renderer_size.1)) {
                Ok(renderer) => self.renderer = Some(renderer),
                Err(error) => self.failed = Some(format!("{error:?}")),
            }
        }
        self.renderer.as_mut()
    }

    fn image(&mut self, asset: &ParticleEffectAsset, root: Option<&Path>) -> Option<Arc<RgbaImage>> {
        let path = asset.look.image.trim();
        if asset.look.shape != ParticleShapeKind::Texture || path.is_empty() {
            return None;
        }
        let load = || root.and_then(|root| image::open(root.join(path)).ok()).map(|picture| Arc::new(picture.to_rgba8()));
        self.images.entry(path.to_owned()).or_insert_with(load).clone()
    }

    fn uv(&self, point: egui::Pos2) -> (f32, f32) {
        let local = point - self.view.min;
        (local.x / self.frame.0, local.y / self.frame.1)
    }

    fn stepped(side: f32) -> u32 {
        (side.max(1.0).ceil() as u32).div_ceil(Self::STEP) * Self::STEP
    }

    fn whole() -> Area {
        Area { offset: (0.0, 0.0), bounds: None }
    }
}

#[cfg(test)]
mod tests {
    use super::{SceneEffects, ScreenPlace};
    use quartz_project::object_effect::{EffectKind, ObjectEffectSpec};

    #[test]
    fn a_shader_effect_lights_its_place_in_the_view_and_nowhere_else() {
        let mut effects = SceneEffects::new();
        let view = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(600.0, 400.0));
        effects.begin(view);
        let place = ScreenPlace { rect: egui::Rect::from_min_size(egui::pos2(150.0, 100.0), egui::vec2(160.0, 120.0)), rotation_deg: 0.0, zoom: 1.0 };
        effects.add_shader(&ObjectEffectSpec { kind: EffectKind::EnergyDome, ..Default::default() }, place);
        let items = std::mem::take(&mut effects.items);
        let Some(renderer) = effects.renderer() else { return };
        let (width, _, pixels) = renderer.render(items);
        let alpha = |x: usize, y: usize| pixels[(y * width as usize + x) * 4 + 3];
        let lit = (60..180).flat_map(|y| (60..200).map(move |x| (x, y))).filter(|&(x, y)| alpha(x, y) > 20).count();
        assert!(lit > 500, "the dome draws over its object (view-local 50..210, 50..170): {lit}");
        assert_eq!(alpha(500, 350), 0, "clear away from it");
    }

    #[test]
    fn particles_play_at_their_object_turned_offset() {
        let mut effects = SceneEffects::new();
        let view = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0));
        effects.begin(view);
        effects.clock = 1.5;
        let mut fire = quartz_project::particle_effect::ParticleEffectAsset::new("fire");
        fire.look.additive = 1.0;
        let place = ScreenPlace { rect: egui::Rect::from_center_size(egui::pos2(300.0, 200.0), egui::vec2(40.0, 40.0)), rotation_deg: 90.0, zoom: 1.0 };
        effects.add_particles(&fire, [100.0, 0.0], place, None);
        let items = std::mem::take(&mut effects.items);
        let Some(renderer) = effects.renderer() else { return };
        let (width, _, pixels) = renderer.render(items);
        // Additive light: premultiplied with no alpha, so colour is what shows.
        let lit = |x0: usize, y0: usize| (y0..y0 + 80).flat_map(|y| (x0..x0 + 80).map(move |x| (x, y))).filter(|&(x, y)| pixels[(y * width as usize + x) * 4] > 40).count();
        assert!(lit(260, 260) > 50, "the offset turned 90 degrees puts the fire below the object: {}", lit(260, 260));
        assert!(lit(360, 160) < lit(260, 260) / 4, "not to its right: {}", lit(360, 160));
    }
}
