//! SYNFUL lighting preview.
//!
//! Approximates the synful lighting shader
//! (`wgpu_canvas/src/renderer/material/lit_rectangle.wgsl`) on the CPU so the
//! editor can show roughly how a scene's lighting will look WITHOUT linking the
//! engine — the same independence principle as `background_preview`.
//!
//! Faithful to the shader's core math (lit_rectangle.wgsl ~359–387):
//!   - `atten = 1.0 - smoothstep(0.0, radius, dist)`  (point/spot falloff)
//!   - `accum += light.color * light.intensity * atten`
//!   - final = ambient_rgb * ambient_strength + accum, clamped
//!
//! Deliberate simplifications (this is an author-facing preview, not the GPU
//! pass): no normal/`ndl` term (flat 2D surface assumed lit head-on = 1.0), no
//! shadow occlusion, spot cones approximated as points, directional lights
//! added as a flat ambient-style term. Disabled lights are skipped.

use crate::core::project::{LightKindSpec, LightingSpec};
use image::{Rgba, RgbaImage};

/// smoothstep(edge0, edge1, x) — matches WGSL semantics.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge0 == edge1 {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn rgb_f(c: [u8; 4]) -> [f32; 3] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]
}

/// Render an approximate lit preview at `out_w × out_h`, mapping preview pixels
/// onto the `vw × vh` virtual scene so light positions land where they belong.
pub fn render_lighting_preview(
    lighting: &LightingSpec,
    vw: f32,
    vh: f32,
    out_w: u32,
    out_h: u32,
) -> RgbaImage {
    let out_w = out_w.max(1);
    let out_h = out_h.max(1);
    let vw = vw.max(1.0);
    let vh = vh.max(1.0);

    let ambient = rgb_f(lighting.ambient_color);
    let amb_s = lighting.ambient_strength;
    let base = [ambient[0] * amb_s, ambient[1] * amb_s, ambient[2] * amb_s];

    // Precompute directional contribution (flat, position-independent).
    let mut directional = [0.0f32; 3];
    for light in &lighting.lights {
        if !light.enabled {
            continue;
        }
        if let LightKindSpec::Directional { .. } = light.kind {
            let c = rgb_f(light.color);
            // The shader uses an ndl term for directional; here the surface is
            // flat so approximate with a fixed 0.3 factor (matches the engine's
            // moonlight/sun presets reading dim-but-global).
            let k = light.intensity * 0.3;
            directional[0] += c[0] * k;
            directional[1] += c[1] * k;
            directional[2] += c[2] * k;
        }
    }

    let mut img = RgbaImage::new(out_w, out_h);
    for py in 0..out_h {
        // Map preview pixel → world coords (light positions are world-space).
        let wy = (py as f32 + 0.5) / out_h as f32 * vh;
        for px in 0..out_w {
            let wx = (px as f32 + 0.5) / out_w as f32 * vw;

            let mut accum = [base[0] + directional[0], base[1] + directional[1], base[2] + directional[2]];

            for light in &lighting.lights {
                if !light.enabled {
                    continue;
                }
                match light.kind {
                    LightKindSpec::Directional { .. } => {} // handled above
                    LightKindSpec::Point | LightKindSpec::Spot { .. } => {
                        if light.radius <= 0.0 {
                            continue;
                        }
                        let dx = wx - light.x;
                        let dy = wy - light.y;
                        let dist = (dx * dx + dy * dy).sqrt();
                        let atten = 1.0 - smoothstep(0.0, light.radius, dist);
                        if atten <= 0.0 {
                            continue;
                        }
                        let c = rgb_f(light.color);
                        let k = light.intensity * atten;
                        accum[0] += c[0] * k;
                        accum[1] += c[1] * k;
                        accum[2] += c[2] * k;
                    }
                }
            }

            let r = (accum[0].clamp(0.0, 1.0) * 255.0) as u8;
            let g = (accum[1].clamp(0.0, 1.0) * 255.0) as u8;
            let b = (accum[2].clamp(0.0, 1.0) * 255.0) as u8;
            img.put_pixel(px, py, Rgba([r, g, b, 255]));
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::project::LightSpec;

    fn base_lighting() -> LightingSpec {
        LightingSpec {
            enabled: true,
            ambient_color: [0, 0, 0, 255],
            ambient_strength: 0.0,
            max_lights: 64,
            lights: Vec::new(),
        }
    }

    #[test]
    fn zero_ambient_no_lights_is_black() {
        let img = render_lighting_preview(&base_lighting(), 800.0, 600.0, 16, 12);
        assert_eq!(*img.get_pixel(0, 0), Rgba([0, 0, 0, 255]));
        assert_eq!(*img.get_pixel(8, 6), Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn ambient_lifts_every_pixel_uniformly() {
        let mut lt = base_lighting();
        lt.ambient_color = [255, 255, 255, 255];
        lt.ambient_strength = 0.5;
        let img = render_lighting_preview(&lt, 800.0, 600.0, 8, 8);
        let p = *img.get_pixel(0, 0);
        assert!((120..=135).contains(&p[0]), "ambient 0.5 white ≈ 127, got {}", p[0]);
        assert_eq!(*img.get_pixel(0, 0), *img.get_pixel(7, 7), "ambient is uniform");
    }

    #[test]
    fn point_light_is_brightest_at_its_center() {
        let mut lt = base_lighting();
        // Centered white point light.
        lt.lights.push(LightSpec {
            id: "l".to_owned(),
            x: 400.0,
            y: 300.0,
            color: [255, 255, 255, 255],
            radius: 400.0,
            intensity: 1.0,
            ..LightSpec::new("l")
        });
        let img = render_lighting_preview(&lt, 800.0, 600.0, 32, 24);
        let center = *img.get_pixel(16, 12);
        let corner = *img.get_pixel(0, 0);
        assert!(
            center[0] > corner[0],
            "light center ({}) must be brighter than a far corner ({})",
            center[0],
            corner[0]
        );
    }

    #[test]
    fn disabled_light_contributes_nothing() {
        let mut lt = base_lighting();
        lt.lights.push(LightSpec {
            id: "off".to_owned(),
            x: 400.0,
            y: 300.0,
            color: [255, 255, 255, 255],
            radius: 400.0,
            intensity: 1.0,
            enabled: false,
            ..LightSpec::new("off")
        });
        let img = render_lighting_preview(&lt, 800.0, 600.0, 8, 8);
        assert_eq!(*img.get_pixel(4, 4), Rgba([0, 0, 0, 255]), "disabled light must not light anything");
    }
}
