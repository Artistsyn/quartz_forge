//! The layer cards' thumbnails: each shows the stack drawn up to and including its layer, by the
//! engine's renderer, all in one picture cut into cells. A shader's source changes on every
//! slider step, so the thumbnails follow at most a few times a second; each cell's pipeline is
//! rebuilt only when its own text changed (an edit rebuilds its layer and those above it).

use std::time::{Duration, Instant};

use quartz_project::shader_asset::{ProjectShaderAsset, ShaderAuthoring};
use quartz_project::shader_engine::ShaderBridge;
use wgpu_canvas::{Area, EffectColor, HeadlessRenderer, Item};

use crate::app::effects::preview::SourceCache;

/// The thumbnails of the open layered shader.
pub(in crate::app) struct LayerThumbs {
    renderer: Option<HeadlessRenderer>,
    /// Columns and rows the renderer was made for.
    grid: (u32, u32),
    sources: SourceCache,
    textures: Vec<egui::TextureHandle>,
    /// The shader name and source hash the textures show.
    shown: Option<(String, u64)>,
    drawn_at: Option<Instant>,
    failed: Option<String>,
}

impl LayerThumbs {
    const CELL: u32 = 64;
    const COLUMNS: u32 = 8;
    const EVERY: Duration = Duration::from_millis(200);

    pub fn new() -> Self {
        Self { renderer: None, grid: (0, 0), sources: SourceCache::default(), textures: Vec::new(), shown: None, drawn_at: None, failed: None }
    }

    /// The thumbnails for `asset`, redrawn when its source changed and the last draw is old enough.
    pub fn textures(&mut self, ctx: &egui::Context, asset: &ProjectShaderAsset, tint: [u8; 3]) -> &[egui::TextureHandle] {
        let wanted = (asset.name.clone(), asset.source_hash());
        let due = self.drawn_at.is_none_or(|at| at.elapsed() >= Self::EVERY);
        if self.shown.as_ref() != Some(&wanted) {
            if due && self.failed.is_none() {
                self.draw(ctx, asset, tint);
                self.shown = Some(wanted);
            } else {
                ctx.request_repaint_after(Self::EVERY);
            }
        }
        &self.textures
    }

    #[cfg(test)]
    pub fn count(&self) -> usize {
        self.textures.len()
    }

    fn draw(&mut self, ctx: &egui::Context, asset: &ProjectShaderAsset, tint: [u8; 3]) {
        self.drawn_at = Some(Instant::now());
        let ShaderAuthoring::Layers { stack } = &asset.authoring else {
            self.textures.clear();
            return;
        };
        let count = u32::try_from(stack.layers.len()).unwrap_or(u32::MAX).max(1);
        let grid = (count.min(Self::COLUMNS), count.div_ceil(Self::COLUMNS));
        if !self.has_renderer_for(grid) {
            return;
        }
        let mut items = vec![(Area { offset: (0.0, 0.0), bounds: None }, Item::MegaShaderSprite(Self::backdrop()))];
        for index in 0..stack.layers.len() {
            let mut partial = asset.clone();
            if let ShaderAuthoring::Layers { stack } = &mut partial.authoring {
                stack.layers.truncate(index + 1);
            }
            let source = self.sources.source(&format!("{}~layer{index}", asset.name), partial.wgsl());
            let mut sprite = ShaderBridge::effect(&asset.choice(), Some(source)).sprite(EffectColor::srgb8(tint[0], tint[1], tint[2]), 1.0);
            let (column, row) = (index as u32 % Self::COLUMNS, index as u32 / Self::COLUMNS);
            sprite.instance.world_position = ((column as f32 + 0.5) / grid.0 as f32, (row as f32 + 0.5) / grid.1 as f32);
            sprite.instance.scale = (0.92 / grid.0 as f32, 0.92 / grid.1 as f32);
            items.push((Area { offset: (0.0, 0.0), bounds: None }, Item::MegaShaderSprite(sprite)));
        }
        let Some(renderer) = self.renderer.as_mut() else { return };
        let (width, _, pixels) = renderer.render(items);
        self.cut(ctx, &pixels, width, stack.layers.len());
    }

    /// Whether there is a renderer of `grid` cells, made again when the grid changes (not without a GPU).
    fn has_renderer_for(&mut self, grid: (u32, u32)) -> bool {
        if self.grid != grid || self.renderer.is_none() {
            match pollster::block_on(HeadlessRenderer::new(grid.0 * Self::CELL, grid.1 * Self::CELL)) {
                Ok(renderer) => {
                    self.renderer = Some(renderer);
                    self.grid = grid;
                    self.sources = SourceCache::default();
                }
                Err(error) => self.failed = Some(error),
            }
        }
        self.renderer.is_some()
    }

    /// A dark plate behind every cell, so additive layers show.
    fn backdrop() -> wgpu_canvas::MegaShaderSprite {
        let mut plate = wgpu_canvas::Effect::Animated { flags: wgpu_canvas::VfxFlags::NONE, alpha: 1.0 }.sprite(EffectColor::srgb8(18, 20, 30));
        plate.instance.world_position = (0.5, 0.5);
        plate.instance.scale = (1.0, 1.0);
        plate
    }

    fn cut(&mut self, ctx: &egui::Context, pixels: &[u8], width: u32, count: usize) {
        let cell = Self::CELL as usize;
        let stride = width as usize * 4;
        self.textures.truncate(count);
        for index in 0..count {
            let (column, row) = (index % Self::COLUMNS as usize, index / Self::COLUMNS as usize);
            let mut rgba = Vec::with_capacity(cell * cell * 4);
            for y in row * cell..(row + 1) * cell {
                let start = y * stride + column * cell * 4;
                rgba.extend_from_slice(pixels.get(start..start + cell * 4).unwrap_or(&[0; 256]));
            }
            let picture = egui::ColorImage::from_rgba_unmultiplied([cell, cell], &rgba);
            match self.textures.get_mut(index) {
                Some(texture) => texture.set(picture, egui::TextureOptions::LINEAR),
                None => self.textures.push(ctx.load_texture(format!("layer_thumb_{index}"), picture, egui::TextureOptions::LINEAR)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LayerThumbs;
    use quartz_project::shader_asset::ProjectShaderAsset;

    #[test]
    fn every_layer_gets_a_thumbnail_of_the_stack_up_to_it() {
        let ctx = egui::Context::default();
        let asset = ProjectShaderAsset::layered("thumbs");
        let mut thumbs = LayerThumbs::new();
        let count = thumbs.textures(&ctx, &asset, [255, 170, 80]).len();
        if thumbs.failed.is_some() {
            return;
        }
        assert_eq!(count, 2);
        let problems = thumbs.renderer.as_ref().map(wgpu_canvas::HeadlessRenderer::project_shader_problems).unwrap_or_default();
        assert!(problems.is_empty(), "{problems:?}");
    }
}
