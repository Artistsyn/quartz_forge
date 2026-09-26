//! Authoring for quartz's typed shader effects (`wgpu_canvas::Effect`).
//!
//! An object may carry ONE attached effect — the engine has one slot per
//! object — emitted as
//!
//! ```ignore
//! obj.set_effect(Effect::ThrustPlume { thrust: 0.7 }, EffectColor::srgb8(255, 140, 41), (w, h));
//! ```
//!
//! The forge does not link the engine, so this module is a MIRROR of the
//! engine enum. `tests::authorable_effects_match_the_engine` reads
//! `wgpu_canvas/src/effect.rs` and fails when a variant, a field or a
//! `VfxFlags` name exists on one side only — a new engine effect then cannot
//! ship unauthorable without someone deciding so.
//!
//! The spec is FLAT (every parameter any effect takes, one field each) rather
//! than an enum of structs: switching the kind in the editor keeps the values
//! the author already dialled in, and codegen emits only the fields the chosen
//! kind reads.
//!
//! Colour is authored as 8-bit sRGB — what a colour picker shows — and emitted
//! as `EffectColor::srgb8`, which decodes to the shader's linear space. So an
//! orange here is the same orange in the game.
//!
//! Not authorable: `Effect::Image` / `Effect::AnimatedImage` (they need an
//! image handle the object model has no field for; use the object's own
//! visual instead).

use serde::{Deserialize, Serialize};
use syn::{Expr, Lit, Member};

/// Which effect. Mirrors the authorable `wgpu_canvas::Effect` variants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EffectKind {
    /// Combinable animated looks (electricity, fire, …) over a white quad.
    #[default]
    Animated,
    EnergyDome,
    SegmentShield,
    SonicRing,
    ResonanceWave,
    BeatField,
    EqArcs,
    StateMarker,
    StrikeLane,
    BeatNode,
    Impact,
    EnergyTether,
    ThrustPlume,
}

impl EffectKind {
    pub const ALL: [EffectKind; 13] = [
        EffectKind::Animated,
        EffectKind::EnergyDome,
        EffectKind::SegmentShield,
        EffectKind::SonicRing,
        EffectKind::ResonanceWave,
        EffectKind::BeatField,
        EffectKind::EqArcs,
        EffectKind::StateMarker,
        EffectKind::StrikeLane,
        EffectKind::BeatNode,
        EffectKind::Impact,
        EffectKind::EnergyTether,
        EffectKind::ThrustPlume,
    ];

