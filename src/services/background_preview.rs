//! Background composite preview.
//!
//! This is a faithful PORT of quartz's background compositor
//! (`quartz/src/plugin/background/mod.rs`, private `mod image_gen`) so the
//! editor can show what a `LayeredBackground` will actually look like WITHOUT
//! linking the engine. Forge deliberately stays independent of quartz's build:
//! you must be able to author while the engine is mid-refactor.
//!
//! Ported pieces (keep in sync if the engine's compositor changes):
//!   - `LcgRng`            (mod.rs ~631)  seed ^ 0x123456789ABCDEF0, LCG 6364136223846793005 / 1442695040888963407, state >> 33
//!   - `smooth_noise`      (mod.rs ~641)  value noise, smoothstep, hash_to_f32 via LcgRng
//!   - `alpha_over`        (mod.rs ~620)  premultiplied source-over
//!   - `solid`             (mod.rs  460)
//!   - `gradient_vertical` (mod.rs  396)
//!   - `gradient_horizontal`(mod.rs 409)
//!   - `gradient_four_corner`(mod.rs 422) bilinear
//!   - `starfield`         (mod.rs  466)  radial falloff discs
//!   - `nebula`            (mod.rs  497)  scale 0.004, alpha = min(n*density*200, 180)
//!   - `composite`         (mod.rs  520)
//!   - `composite_with_vertical_fade` (mod.rs 531)
//!   - `tint`              (mod.rs  580)  multiply
//!   - `build()` order     (mod.rs  128)  solid black base -> fold layers -> tint
//!
//! The preview renders at a reduced resolution for interactivity; procedural
//! layers therefore differ in exact pixel placement from a full-resolution
//! build (same character, same colors, same density feel).
use image::{Rgba, RgbaImage};

use crate::core::project::{BackgroundLayerSpec, NamedBackground};

// ── math helpers (ported) ────────────────────────────────────────────────────

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t.clamp(0.0, 1.0)) as u8
}

fn bilerp_u8(tl: u8, tr: u8, bl: u8, br: u8, tx: f32, ty: f32) -> u8 {
    let top = lerp_u8(tl, tr, tx) as f32;
    let bottom = lerp_u8(bl, br, tx) as f32;
    (top + (bottom - top) * ty.clamp(0.0, 1.0)) as u8
}

/// Source-over compositing, matching the engine's `alpha_over`.
fn alpha_over(base: Rgba<u8>, over: Rgba<u8>) -> Rgba<u8> {
    let ao = over[3] as f32 / 255.0;
    let ab = base[3] as f32 / 255.0;
    let alpha_out = ao + ab * (1.0 - ao);
    if alpha_out < 1e-6 {
        return Rgba([0, 0, 0, 0]);
    }
    Rgba([
        ((over[0] as f32 * ao + base[0] as f32 * ab * (1.0 - ao)) / alpha_out) as u8,
        ((over[1] as f32 * ao + base[1] as f32 * ab * (1.0 - ao)) / alpha_out) as u8,
        ((over[2] as f32 * ao + base[2] as f32 * ab * (1.0 - ao)) / alpha_out) as u8,
        (alpha_out * 255.0) as u8,
    ])
}

struct LcgRng {
    state: u64,
}

impl LcgRng {
    fn new(seed: u64) -> Self {
        Self { state: seed ^ 0x1234_5678_9ABC_DEF0 }
    }
    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 33) as u32
    }
    fn next_f32(&mut self) -> f32 {
        self.next_u32() as f32 / u32::MAX as f32
    }
}

fn hash_to_f32(x: i32, y: i32, seed: u64) -> f32 {
    let mut rng = LcgRng::new(
        seed ^ (x as u64).wrapping_mul(374761393) ^ (y as u64).wrapping_mul(668265263),
    );
    rng.next_f32()
}

fn smooth_noise(x: f32, y: f32, seed: u64) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let (xf, yf) = (x - xi as f32, y - yi as f32);
    let v00 = hash_to_f32(xi, yi, seed);
    let v10 = hash_to_f32(xi + 1, yi, seed);
    let v01 = hash_to_f32(xi, yi + 1, seed);
    let v11 = hash_to_f32(xi + 1, yi + 1, seed);
    let (ux, uy) = (xf * xf * (3.0 - 2.0 * xf), yf * yf * (3.0 - 2.0 * yf));
    let top = v00 + (v10 - v00) * ux;
    let bottom = v01 + (v11 - v01) * ux;
    top + (bottom - top) * uy
}

