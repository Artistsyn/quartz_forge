//! A particle effect played by the engine's own renderer (headless) and shown as a picture, so
//! the editor shows exactly what the game draws: the same shader, blending and timing.

use std::collections::HashMap;
use std::sync::Arc;

use quartz_project::effect_engine::EffectBridge;
use quartz_project::object_effect::ObjectEffectSpec;
use quartz_project::particle_effect::ParticleEffectAsset;
use quartz_project::particle_engine::ParticleBridge;
use quartz_project::shader_asset::{ProjectEffectChoice, ProjectShaderAsset};
use quartz_project::shader_engine::ShaderBridge;
use wgpu_canvas::{Area, EffectColor, HeadlessRenderer, Image, Item, MegaShaderSprite, ParticlePlacement, ParticleSprite, ProjectSource, RgbaImage, ShapeType};

/// The backdrop the effect plays over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum Backdrop {
    Night,
    Grey,
    Day,
}

/// One source per shader name, kept while its text is unchanged: the renderer rebuilds a
/// project shader's pipeline only when it is handed a new source.
#[derive(Default)]
pub(in crate::app) struct SourceCache {
    sources: HashMap<String, Arc<ProjectSource>>,
}

/// A project shader to draw: the shader, the object's choice of it and the tint.
pub(in crate::app) struct ProjectLook<'a> {
    pub asset: &'a ProjectShaderAsset,
    pub choice: &'a ProjectEffectChoice,
    pub color: EffectColor,
}

/// The preview's picture and clock.
pub(in crate::app) struct EffectPreview {
    renderer: Option<HeadlessRenderer>,
    failed: Option<String>,
    texture: Option<egui::TextureHandle>,
    backdrops: [Arc<RgbaImage>; 3],
    pub age: f32,
    pub playing: bool,
    pub backdrop: Backdrop,
    /// Scene pixels shown across the preview's height (the zoom).
    pub scene_span: f32,
    /// Effect image (Texture shape), loaded by the panel from the project.
    pub image: Option<Arc<RgbaImage>>,
    /// Whether the object inspector plays its shader effect (off: no repaint cost while idle).
    pub shader_open: bool,
    sources: SourceCache,
}

impl SourceCache {
    /// The kept source for `name` when its text is `wgsl`, else a new one that replaces it.
    pub fn source(&mut self, name: &str, wgsl: String) -> Arc<ProjectSource> {
        if let Some(kept) = self.sources.get(name).filter(|kept| kept.wgsl == wgsl) {
            return kept.clone();
        }
        let fresh = Arc::new(ProjectSource { name: name.to_owned(), wgsl });
        self.sources.insert(name.to_owned(), fresh.clone());
        fresh
    }
}