    /// The engine variant name.
    pub fn variant(self) -> &'static str {
        match self {
            EffectKind::Animated => "Animated",
            EffectKind::EnergyDome => "EnergyDome",
            EffectKind::SegmentShield => "SegmentShield",
            EffectKind::SonicRing => "SonicRing",
            EffectKind::ResonanceWave => "ResonanceWave",
            EffectKind::BeatField => "BeatField",
            EffectKind::EqArcs => "EqArcs",
            EffectKind::StateMarker => "StateMarker",
            EffectKind::StrikeLane => "StrikeLane",
            EffectKind::BeatNode => "BeatNode",
            EffectKind::Impact => "Impact",
            EffectKind::EnergyTether => "EnergyTether",
            EffectKind::ThrustPlume => "ThrustPlume",
        }
    }

    pub fn from_variant(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.variant() == name)
    }

    /// Editor label.
    pub fn label(self) -> &'static str {
        match self {
            EffectKind::Animated => "animated looks",
            EffectKind::EnergyDome => "energy dome",
            EffectKind::SegmentShield => "segment shield",
            EffectKind::SonicRing => "sonic ring",
            EffectKind::ResonanceWave => "resonance wave",
            EffectKind::BeatField => "beat field",
            EffectKind::EqArcs => "equaliser arcs",
            EffectKind::StateMarker => "state marker",
            EffectKind::StrikeLane => "strike lane",
            EffectKind::BeatNode => "beat node",
            EffectKind::Impact => "impact",
            EffectKind::EnergyTether => "energy tether",
            EffectKind::ThrustPlume => "thrust plume",
        }
    }

    /// The 0..1 scalar every effect has, by its engine field name.
    pub fn amount_field(self) -> &'static str {
        match self {
            EffectKind::Animated => "alpha",
            EffectKind::EnergyDome => "strength",
            EffectKind::SegmentShield => "energy",
            EffectKind::ResonanceWave => "amplitude",
            EffectKind::BeatField => "pulse",
            EffectKind::BeatNode => "near",
            EffectKind::Impact => "age",
            EffectKind::ThrustPlume => "thrust",
            EffectKind::SonicRing
            | EffectKind::EqArcs
            | EffectKind::StateMarker
            | EffectKind::StrikeLane
            | EffectKind::EnergyTether => "intensity",
        }
    }

    /// Every engine field this kind takes, in declaration order.
    pub fn fields(self) -> &'static [&'static str] {
        match self {
            EffectKind::Animated => &["flags", "alpha"],
            EffectKind::BeatField => &["pulse", "edge"],
            EffectKind::EqArcs => &["intensity", "levels"],
            EffectKind::StateMarker => &["mode", "intensity"],
            EffectKind::StrikeLane => &["intensity", "progress"],
            EffectKind::Impact => &["age", "side"],
            EffectKind::EnergyTether => &["intensity", "snap"],
            k => std::slice::from_ref(match k {
                EffectKind::EnergyDome => &"strength",
                EffectKind::SegmentShield => &"energy",
                EffectKind::SonicRing => &"intensity",
                EffectKind::ResonanceWave => &"amplitude",
                EffectKind::BeatNode => &"near",
                EffectKind::ThrustPlume => &"thrust",
                _ => unreachable!(),
            }),
        }
    }

    pub fn takes(self, field: &str) -> bool {
        self.fields().contains(&field)
    }

    /// Strips run along the object's local +x, source at the left edge — the
    /// object's rotation aims them. Shown as a hint in the editor.
    pub fn is_strip(self) -> bool {
        matches!(self, EffectKind::StrikeLane | EffectKind::EnergyTether | EffectKind::ThrustPlume)
    }
}

/// `ScreenEdge` mirror.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenEdgeSpec {
    Top,
    Bottom,
    Left,
    Right,
    #[default]
    All,
}

impl ScreenEdgeSpec {
    pub const ALL: [ScreenEdgeSpec; 5] =
        [ScreenEdgeSpec::Top, ScreenEdgeSpec::Bottom, ScreenEdgeSpec::Left, ScreenEdgeSpec::Right, ScreenEdgeSpec::All];
    pub fn name(self) -> &'static str {
        match self {
            ScreenEdgeSpec::Top => "Top",
            ScreenEdgeSpec::Bottom => "Bottom",
            ScreenEdgeSpec::Left => "Left",
            ScreenEdgeSpec::Right => "Right",
            ScreenEdgeSpec::All => "All",
        }
    }
}

/// `MarkerMode` mirror.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkerModeSpec {
    #[default]
    Vulnerable,
    Shielded,
    WindingUp,
}

impl MarkerModeSpec {
    pub const ALL: [MarkerModeSpec; 3] = [MarkerModeSpec::Vulnerable, MarkerModeSpec::Shielded, MarkerModeSpec::WindingUp];
    pub fn name(self) -> &'static str {
        match self {
            MarkerModeSpec::Vulnerable => "Vulnerable",
            MarkerModeSpec::Shielded => "Shielded",
            MarkerModeSpec::WindingUp => "WindingUp",
        }
    }
}

/// `ImpactSide` mirror.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImpactSideSpec {
    #[default]
    Dealt,
    Taken,
}

impl ImpactSideSpec {
    pub const ALL: [ImpactSideSpec; 2] = [ImpactSideSpec::Dealt, ImpactSideSpec::Taken];
    pub fn name(self) -> &'static str {
        match self {
            ImpactSideSpec::Dealt => "Dealt",
            ImpactSideSpec::Taken => "Taken",
        }
    }
}