// ── layer renderers (ported) ─────────────────────────────────────────────────

fn solid(width: u32, height: u32, color: (u8, u8, u8, u8)) -> RgbaImage {
    let mut img = RgbaImage::new(width, height);
    for pixel in img.pixels_mut() {
        *pixel = Rgba([color.0, color.1, color.2, color.3]);
    }
    img
}

fn gradient_vertical(width: u32, height: u32, top: [u8; 3], bottom: [u8; 3]) -> RgbaImage {
    let mut img = RgbaImage::new(width, height);
    for y in 0..height {
        let t = y as f32 / (height - 1).max(1) as f32;
        let c = [
            lerp_u8(top[0], bottom[0], t),
            lerp_u8(top[1], bottom[1], t),
            lerp_u8(top[2], bottom[2], t),
        ];
        for x in 0..width {
            img.put_pixel(x, y, Rgba([c[0], c[1], c[2], 255]));
        }
    }
    img
}

fn gradient_horizontal(width: u32, height: u32, left: [u8; 3], right: [u8; 3]) -> RgbaImage {
    let mut img = RgbaImage::new(width, height);
    for x in 0..width {
        let t = x as f32 / (width - 1).max(1) as f32;
        let c = [
            lerp_u8(left[0], right[0], t),
            lerp_u8(left[1], right[1], t),
            lerp_u8(left[2], right[2], t),
        ];
        for y in 0..height {
            img.put_pixel(x, y, Rgba([c[0], c[1], c[2], 255]));
        }
    }
    img
}

fn gradient_four_corner(
    width: u32,
    height: u32,
    tl: [u8; 3],
    tr: [u8; 3],
    bl: [u8; 3],
    br: [u8; 3],
) -> RgbaImage {
    let mut img = RgbaImage::new(width, height);
    for y in 0..height {
        let ty = y as f32 / (height - 1).max(1) as f32;
        for x in 0..width {
            let tx = x as f32 / (width - 1).max(1) as f32;
            img.put_pixel(
                x,
                y,
                Rgba([
                    bilerp_u8(tl[0], tr[0], bl[0], br[0], tx, ty),
                    bilerp_u8(tl[1], tr[1], bl[1], br[1], tx, ty),
                    bilerp_u8(tl[2], tr[2], bl[2], br[2], tx, ty),
                    255,
                ]),
            );
        }
    }
    img
}

#[allow(clippy::too_many_arguments)]
fn starfield(
    width: u32,
    height: u32,
    density: u32,
    seed: u64,
    size_range: (u32, u32),
    brightness_range: (u8, u8),
) -> RgbaImage {
    let mut img = RgbaImage::new(width, height);
    let mut rng = LcgRng::new(seed);
    for _ in 0..density {
        let x = rng.next_u32() % width.max(1);
        let y = rng.next_u32() % height.max(1);
        let brange = brightness_range.1.saturating_sub(brightness_range.0);
        let brightness = brightness_range.0 + (rng.next_u32() % (brange as u32 + 1)) as u8;
        let srange = (size_range.1.saturating_sub(size_range.0) + 1).max(1);
        let radius = size_range.0 + rng.next_u32() % srange;
        for dy in 0..=(radius * 2) {
            for dx in 0..=(radius * 2) {
                let px = x as i32 + dx as i32 - radius as i32;
                let py = y as i32 + dy as i32 - radius as i32;
                if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                    continue;
                }
                let dist = ((dx as f32 - radius as f32).powi(2)
                    + (dy as f32 - radius as f32).powi(2))
                .sqrt();
                if dist <= radius as f32 {
                    let alpha =
                        (brightness as f32 * (1.0 - dist / (radius as f32).max(0.01))) as u8;
                    img.put_pixel(px as u32, py as u32, Rgba([255, 255, 255, alpha]));
                }
            }
        }
    }
    img
}

fn nebula(width: u32, height: u32, color: [u8; 3], density: f32, seed: u64) -> RgbaImage {
    let mut img = RgbaImage::new(width, height);
    let scale = 0.004_f32;
    for y in 0..height {
        for x in 0..width {
            let n = smooth_noise(x as f32 * scale, y as f32 * scale, seed);
            let alpha = ((n * density * 200.0) as u8).min(180);
            img.put_pixel(x, y, Rgba([color[0], color[1], color[2], alpha]));
        }
    }
    img
}

