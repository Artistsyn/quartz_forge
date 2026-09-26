//! SYNFUL lighting preview.
//!
//! Evaluates the synful lighting shader
//! (`wgpu_canvas/src/renderer/material/lit_rectangle.wgsl`) on the CPU so the
//! editor can show how a scene's lighting will look WITHOUT linking the engine
//! — the same independence principle as `background_preview`.
//!
//! It computes what the shader computes for a white sprite with a FLAT normal,
//! which is what every sprite without a normal map has:
//!   - point: `color * intensity * ndl * (1 - smoothstep(0, radius, dist))`
//!   - spot: the same, times the cone term, zero outside the cone
//!   - directional: `color * intensity * ndl`
//!   - `ndl = dot((0,0,1), normalize(vec3(to_light_2d, 0.5)))`, which for a
//!     flat normal is `0.5 / sqrt(|to_light_2d|^2 + 0.25)` — 0.447 for any
//!     point or spot light, and for a directional light with a unit direction
//!   - result clamped, then sRGB-encoded as the surface does on write
//!
//! This used to leave `ndl` out ("lit head-on = 1.0"), fake directional light
//! with a 0.3 factor, treat spots as points, and write linear values straight
//! to 8-bit. The first alone made every point light look 2.2x brighter in the
//! editor than in the game, so any intensity tuned here was wrong on device.
//! `the_shader_still_computes_what_this_preview_assumes` pins the formulas.
//!
//! Still not modelled: shadow occlusion, normal maps, the sprite's own colour.
//! Disabled lights are skipped.

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
/// `ndl` for a flat normal lit from 2D direction `d` (not necessarily unit).
fn flat_ndl(d: (f32, f32)) -> f32 {
    0.5 / (d.0 * d.0 + d.1 * d.1 + 0.25).sqrt()
}

/// Linear -> sRGB, as an `*Srgb` render target encodes on write.
fn srgb_encode(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// One light's contribution (a scalar, before colour) at world point `(wx, wy)`.
pub(crate) fn light_factor(light: &crate::core::project::LightSpec, wx: f32, wy: f32) -> f32 {
    match light.kind {
        LightKindSpec::Directional { dx, dy } => {
            // The shader: normalize(vec3(-dir.x, -dir.y, 0.5)).
            light.intensity * flat_ndl((-dx, -dy))
        }
        LightKindSpec::Point | LightKindSpec::Spot { .. } => {
            if light.radius <= 0.0 {
                return 0.0;
            }
            let (tx, ty) = (light.x - wx, light.y - wy);
            let dist = (tx * tx + ty * ty).sqrt();
            if dist >= light.radius {
                return 0.0;
            }
            let atten = 1.0 - smoothstep(0.0, light.radius, dist);
            // normalize(vec3(normalize(to_light), 0.5)) -> ndl = 0.447.
            let ndl = flat_ndl((1.0, 0.0));
            let mut k = light.intensity * atten * ndl;
            if let LightKindSpec::Spot { direction, cone_angle } = light.kind {
                // The engine turns the angle into a unit vector and halves
                // the cone (quartz lighting/types.rs `to_point_light`).
                let half_cos = (cone_angle * 0.5).cos();
                let (fx, fy) = if dist > 0.0 { (-tx / dist, -ty / dist) } else { (0.0, 0.0) };
                let cos_angle = fx * direction.cos() + fy * direction.sin();
                if cos_angle < half_cos {
                    return 0.0;
                }
                k *= smoothstep(half_cos - 0.05, half_cos + 0.10, cos_angle);
            }
            k
        }
    }
}

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

    let mut img = RgbaImage::new(out_w, out_h);
    for py in 0..out_h {
        // Map preview pixel -> world coords (light positions are world-space).
        let wy = (py as f32 + 0.5) / out_h as f32 * vh;
        for px in 0..out_w {
            let wx = (px as f32 + 0.5) / out_w as f32 * vw;
            let mut accum = base;
            for light in lighting.lights.iter().filter(|l| l.enabled) {
                let k = light_factor(light, wx, wy);
                if k > 0.0 {
                    let c = rgb_f(light.color);
                    accum[0] += c[0] * k;
                    accum[1] += c[1] * k;
                    accum[2] += c[2] * k;
                }
            }
            let enc = |v: f32| (srgb_encode(v) * 255.0 + 0.5) as u8;
            img.put_pixel(px, py, Rgba([enc(accum[0]), enc(accum[1]), enc(accum[2]), 255]));
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
        // Linear 0.5 encodes to sRGB 188, as the surface does on write.
        assert!((185..=191).contains(&p[0]), "ambient 0.5 white ≈ 188 (sRGB), got {}", p[0]);
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

    fn light(kind: LightKindSpec) -> LightSpec {
        LightSpec {
            id: "l".to_owned(),
            x: 400.0,
            y: 300.0,
            color: [255, 255, 255, 255],
            radius: 400.0,
            intensity: 1.0,
            kind,
            ..LightSpec::new("l")
        }
    }

    /// The factor the old preview left out: a point light at its own centre
    /// gives 0.447, not 1.0, because the shader tilts the light direction
    /// out of the page by 0.5.
    #[test]
    fn a_point_light_at_its_centre_gives_the_shaders_ndl() {
        let k = light_factor(&light(LightKindSpec::Point), 400.0, 300.0);
        assert!((k - 0.4472).abs() < 1e-3, "got {k}");
    }

    #[test]
    fn a_directional_light_gives_intensity_times_ndl() {
        let k = light_factor(&light(LightKindSpec::Directional { dx: 1.0, dy: 0.0 }), 0.0, 0.0);
        assert!((k - 0.4472).abs() < 1e-3, "got {k}");
    }

    #[test]
    fn a_spot_light_is_dark_outside_its_cone_and_lit_inside() {
        // Pointing +x with a 60 degree cone.
        let spot = light(LightKindSpec::Spot { direction: 0.0, cone_angle: 60f32.to_radians() });
        assert!(light_factor(&spot, 500.0, 300.0) > 0.1, "on-axis point is dark");
        assert_eq!(light_factor(&spot, 300.0, 300.0), 0.0, "behind the spot is lit");
        assert_eq!(light_factor(&spot, 400.0, 400.0), 0.0, "90 degrees off-axis is lit");
    }

    /// The preview is a second implementation of the shader. Pin the shader
    /// text it transcribes, so an edit there fails here instead of drifting.
    #[test]
    fn the_shader_still_computes_what_this_preview_assumes() {
        let wgsl = include_str!("../../../wgpu_canvas/src/renderer/material/lit_rectangle.wgsl");
        for needle in [
            "let atten = 1.0 - smoothstep(0.0, l.radius, dist);",
            "let ldir  = normalize(vec3<f32>(normalize(to_light), 0.5));",
            "let ldir = normalize(vec3<f32>(-l.direction.x, -l.direction.y, 0.5));",
            "l.cone_half_cos - 0.05,",
            "l.cone_half_cos + 0.10,",
            "accum += l.color * ndl * l.intensity * atten * shadow;",
        ] {
            assert!(wgsl.contains(needle), "lit_rectangle.wgsl no longer contains `{needle}` — update the preview to match");
        }
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