/// `VfxFlags` names, in bit order. Pinned to the engine by test.
pub const VFX_FLAG_NAMES: [&str; 20] = [
    "PULSE_GLOW", "FIRE", "ELECTRICITY", "RIPPLE_DISTORT", "GLITCH", "DISSOLVE",
    "RAINBOW_SHIFT", "SHOCKWAVE", "HOLO_SCAN", "POISON_BUBBLES", "WIND_SWIRL",
    "EXPLOSIVE_SPARKS", "BREATHING_SCALE", "OUTLINE_PULSE", "TELEPORT_GATE",
    "COMET_TAIL", "RAIN", "AIR_SHIELD", "AIR_SHIELD_ELEC", "WINDOW_DROPLET",
];

fn default_amount() -> f32 { 1.0 }
fn default_scale() -> [f32; 2] { [1.0, 1.0] }
fn default_rgb() -> [u8; 3] { [140, 215, 255] }
fn default_levels() -> [f32; 4] { [1.0; 4] }
fn default_flags() -> Vec<String> { vec!["ELECTRICITY".to_owned()] }

/// One object's attached effect. See the module docs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectEffectSpec {
    pub kind: EffectKind,
    /// 8-bit sRGB, as a picker shows it.
    #[serde(default = "default_rgb")]
    pub rgb: [u8; 3],
    /// Effect size as a multiple of the object's (w, h).
    #[serde(default = "default_scale")]
    pub scale: [f32; 2],
    /// The kind's 0..1 scalar (`EffectKind::amount_field`).
    #[serde(default = "default_amount")]
    pub amount: f32,
    /// StrikeLane: how far the lane has filled.
    #[serde(default)]
    pub progress: f32,
    /// EnergyTether: `Some(progress)` while snapping.
    #[serde(default)]
    pub snap: Option<f32>,
    #[serde(default)]
    pub edge: ScreenEdgeSpec,
    #[serde(default)]
    pub mode: MarkerModeSpec,
    #[serde(default)]
    pub side: ImpactSideSpec,
    /// EqArcs: one level per quadrant.
    #[serde(default = "default_levels")]
    pub levels: [f32; 4],
    /// Animated: `VFX_FLAG_NAMES` entries.
    #[serde(default = "default_flags")]
    pub flags: Vec<String>,
}

impl Default for ObjectEffectSpec {
    fn default() -> Self {
        Self {
            kind: EffectKind::default(),
            rgb: default_rgb(),
            scale: default_scale(),
            amount: default_amount(),
            progress: 0.0,
            snap: None,
            edge: ScreenEdgeSpec::default(),
            mode: MarkerModeSpec::default(),
            side: ImpactSideSpec::default(),
            levels: default_levels(),
            flags: default_flags(),
        }
    }
}

impl ObjectEffectSpec {
    /// The `Effect::..` expression.
    pub fn effect_expr(&self, f32_lit: impl Fn(f32) -> String) -> String {
        let k = self.kind;
        let mut fields = Vec::new();
        for field in k.fields() {
            let value = match *field {
                "flags" => {
                    let names: Vec<String> = VFX_FLAG_NAMES
                        .iter()
                        .filter(|n| self.flags.iter().any(|f| f == *n))
                        .map(|n| format!("VfxFlags::{n}"))
                        .collect();
                    if names.is_empty() { "VfxFlags::NONE".to_owned() } else { names.join(" | ") }
                }
                "progress" => f32_lit(self.progress.clamp(0.0, 1.0)),
                "snap" => match self.snap {
                    None => "None".to_owned(),
                    Some(t) => format!("Some({})", f32_lit(t.clamp(0.0, 1.0))),
                },
                "edge" => format!("ScreenEdge::{}", self.edge.name()),
                "mode" => format!("MarkerMode::{}", self.mode.name()),
                "side" => format!("ImpactSide::{}", self.side.name()),
                "levels" => format!(
                    "[{}]",
                    self.levels.iter().map(|l| f32_lit(l.clamp(0.0, 1.0))).collect::<Vec<_>>().join(", ")
                ),
                _ => f32_lit(self.amount.clamp(0.0, 1.0)),
            };
            fields.push(format!("{field}: {value}"));
        }
        format!("Effect::{} {{ {} }}", k.variant(), fields.join(", "))
    }