impl Backdrop {
    pub const ALL: [(Self, &'static str); 3] = [(Self::Night, "Night"), (Self::Grey, "Grey"), (Self::Day, "Day")];

    fn index(self) -> usize {
        match self {
            Self::Night => 0,
            Self::Grey => 1,
            Self::Day => 2,
        }
    }
}

impl EffectPreview {
    pub const SIZE: (u32, u32) = (480, 320);

    pub fn new() -> Self {
        let plain = |rgb: [u8; 3]| Arc::new(RgbaImage::from_pixel(1, 1, image::Rgba([rgb[0], rgb[1], rgb[2], 255])));
        Self {
            renderer: None,
            failed: None,
            texture: None,
            backdrops: [plain([18, 20, 30]), plain([96, 98, 104]), plain([170, 196, 222])],
            age: 0.0,
            playing: true,
            backdrop: Backdrop::Night,
            scene_span: 360.0,
            image: None,
            shader_open: false,
            sources: SourceCache::default(),
        }
    }

    pub fn restart(&mut self) {
        self.age = 0.0;
    }

    /// Advance by `step` seconds when playing; an effect that has ended starts again.
    pub fn advance(&mut self, asset: &ParticleEffectAsset, step: f32) {
        if !self.playing {
            return;
        }
        self.age += step.clamp(0.0, 0.1);
        let emitter = ParticleBridge::emitter(asset);
        if emitter.finished_after().is_some_and(|end| self.age > end + 0.5) {
            self.age = 0.0;
        }
    }

    /// Draw `asset` at the current age and show it; an error (no GPU) is shown instead.
    pub fn show(&mut self, ui: &mut egui::Ui, asset: &ParticleEffectAsset) {
        let items = self.items(asset);
        self.show_items(ui, items);
    }

    /// An object's shader effect, drawn by the mega shader at the object's size and filling most
    /// of the picture; it animates on the renderer's own effect clock.
    /// A project shader the effect chooses is drawn from `shaders`.
    pub fn show_shader(&mut self, ui: &mut egui::Ui, spec: &ObjectEffectSpec, object_size: (f32, f32), shaders: &[ProjectShaderAsset]) {
        let project = spec.project.as_ref().and_then(|choice| shaders.iter().find(|asset| asset.name == choice.shader).map(|asset| (asset, choice)));
        let items = match project {
            Some((asset, choice)) => self.project_items(&ProjectLook { asset, choice, color: EffectBridge::color(spec) }, object_size),
            None => self.shader_items(spec, object_size),
        };
        self.show_items(ui, items);
        ui.ctx().request_repaint();
    }

    /// A project shader at `size` scene pixels, as the game draws it.
    pub fn show_project(&mut self, ui: &mut egui::Ui, look: &ProjectLook<'_>, size: (f32, f32)) {
        let items = self.project_items(look, size);
        self.show_items(ui, items);
        ui.ctx().request_repaint();
    }

    fn project_items(&mut self, look: &ProjectLook<'_>, size: (f32, f32)) -> Vec<(Area, Item)> {
        let source = self.sources.source(&look.asset.name, look.asset.wgsl());
        let sprite = ShaderBridge::effect(look.choice, Some(source)).sprite(look.color, 1.0);
        self.framed(sprite, size)
    }

    fn shader_items(&self, spec: &ObjectEffectSpec, object_size: (f32, f32)) -> Vec<(Area, Item)> {
        let size = EffectBridge::size(spec, object_size);
        self.framed(EffectBridge::effect(spec, size).sprite(EffectBridge::color(spec)), size)
    }

    /// `sprite` at `size`, centred and filling most of the picture, over the backdrop.
    fn framed(&self, mut sprite: MegaShaderSprite, size: (f32, f32)) -> Vec<(Area, Item)> {
        let aspect = Self::SIZE.0 as f32 / Self::SIZE.1 as f32;
        let span = (size.1.abs().max(size.0.abs() / aspect) / 0.7).max(1.0);
        sprite.instance.world_position = (0.5, 0.5);
        sprite.instance.scale = (size.0 / (span * aspect), size.1 / span);
        let whole = Area { offset: (0.0, 0.0), bounds: None };
        vec![(whole, self.backdrop_item()), (whole, Item::MegaShaderSprite(sprite))]
    }

    fn show_items(&mut self, ui: &mut egui::Ui, items: Vec<(Area, Item)>) {
        let pixels = match self.render(items) {
            Ok(pixels) => pixels,
            Err(error) => {
                ui.colored_label(crate::app::shell::ForgeTheme::WARN, format!("The preview cannot draw here: {error}"));
                return;
            }
        };
        let size = [Self::SIZE.0 as usize, Self::SIZE.1 as usize];
        let picture = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
        let texture = match &mut self.texture {
            Some(texture) => {
                texture.set(picture, egui::TextureOptions::LINEAR);
                texture.clone()
            }
            None => self.texture.insert(ui.ctx().load_texture("effect_preview", picture, egui::TextureOptions::LINEAR)).clone(),
        };
        let width = ui.available_width().min(Self::SIZE.0 as f32);
        ui.add(egui::Image::new(&texture).fit_to_exact_size(egui::vec2(width, width * Self::SIZE.1 as f32 / Self::SIZE.0 as f32)));
    }

    fn render(&mut self, items: Vec<(Area, Item)>) -> Result<Vec<u8>, String> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        if self.renderer.is_none() {
            match pollster::block_on(HeadlessRenderer::new(Self::SIZE.0, Self::SIZE.1)) {
                Ok(renderer) => self.renderer = Some(renderer),
                Err(error) => {
                    let text = format!("{error:?}");
                    self.failed = Some(text.clone());
                    return Err(text);
                }
            }
        }
        let Some(renderer) = self.renderer.as_mut() else { return Err("no renderer".to_owned()) };
        Ok(renderer.render(items).2)
    }

    fn items(&self, asset: &ParticleEffectAsset) -> Vec<(Area, Item)> {
        let whole = Area { offset: (0.0, 0.0), bounds: None };
        let backdrop = self.backdrop_item();
        let sprite = ParticleSprite {
            emitter: ParticleBridge::emitter(asset),
            placement: ParticlePlacement { origin: (0.5, 0.62), units: 1.0 / self.scene_span.max(1.0), rotation: 0.0, velocity: (0.0, 0.0), z_index: 0 },
            age: self.age,
            image: self.image.clone(),
        };
        vec![(whole, backdrop), (whole, Item::Particles(sprite))]
    }

    /// The picture of the current backdrop, for other engine pictures drawn over it.
    pub fn backdrop_item(&self) -> Item {
        Item::Image(Image {
            shape: ShapeType::Rectangle(0.0, (Self::SIZE.0 as f32, Self::SIZE.1 as f32), 0.0),
            image: self.backdrops[self.backdrop.index()].clone(),
            color: None,
            recolor: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{EffectPreview, ProjectLook};
    use quartz_project::object_effect::{EffectKind, ObjectEffectSpec};
    use quartz_project::particle_effect::ParticleEffectAsset;
    use quartz_project::shader_asset::ProjectShaderAsset;
    use wgpu_canvas::{EffectColor, Item};

    fn lit(pixels: &[u8]) -> usize {
        pixels.chunks_exact(4).filter(|rgba| rgba[0].max(rgba[1]).max(rgba[2]) > 60).count()
    }

    #[test]
    fn both_previews_draw_with_the_engine_renderer() {
        let mut preview = EffectPreview::new();
        let spec = ObjectEffectSpec { kind: EffectKind::ThrustPlume, ..Default::default() };
        let items = preview.shader_items(&spec, (200.0, 80.0));
        let Ok(shader) = preview.render(items) else { return };
        assert!(lit(&shader) > 500, "the plume lights the picture: {}", lit(&shader));
        let mut fire = ParticleEffectAsset::new("fire");
        fire.look.additive = 1.0;
        preview.age = 1.0;
        let items = preview.items(&fire);
        let particles = preview.render(items).unwrap_or_default();
        assert!(lit(&particles) > 100, "the particles light the picture: {}", lit(&particles));
    }

    #[test]
    fn a_layered_project_shader_draws_with_the_engine_renderer() {
        let mut preview = EffectPreview::new();
        let mut asset = ProjectShaderAsset::layered("glow_preview");
        asset.additive = 1.0;
        let choice = asset.choice();
        let look = ProjectLook { asset: &asset, choice: &choice, color: EffectColor::srgb8(255, 170, 80) };
        let items = preview.project_items(&look, (200.0, 200.0));
        let Ok(pixels) = preview.render(items) else { return };
        let problems = preview.renderer.as_ref().map(wgpu_canvas::HeadlessRenderer::project_shader_problems).unwrap_or_default();
        assert!(problems.is_empty(), "{problems:?}");
        assert!(lit(&pixels) > 2000, "the glow lights the picture: {}", lit(&pixels));
        let source = |items: &[(wgpu_canvas::Area, Item)]| match &items[1].1 {
            Item::MegaShaderSprite(sprite) => sprite.project_source.clone(),
            _ => None,
        };
        let first = source(&preview.project_items(&look, (200.0, 200.0)));
        let second = source(&preview.project_items(&look, (200.0, 200.0)));
        assert!(first.zip(second).is_some_and(|(left, right)| std::sync::Arc::ptr_eq(&left, &right)), "an unchanged shader keeps its source");
    }
}