fn composite(base: &RgbaImage, overlay: &RgbaImage) -> RgbaImage {
    let (w, h) = base.dimensions();
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            out.put_pixel(x, y, alpha_over(*base.get_pixel(x, y), *overlay.get_pixel(x, y)));
        }
    }
    out
}

fn composite_with_vertical_fade(
    base: &RgbaImage,
    overlay: &RgbaImage,
    fade_height: u32,
) -> RgbaImage {
    let (w, h) = base.dimensions();
    let mut out = base.clone();
    for y in 0..h.min(overlay.height()) {
        let alpha_scale = if y < fade_height {
            1.0 - (y as f32 / fade_height.max(1) as f32)
        } else {
            0.0
        };
        for x in 0..w.min(overlay.width()) {
            let b = *base.get_pixel(x, y);
            let mut o = *overlay.get_pixel(x, y);
            o[3] = (o[3] as f32 * alpha_scale) as u8;
            out.put_pixel(x, y, alpha_over(b, o));
        }
    }
    out
}

fn tint_image(src: &RgbaImage, tint: [u8; 3]) -> RgbaImage {
    let mut out = src.clone();
    for pixel in out.pixels_mut() {
        pixel[0] = ((pixel[0] as u16 * tint[0] as u16) / 255) as u8;
        pixel[1] = ((pixel[1] as u16 * tint[1] as u16) / 255) as u8;
        pixel[2] = ((pixel[2] as u16 * tint[2] as u16) / 255) as u8;
    }
    out
}

/// Decode an image asset and resize it to the background dimensions, matching
/// `ImageGen::load_and_resize` (mod.rs ~590). Returns a transparent layer when
/// the asset can't be found/decoded so the preview degrades instead of panicking
/// (the engine's version `expect`s, but a preview must never take down the editor).
fn image_layer(
    asset_path: &str,
    filter: crate::core::project::BackgroundResizeFilter,
    width: u32,
    height: u32,
    project_root: Option<&std::path::Path>,
) -> RgbaImage {
    use crate::core::project::BackgroundResizeFilter as F;
    use image::imageops::FilterType;

    let filter_type = match filter {
        F::Nearest => FilterType::Nearest,
        F::Bilinear => FilterType::Triangle,
        F::Bicubic => FilterType::CatmullRom,
        F::Lanczos3 => FilterType::Lanczos3,
    };

    let resolved = project_root.map(|root| root.join(asset_path.trim_start_matches(['/', '\\'])));
    let decoded = resolved
        .as_ref()
        .and_then(|p| image::open(p).ok())
        .map(|img| img.to_rgba8());

    match decoded {
        Some(src) => image::imageops::resize(&src, width.max(1), height.max(1), filter_type),
        // Missing/undecodable asset → fully transparent (layer contributes nothing).
        None => solid(width, height, (0, 0, 0, 0)),
    }
}

fn render_layer(
    layer: &BackgroundLayerSpec,
    width: u32,
    height: u32,
    project_root: Option<&std::path::Path>,
) -> RgbaImage {
    match layer {
        BackgroundLayerSpec::Solid { color } => solid(width, height, (color[0], color[1], color[2], 255)),
        BackgroundLayerSpec::GradientVertical { top, bottom } => {
            gradient_vertical(width, height, *top, *bottom)
        }
        BackgroundLayerSpec::GradientHorizontal { left, right } => {
            gradient_horizontal(width, height, *left, *right)
        }
        BackgroundLayerSpec::GradientFourCorner {
            top_left,
            top_right,
            bottom_left,
            bottom_right,
        } => gradient_four_corner(width, height, *top_left, *top_right, *bottom_left, *bottom_right),
        BackgroundLayerSpec::Starfield {
            density,
            seed,
            size_min,
            size_max,
            brightness_min,
            brightness_max,
            vertical_fade,
        } => {
            let stars = starfield(
                width,
                height,
                *density,
                *seed,
                (*size_min, *size_max),
                (*brightness_min, *brightness_max),
            );
            match vertical_fade {
                // Engine composites the faded starfield over a TRANSPARENT base
                // here; the caller then composites the result over the stack.
                Some(fade) => {
                    let base = solid(width, height, (0, 0, 0, 0));
                    composite_with_vertical_fade(&base, &stars, *fade)
                }
                None => stars,
            }
        }
        BackgroundLayerSpec::Nebula { color, density, seed } => {
            nebula(width, height, *color, *density, *seed)
        }
        BackgroundLayerSpec::Image { asset_path, filter } => {
            image_layer(asset_path, *filter, width, height, project_root)
        }
    }
}