    /// The whole statement for object local `id` of size `(w, h)`.
    pub fn set_effect_stmt(&self, id: &str, w: f32, h: f32, f32_lit: impl Fn(f32) -> String) -> String {
        let size = (w * self.scale[0], h * self.scale[1]);
        format!(
            "    {id}.set_effect({}, EffectColor::srgb8({}, {}, {}), ({}, {}));\n",
            self.effect_expr(&f32_lit),
            self.rgb[0],
            self.rgb[1],
            self.rgb[2],
            f32_lit(size.0),
            f32_lit(size.1),
        )
    }

    /// Parse the three arguments of `obj.set_effect(effect, colour, size)`
    /// back into a spec. `(w, h)` is the object's size, for the scale.
    /// `None` when the effect is not one the forge can author.
    pub fn from_call_args(args: &[&Expr], w: f32, h: f32) -> Option<Self> {
        let [effect, colour, size] = args else { return None };
        let Expr::Struct(st) = strip(effect) else { return None };
        let kind = EffectKind::from_variant(&st.path.segments.last()?.ident.to_string())?;
        let mut spec = ObjectEffectSpec { kind, ..Default::default() };
        for fv in &st.fields {
            let Member::Named(name) = &fv.member else { continue };
            let name = name.to_string();
            let e = strip(&fv.expr);
            match name.as_str() {
                "flags" => {
                    let mut names = Vec::new();
                    collect_path_tails(e, &mut names);
                    spec.flags = names.into_iter().filter(|n| VFX_FLAG_NAMES.contains(&n.as_str())).collect();
                }
                "progress" => spec.progress = num(e)?,
                "snap" => {
                    spec.snap = match e {
                        Expr::Call(c) if path_tail(&c.func).as_deref() == Some("Some") => {
                            Some(num(c.args.first()?)?)
                        }
                        _ => None,
                    }
                }
                "edge" => {
                    let n = path_tail(e)?;
                    spec.edge = ScreenEdgeSpec::ALL.into_iter().find(|x| x.name() == n)?;
                }
                "mode" => {
                    let n = path_tail(e)?;
                    spec.mode = MarkerModeSpec::ALL.into_iter().find(|x| x.name() == n)?;
                }
                "side" => {
                    let n = path_tail(e)?;
                    spec.side = ImpactSideSpec::ALL.into_iter().find(|x| x.name() == n)?;
                }
                "levels" => {
                    let Expr::Array(a) = e else { return None };
                    for (i, el) in a.elems.iter().take(4).enumerate() {
                        spec.levels[i] = num(el)?;
                    }
                }
                f if f == kind.amount_field() => spec.amount = num(e)?,
                _ => {}
            }
        }
        spec.rgb = parse_colour(strip(colour))?;
        if let Expr::Tuple(t) = strip(size) {
            let mut it = t.elems.iter();
            let sw = num(it.next()?)?;
            let sh = num(it.next()?)?;
            spec.scale = [ratio(sw, w), ratio(sh, h)];
        }
        Some(spec)
    }
}

/// Size over object size, rounded so a codegen -> import round trip is exact
/// to the thousandth rather than drifting by float error each pass.
fn ratio(size: f32, of: f32) -> f32 {
    if of.abs() < f32::EPSILON {
        return 1.0;
    }
    ((size / of) * 1000.0).round() / 1000.0
}

fn strip(e: &Expr) -> &Expr {
    match e {
        Expr::Paren(p) => strip(&p.expr),
        Expr::Group(g) => strip(&g.expr),
        _ => e,
    }
}

fn path_tail(e: &Expr) -> Option<String> {
    match strip(e) {
        Expr::Path(p) => Some(p.path.segments.last()?.ident.to_string()),
        _ => None,
    }
}

