//! Generated code for particle effects: the effect's emitter written inline where it is played,
//! and a texture image loaded once per program (a `OnceLock`), so playing an effect never decodes
//! or uploads its image again.
//!
//! Action code is generated without the project in hand (`action_expr_inner`), so the scene's
//! generator lends the project's effects for the time it runs (`ParticleCodegen::lend`).

use std::cell::RefCell;

use crate::core::particle_effect::{ParticleEffectAsset, ParticleShapeKind};
use crate::core::quartz_domain::{QuartzAction, QuartzObjectBlueprint};
use crate::services::codegen_text::target_expr;

thread_local! {
    static LENT: RefCell<Vec<ParticleEffectAsset>> = const { RefCell::new(Vec::new()) };
}

/// Writes particle effect code.
pub struct ParticleCodegen;

/// The effects lent to generation; dropping it takes them back.
pub struct LentEffects;

impl ParticleCodegen {
    /// Lend `effects` to every particle action generated until the guard is dropped.
    pub fn lend(effects: &[ParticleEffectAsset]) -> LentEffects {
        LENT.with(|lent| lent.borrow_mut().clone_from(&effects.to_vec()));
        LentEffects
    }

    /// The engine action for a particle action, or `None` for any other action.
    pub fn action_expr(action: &QuartzAction) -> Option<String> {
        match action {
            QuartzAction::PlayParticles { effect, name, target, offset } => {
                let shown_name = if name.is_empty() { effect } else { name };
                let target = target.as_ref().map_or_else(|| "None".to_owned(), |target| format!("Some({})", target_expr(target)));
                Some(format!(
                    "Action::PlayParticles {{ name: {shown_name:?}.to_owned(), emitter: {}, image: {}, target: {target}, offset: ({:?}, {:?}) }}",
                    Self::emitter_expr(effect),
                    Self::image_expr(effect),
                    offset[0],
                    offset[1]
                ))
            }
            QuartzAction::StopParticles { name } => Some(format!("Action::StopParticles {{ name: {name:?}.to_owned() }}")),
            QuartzAction::RemoveParticles { name } => Some(format!("Action::RemoveParticles {{ name: {name:?}.to_owned() }}")),
            _ => None,
        }
    }

    /// Statements that start the effects an object lists, once it is on the canvas.
    pub fn object_lines(blueprint: &QuartzObjectBlueprint) -> String {
        let mut out = String::new();
        for attached in &blueprint.particles {
            let name = format!("{}:{}", blueprint.id, attached.effect);
            out.push_str(&format!(
                "    for playing in canvas.play_particles({name:?}, {}, Some(&Target::name({:?})), ({:?}, {:?})) {{\n",
                Self::emitter_expr(&attached.effect),
                blueprint.id,
                attached.offset[0],
                attached.offset[1]
            ));
            out.push_str(&format!("        if let Some(image) = {} {{ canvas.set_particles_image(&playing, image); }}\n", Self::image_expr(&attached.effect)));
            out.push_str("    }\n");
        }
        out
    }

    fn lent(effect: &str) -> Option<ParticleEffectAsset> {
        LENT.with(|lent| lent.borrow().iter().find(|asset| asset.name == effect).cloned())
    }

    /// The emitter literal, or the engine default when the project has no such effect (the
    /// editor reports a missing or blocked effect before code is generated).
    /// A block naming the effect (`let _effect = "fire";`), which import reads back, then the
    /// literal.
    fn emitter_expr(effect: &str) -> String {
        let literal = Self::lent(effect).map_or_else(|| "quartz::gpu_particles::ParticleEmitter::default()".to_owned(), |asset| asset.rust_emitter());
        format!("{{ let _effect = {effect:?}; {literal} }}")
    }

    /// The effect a generated emitter expression names, for import.
    pub fn effect_named_by(expr: &syn::Expr) -> Option<String> {
        let syn::Expr::Block(block) = expr else { return None };
        let Some(syn::Stmt::Local(local)) = block.block.stmts.first() else { return None };
        let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(name), .. }) = local.init.as_ref()?.expr.as_ref() else { return None };
        Some(name.value())
    }

    fn image_expr(effect: &str) -> String {
        let Some(asset) = Self::lent(effect).filter(|asset| asset.look.shape == ParticleShapeKind::Texture) else {
            return "None".to_owned();
        };
        let path = asset.look.image.trim().trim_start_matches("./").trim_start_matches('/').replace('\\', "/").replace('"', "\\\"");
        format!(
            "{{ static IMAGE: std::sync::OnceLock<Arc<prism::canvas::RgbaImage>> = std::sync::OnceLock::new(); \
             Some(IMAGE.get_or_init(|| quartz::sprite::load_image(include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{path}\"))).image).clone()) }}"
        )
    }
}

impl Drop for LentEffects {
    fn drop(&mut self) {
        LENT.with(|lent| lent.borrow_mut().clear());
    }
}

#[cfg(test)]
mod tests {
    use super::ParticleCodegen;
    use crate::core::particle_effect::{ObjectParticles, ParticleEffectAsset};
    use crate::core::quartz_domain::{QuartzAction, QuartzObjectBlueprint, QuartzTargetRef};

    #[test]
    fn a_played_effect_is_written_inline_and_parses() {
        let _lent = ParticleCodegen::lend(&[ParticleEffectAsset::new("fire")]);
        let action = QuartzAction::PlayParticles { effect: "fire".into(), name: String::new(), target: Some(QuartzTargetRef::Name("torch".into())), offset: [0.0, -8.0] };
        let text = ParticleCodegen::action_expr(&action).unwrap_or_default();
        assert!(text.contains("Emission::Stream") && text.contains("name: \"fire\""), "{text}");
        assert!(syn::parse_str::<syn::Expr>(&text).is_ok(), "{text}");
        let mut torch = QuartzObjectBlueprint::new("torch".into(), "torch".into());
        torch.particles.push(ObjectParticles { effect: "fire".into(), offset: [0.0, 0.0] });
        let lines = ParticleCodegen::object_lines(&torch);
        assert!(syn::parse_str::<syn::Block>(&format!("{{{lines}}}")).is_ok(), "{lines}");
        assert!(lines.contains("\"torch:fire\""));
        let parsed: syn::Expr = syn::parse_str(&text).unwrap_or_else(|error| panic!("{error}"));
        let syn::Expr::Struct(play) = parsed else { panic!("{text}") };
        let emitter = play.fields.iter().find(|field| matches!(&field.member, syn::Member::Named(name) if name == "emitter")).map(|field| &field.expr);
        assert_eq!(emitter.and_then(ParticleCodegen::effect_named_by).as_deref(), Some("fire"));
    }
}