/// Composite a named background into an RGBA image, mirroring
/// `LayeredBackground::build()`: solid black base → fold layers → tint.
pub fn render_background(
    bg: &NamedBackground,
    width: u32,
    height: u32,
    project_root: Option<&std::path::Path>,
) -> RgbaImage {
    let width = width.max(1);
    let height = height.max(1);
    let base = solid(width, height, (0, 0, 0, 255));
    let composited = bg.layers.iter().fold(base, |acc, layer| {
        composite(&acc, &render_layer(layer, width, height, project_root))
    });

    if bg.tint != [255, 255, 255] {
        tint_image(&composited, bg.tint)
    } else {
        composited
    }
}

/// Preview dimensions for an aspect ratio, capped for interactivity.
pub fn preview_size(virtual_w: f32, virtual_h: f32, max_w: u32) -> (u32, u32) {
    let aspect = if virtual_h > 0.0 { virtual_w / virtual_h } else { 16.0 / 9.0 };
    let w = max_w.max(16);
    let h = ((w as f32 / aspect.max(0.05)).round() as u32).clamp(9, 4320);
    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bg(layers: Vec<BackgroundLayerSpec>, tint: [u8; 3]) -> NamedBackground {
        NamedBackground { key: "t".to_owned(), tint, layers }
    }

    #[test]
    fn solid_layer_fills_exact_color() {
        let img = render_background(&bg(vec![BackgroundLayerSpec::Solid { color: [10, 20, 30] }], [255, 255, 255]), 8, 4, None);
        assert_eq!(img.dimensions(), (8, 4));
        assert_eq!(*img.get_pixel(0, 0), Rgba([10, 20, 30, 255]));
        assert_eq!(*img.get_pixel(7, 3), Rgba([10, 20, 30, 255]));
    }

    #[test]
    fn vertical_gradient_interpolates_top_to_bottom() {
        let img = render_background(
            &bg(vec![BackgroundLayerSpec::GradientVertical { top: [0, 0, 0], bottom: [255, 255, 255] }], [255, 255, 255]),
            4,
            5,
            None,
        );
        assert_eq!(img.get_pixel(0, 0)[0], 0);
        assert_eq!(img.get_pixel(0, 4)[0], 255);
        // Middle row is between the endpoints.
        let mid = img.get_pixel(0, 2)[0];
        assert!(mid > 0 && mid < 255, "mid={mid}");
    }

    #[test]
    fn tint_multiplies_result() {
        let untinted = render_background(&bg(vec![BackgroundLayerSpec::Solid { color: [200, 200, 200] }], [255, 255, 255]), 2, 2, None);
        let tinted = render_background(&bg(vec![BackgroundLayerSpec::Solid { color: [200, 200, 200] }], [128, 255, 255]), 2, 2, None);
        assert_eq!(untinted.get_pixel(0, 0)[0], 200);
        assert_eq!(tinted.get_pixel(0, 0)[0], (200u16 * 128 / 255) as u8);
        assert_eq!(tinted.get_pixel(0, 0)[1], 200, "green channel untouched");
    }

    #[test]
    fn layers_composite_bottom_to_top() {
        // Opaque red under opaque blue → blue wins (last added is topmost).
        let img = render_background(
            &bg(
                vec![
                    BackgroundLayerSpec::Solid { color: [255, 0, 0] },
                    BackgroundLayerSpec::Solid { color: [0, 0, 255] },
                ],
                [255, 255, 255],
            ),
            2,
            2,
            None,
        );
        assert_eq!(*img.get_pixel(0, 0), Rgba([0, 0, 255, 255]));
    }

    #[test]
    fn starfield_is_deterministic_for_a_seed() {
        let spec = BackgroundLayerSpec::Starfield {
            density: 40,
            seed: 0xCAFE_BABE,
            size_min: 0,
            size_max: 1,
            brightness_min: 100,
            brightness_max: 255,
            vertical_fade: None,
        };
        let a = render_background(&bg(vec![spec.clone()], [255, 255, 255]), 32, 32, None);
        let b = render_background(&bg(vec![spec], [255, 255, 255]), 32, 32, None);
        assert_eq!(a.as_raw(), b.as_raw(), "same seed must produce identical output");
    }

    #[test]
    fn preview_size_preserves_aspect() {
        let (w, h) = preview_size(3840.0, 2160.0, 320);
        assert_eq!(w, 320);
        assert_eq!(h, 180);
    }
}
