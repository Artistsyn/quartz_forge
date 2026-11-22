//! MCP tools over a project's particle effects (`data/effects/<name>.json`). Writing runs the
//! same check the editor shows: an effect it blocks is refused with the reasons, and every write
//! answers with what the effect costs, so an agent designs within the mega shader pass's limits.

use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use crate::core::object_effect::ObjectEffectSpec;
use crate::core::particle_effect::{EffectFinding, FindingLevel, ParticleEffectAsset};
use crate::core::particle_engine::ParticleBridge;
use crate::core::project::{ProjectManifest, SceneDocument};
use crate::core::shader_asset::ProjectShaderAsset;
use crate::core::store::ProjectStore;

/// The particle effect tool: its MCP description and its calls.
pub struct EffectTools;

impl EffectTools {
    pub const NAME: &'static str = "qf_particle_effect";

    /// `(name, description, input schema)`.
    pub fn definitions() -> Vec<(&'static str, &'static str, Value)> {
        vec![(
            Self::NAME,
            "List, get, set, delete or check a project's GPU particle effects (data/effects/<name>.json), or list the engine presets to start from. Effects draw in the mega shader pass from their emitter alone; `look.keys` holds up to two moments over life (`{ at: 0..1, color?: [r,g,b,a], size?: px }`) the look runs through. `set` refuses an effect the check blocks and every answer prices the effect (particles alive at once, screens of fill, whether it batches). `list` also prices every object shader effect in the scene, and `shader_check` prices a shader effect spec (`value`, an object's `effect`) on an object `object_size` big, from per-kind weights measured on the GPU. Put one on an object with qf_project_entity (object `particles`), or play it with the PlayParticles action.",
            json!({
                "type": "object",
                "properties": {
                    "project_root": { "type": "string", "description": "Absolute or workspace-relative project root (not needed for presets)" },
                    "op": { "type": "string", "enum": ["list", "get", "set", "delete", "check", "shader_check", "presets"] },
                    "name": { "type": "string", "description": "The effect's name (its file name)" },
                    "value": { "type": "object", "description": "For set: the whole effect; for check: an effect to check without saving it; for shader_check: an object shader effect spec" },
                    "object_size": { "type": "array", "items": { "type": "number" }, "description": "For shader_check: the object's [w, h] in scene pixels" },
                    "scene": { "type": "string", "description": "Scene id or name whose size prices fill; default the active scene" }
                },
                "required": ["op"]
            }),
        )]
    }

    /// The tool's answer, or `None` when `name` is not this tool.
    pub fn call(name: &str, args: &Value, root: Option<&Path>) -> Option<Result<Value>> {
        if name != Self::NAME {
            return None;
        }
        let op = args.get("op").and_then(Value::as_str).unwrap_or_default();
        if op == "presets" {
            return Some(Ok(json!({ "presets": ParticleBridge::presets() })));
        }
        Some(root.ok_or_else(|| anyhow!("{name} requires arguments.project_root")).and_then(|root| Self::on_project(root, op, args)))
    }

    fn on_project(root: &Path, op: &str, args: &Value) -> Result<Value> {
        let store = ProjectStore::new(root);
        let (mut manifest, _) = store.load().with_context(|| format!("failed to load {}", root.display()))?;
        let scene = Self::scene(&manifest, args.get("scene").and_then(Value::as_str));
        let scene_size = scene.map_or((1280.0, 720.0), |scene| (scene.canvas.virtual_width, scene.canvas.virtual_height));
        let effect_name = || args.get("name").and_then(Value::as_str).ok_or_else(|| anyhow!("{op} requires arguments.name"));
        match op {
            "list" => Ok(json!({
                "effects": manifest.particle_effects.iter().map(|asset| Self::summary(asset, scene_size)).collect::<Vec<_>>(),
                "shader_effects": scene.map(|scene| Self::scene_shaders(scene, scene_size, &manifest.shaders)).unwrap_or_default(),
            })),
            "shader_check" => {
                let spec: ObjectEffectSpec = serde_json::from_value(args.get("value").cloned().unwrap_or_default()).map_err(|error| anyhow!("the value is not a shader effect spec: {error}"))?;
                let size = args.get("object_size").and_then(Value::as_array).map(|pair| pair.iter().filter_map(Value::as_f64).map(|side| side as f32).collect::<Vec<_>>()).unwrap_or_default();
                let [w, h] = size[..] else { bail!("shader_check requires arguments.object_size as [w, h]") };
                Ok(Self::shader_summary(&spec, (w, h), scene_size, &manifest.shaders))
            }
            "get" => {
                let name = effect_name()?;
                let asset = manifest.particle_effects.iter().find(|asset| asset.name == name).ok_or_else(|| anyhow!("no effect `{name}`"))?;
                Ok(json!({ "effect": asset, "check": Self::summary(asset, scene_size) }))
            }
            "check" => {
                let asset = match args.get("value") {
                    Some(value) => Self::parse(value.clone())?,
                    None => {
                        let name = effect_name()?;
                        manifest.particle_effects.iter().find(|asset| asset.name == name).cloned().ok_or_else(|| anyhow!("no effect `{name}`"))?
                    }
                };
                Ok(Self::summary(&asset, scene_size))
            }
            "set" => Self::set(&store, &mut manifest, args, scene_size),
            "delete" => {
                let name = effect_name()?;
                let before = manifest.particle_effects.len();
                manifest.particle_effects.retain(|asset| asset.name != name);
                if before == manifest.particle_effects.len() {
                    bail!("no effect `{name}`");
                }
                let report = store.save(&manifest)?;
                Ok(json!({ "deleted": name, "removed": report.removed }))
            }
            other => bail!("unknown op `{other}`: use list, get, set, delete, check or presets"),
        }
    }