fn collect_path_tails(e: &Expr, out: &mut Vec<String>) {
    match strip(e) {
        Expr::Binary(b) => {
            collect_path_tails(&b.left, out);
            collect_path_tails(&b.right, out);
        }
        other => {
            if let Some(n) = path_tail(other) {
                out.push(n);
            }
        }
    }
}

fn num(e: &Expr) -> Option<f32> {
    match strip(e) {
        Expr::Lit(l) => match &l.lit {
            Lit::Float(f) => f.base10_parse::<f32>().ok(),
            Lit::Int(i) => i.base10_parse::<f32>().ok(),
            _ => None,
        },
        Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => num(&u.expr).map(|v| -v),
        _ => None,
    }
}

fn linear_to_srgb8(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// `EffectColor::srgb8(r, g, b)`, `::srgb(r, g, b)`, `::linear(r, g, b)`,
/// `Color(r, g, b, a)` or `EffectColor::WHITE` -> 8-bit sRGB.
fn parse_colour(e: &Expr) -> Option<[u8; 3]> {
    if path_tail(e).as_deref() == Some("WHITE") {
        return Some([255, 255, 255]);
    }
    let Expr::Call(c) = e else { return None };
    let f = path_tail(&c.func)?;
    let v: Vec<f32> = c.args.iter().take(3).map(num).collect::<Option<_>>()?;
    if v.len() < 3 {
        return None;
    }
    Some(match f.as_str() {
        "srgb8" | "Color" => [v[0] as u8, v[1] as u8, v[2] as u8],
        "srgb" => v.iter().map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8).collect::<Vec<_>>().try_into().ok()?,
        "linear" => [linear_to_srgb8(v[0]), linear_to_srgb8(v[1]), linear_to_srgb8(v[2])],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENGINE: &str = include_str!("../../../wgpu_canvas/src/effect.rs");

    fn lit(v: f32) -> String {
        crate::services::codegen::f32_lit(v)
    }

    /// `pub enum Effect { .. }` from the engine source: variant -> fields.
    fn engine_variants() -> Vec<(String, Vec<String>)> {
        let start = ENGINE.find("pub enum Effect {").expect("engine Effect enum");
        let body = &ENGINE[start..];
        let body = &body[..body.find("\n}\n").expect("enum end")];
        let mut out = Vec::new();
        for line in body.lines().skip(1) {
            let t = line.trim();
            if t.starts_with("///") || t.is_empty() {
                continue;
            }
            let (name, rest) = t.split_once(' ').unwrap_or((t.trim_end_matches(','), ""));
            let fields = rest
                .trim_start_matches('{')
                .trim_end_matches(',')
                .trim_end_matches('}')
                .split(',')
                .filter_map(|f| f.split(':').next().map(|n| n.trim().to_owned()))
                .filter(|n| !n.is_empty())
                .collect();
            out.push((name.to_owned(), fields));
        }
        out
    }

    #[test]
    fn authorable_effects_match_the_engine() {
        let engine = engine_variants();
        assert!(engine.len() >= 13, "failed to read the engine enum: {engine:?}");
        let not_authorable = ["Image", "AnimatedImage"];
        for (name, fields) in &engine {
            if not_authorable.contains(&name.as_str()) {
                continue;
            }
            let kind = EffectKind::from_variant(name)
                .unwrap_or_else(|| panic!("engine Effect::{name} has no forge EffectKind — author it or list it as not authorable"));
            let ours: Vec<String> = kind.fields().iter().map(|s| s.to_string()).collect();
            assert_eq!(&ours, fields, "fields of Effect::{name}");
            assert!(kind.takes(kind.amount_field()), "{name}'s amount field is not one of its fields");
        }
        for kind in EffectKind::ALL {
            assert!(engine.iter().any(|(n, _)| n == kind.variant()), "forge {kind:?} is gone from the engine");
        }
    }

    #[test]
    fn vfx_flag_names_match_the_engine() {
        let start = ENGINE.find("VfxFlags {").expect("VfxFlags block");
        let body = &ENGINE[start..];
        let body = &body[..body.find("\n    }\n").unwrap()];
        let engine: Vec<&str> = body
            .lines()
            .filter_map(|l| l.trim().split_once(" = ").map(|(n, _)| n))
            .collect();
        assert_eq!(engine, VFX_FLAG_NAMES.to_vec());
    }

    #[test]
    fn every_kind_round_trips_through_codegen_and_import() {
        for kind in EffectKind::ALL {
            let spec = ObjectEffectSpec {
                kind,
                rgb: [255, 140, 41],
                scale: [1.5, 0.25],
                amount: 0.7,
                progress: 0.6,
                snap: Some(0.3),
                edge: ScreenEdgeSpec::Left,
                mode: MarkerModeSpec::WindingUp,
                side: ImpactSideSpec::Taken,
                levels: [0.1, 0.5, 0.9, 1.0],
                flags: vec!["FIRE".into(), "PULSE_GLOW".into()],
            };
            let stmt = spec.set_effect_stmt("gen", 200.0, 80.0, lit);
            let parsed: syn::Stmt = syn::parse_str(stmt.trim()).unwrap_or_else(|e| panic!("{stmt}: {e}"));
            let syn::Stmt::Expr(Expr::MethodCall(call), _) = parsed else { panic!("{stmt}") };
            assert_eq!(call.method, "set_effect");
            let args: Vec<&Expr> = call.args.iter().collect();
            let back = ObjectEffectSpec::from_call_args(&args, 200.0, 80.0).unwrap_or_else(|| panic!("{stmt}"));
            assert_eq!(back.kind, kind);
            assert_eq!(back.rgb, spec.rgb);
            assert_eq!(back.scale, spec.scale, "{stmt}");
            assert!((back.amount - 0.7).abs() < 1e-6, "{stmt}");
            // Only what this kind emits is expected back.
            if kind.takes("progress") { assert!((back.progress - 0.6).abs() < 1e-6); }
            if kind.takes("snap") { assert_eq!(back.snap, Some(0.3)); }
            if kind.takes("edge") { assert_eq!(back.edge, ScreenEdgeSpec::Left); }
            if kind.takes("mode") { assert_eq!(back.mode, MarkerModeSpec::WindingUp); }
            if kind.takes("side") { assert_eq!(back.side, ImpactSideSpec::Taken); }
            if kind.takes("levels") { assert_eq!(back.levels, spec.levels); }
            if kind.takes("flags") {
                // Emitted in bit order, so compare as sets.
                let mut f = back.flags.clone();
                f.sort();
                assert_eq!(f, vec!["FIRE".to_owned(), "PULSE_GLOW".to_owned()]);
            }
        }
    }

    #[test]
    fn codegen_emits_what_the_engine_api_expects() {
        let spec = ObjectEffectSpec {
            kind: EffectKind::EnergyTether,
            rgb: [140, 215, 255],
            scale: [1.0, 1.0],
            amount: 0.95,
            snap: None,
            ..Default::default()
        };
        assert_eq!(
            spec.set_effect_stmt("tether", 1000.0, 70.0, lit),
            "    tether.set_effect(Effect::EnergyTether { intensity: 0.95, snap: None }, \
             EffectColor::srgb8(140, 215, 255), (1000.0, 70.0));\n"
        );
    }

    #[test]
    fn hand_written_colours_import_as_srgb() {
        let e: Expr = syn::parse_str("EffectColor::linear(1.0, 0.2622, 0.0222)").unwrap();
        let rgb = parse_colour(&e).unwrap();
        assert_eq!(rgb[0], 255);
        assert!((rgb[1] as i32 - 140).abs() <= 1 && (rgb[2] as i32 - 41).abs() <= 1, "{rgb:?}");
        let e: Expr = syn::parse_str("Color(10, 20, 30, 255)").unwrap();
        assert_eq!(parse_colour(&e), Some([10, 20, 30]));
    }
}