    fn set(store: &ProjectStore, manifest: &mut ProjectManifest, args: &Value, scene_size: (f32, f32)) -> Result<Value> {
        let mut asset = Self::parse(args.get("value").cloned().ok_or_else(|| anyhow!("set requires arguments.value"))?)?;
        if let Some(name) = args.get("name").and_then(Value::as_str) {
            asset.name = name.to_owned();
        }
        if asset.is_blocked() {
            return Ok(json!({ "saved": false, "why": "the check blocks this effect; fix every `Block` finding", "check": Self::summary(&asset, scene_size) }));
        }
        match manifest.particle_effects.iter_mut().find(|existing| existing.name == asset.name) {
            Some(existing) => existing.clone_from(&asset),
            None => manifest.particle_effects.push(asset.clone()),
        }
        let report = store.save(manifest)?;
        Ok(json!({ "saved": true, "written": report.written, "check": Self::summary(&asset, scene_size) }))
    }

    fn parse(value: Value) -> Result<ParticleEffectAsset> {
        serde_json::from_value(value).map_err(|error| anyhow!("the value is not a particle effect: {error} (op `presets` shows valid ones)"))
    }

    fn summary(asset: &ParticleEffectAsset, scene_size: (f32, f32)) -> Value {
        let cost = asset.cost(scene_size);
        let findings = Self::findings(asset.check(scene_size));
        json!({ "name": asset.name, "played": !asset.is_blocked(), "particles_at_once": cost.slots, "fill_screens": cost.fill_screens, "batches": cost.batches, "findings": findings })
    }

    fn shader_summary(spec: &ObjectEffectSpec, object_size: (f32, f32), scene_size: (f32, f32), shaders: &[ProjectShaderAsset]) -> Value {
        let cost = spec.cost(object_size, scene_size, shaders);
        let findings = Self::findings(spec.check(object_size, scene_size, shaders));
        let kind = spec.project.as_ref().map_or_else(|| spec.kind.variant().to_owned(), |choice| format!("project shader {}", choice.shader));
        json!({ "kind": kind, "weight": cost.weight, "fill_screens": cost.fill_screens, "plain_screens": cost.plain_screens, "findings": findings })
    }

    fn scene_shaders(scene: &SceneDocument, scene_size: (f32, f32), shaders: &[ProjectShaderAsset]) -> Vec<Value> {
        scene.objects.iter().filter_map(|blueprint| {
            let mut summary = Self::shader_summary(blueprint.effect.as_ref()?, (blueprint.w, blueprint.h), scene_size, shaders);
            summary["object"] = json!(blueprint.name);
            Some(summary)
        }).collect()
    }

    fn findings(findings: Vec<EffectFinding>) -> Vec<Value> {
        findings.into_iter().map(|finding| {
            let level = match finding.level {
                FindingLevel::Block => "block",
                FindingLevel::Warn => "warn",
                FindingLevel::Info => "info",
            };
            json!({ "level": level, "message": finding.message })
        }).collect()
    }

    fn scene<'a>(manifest: &'a ProjectManifest, wanted: Option<&str>) -> Option<&'a SceneDocument> {
        match wanted {
            Some(wanted) => manifest.scenes.iter().find(|scene| scene.id == wanted || scene.name == wanted),
            None => manifest.active_scene_id.as_ref().and_then(|id| manifest.scenes.iter().find(|scene| &scene.id == id)).or_else(|| manifest.scenes.first()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EffectTools;
    use crate::core::project::ProjectManifest;
    use crate::core::store::ProjectStore;
    use serde_json::json;

    #[test]
    fn an_agent_makes_an_effect_from_a_preset_and_a_blocked_one_is_refused() {
        let root = std::env::temp_dir().join(format!("qf_effect_tool_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        ProjectStore::new(&root).save(&ProjectManifest::new("effects".into())).unwrap_or_else(|error| panic!("{error}"));
        let call = |args: serde_json::Value| EffectTools::call(EffectTools::NAME, &args, Some(&root)).and_then(Result::ok).unwrap_or_default();
        let presets = call(json!({ "op": "presets" }));
        let fire = presets["presets"].as_array().and_then(|list| list.iter().find(|preset| preset["name"] == "fire")).cloned().unwrap_or_default();
        let saved = call(json!({ "op": "set", "name": "torch", "value": fire }));
        assert_eq!(saved["saved"], true, "{saved}");
        assert!(root.join("data/effects/torch.json").is_file());
        let mut broken = fire.clone();
        broken["emission"]["lifetime"] = json!([0.0, 0.0]);
        let refused = call(json!({ "op": "set", "name": "broken", "value": broken }));
        assert_eq!(refused["saved"], false, "{refused}");
        assert!(!root.join("data/effects/broken.json").exists());
        let listed = call(json!({ "op": "list" }));
        assert_eq!(listed["effects"][0]["name"], "torch");
        assert!(listed["effects"][0]["particles_at_once"].as_u64().unwrap_or(0) > 0);
        let fire = json!({ "kind": "Animated", "flags": ["FIRE", "ELECTRICITY"] });
        let priced = call(json!({ "op": "shader_check", "value": fire, "object_size": [8000.0, 8000.0] }));
        assert!(priced["weight"].as_f64().unwrap_or(0.0) > 5.0, "{priced}");
        assert_eq!(priced["findings"][0]["level"], "warn", "{priced}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
