//! MCP tools over a project's own shader effects (`data/shaders/<name>.json` and `.wgsl`). An
//! agent writes hand-written WGSL against the engine's contract or a layer stack as JSON; every
//! write is compiled the way the renderer will compile it, and one that does not build is refused
//! with the line and the reason. Shaders are drawn by name: put one on an object with
//! qf_project_entity (the object's `effect.project`).

use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use crate::core::particle_effect::{EffectFinding, FindingLevel};
use crate::core::project::ProjectManifest;
use crate::core::shader_asset::{ProjectShaderAsset, ShaderAuthoring};
use crate::core::shader_engine::ShaderBridge;
use crate::core::shader_layers::{Pattern, Shaping, Warp};
use crate::core::store::ProjectStore;

/// The project shader tool: its MCP description and its calls.
pub struct ShaderTools;

impl ShaderTools {
    pub const NAME: &'static str = "qf_shader";

    /// `(name, description, input schema)`.
    pub fn definitions() -> Vec<(&'static str, &'static str, Value)> {
        vec![(
            Self::NAME,
            "List, get, set, delete or check a project's own shader effects (data/shaders), or read the contract and the layer-stack parts. A shader is either `authoring: {kind: \"layers\", stack: {layers: [...]}}` (patterns, warps, shaping, colour, blend; see op `parts`) or `{kind: \"wgsl\"}` with the source in `wgsl`: one `fn effect(input: EffectInput) -> vec4<f32>` returning linear colour and straight alpha (op `contract` shows EffectInput and the fx_ helpers). `set` compiles it as the renderer will and refuses one that does not build, with the line. Up to 8 named params reach the shader as input.a and input.b. Use one on an object through its effect's `project: {shader, params, additive}`.",
            json!({
                "type": "object",
                "properties": {
                    "project_root": { "type": "string", "description": "Absolute or workspace-relative project root (not needed for parts or contract)" },
                    "op": { "type": "string", "enum": ["list", "get", "set", "delete", "check", "parts", "contract"] },
                    "name": { "type": "string", "description": "The shader's name (its file name)" },
                    "value": { "type": "object", "description": "For set and check: the whole shader (name, params, additive, authoring)" },
                    "wgsl": { "type": "string", "description": "For set and check of a `wgsl` shader: its source" }
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
        match op {
            "parts" => return Some(Ok(Self::parts())),
            "contract" => return Some(Ok(json!({ "prelude": ShaderBridge::prelude(), "you_write": "fn effect(input: EffectInput) -> vec4<f32> { ... } plus your own helper functions; no bindings or entry points" }))),
            "check" if args.get("value").is_some() => return Some(Self::parse(args).map(|asset| Self::summary(&asset))),
            _ => {}
        }
        Some(root.ok_or_else(|| anyhow!("{name} requires arguments.project_root")).and_then(|root| Self::on_project(root, op, args)))
    }

    fn on_project(root: &Path, op: &str, args: &Value) -> Result<Value> {
        let store = ProjectStore::new(root);
        let (mut manifest, _) = store.load().with_context(|| format!("failed to load {}", root.display()))?;
        let shader_name = || args.get("name").and_then(Value::as_str).ok_or_else(|| anyhow!("{op} requires arguments.name"));
        match op {
            "list" => Ok(json!({ "shaders": manifest.shaders.iter().map(Self::summary).collect::<Vec<_>>() })),
            "get" | "check" => {
                let name = shader_name()?;
                let asset = manifest.shaders.iter().find(|asset| asset.name == name).ok_or_else(|| anyhow!("no shader `{name}`"))?;
                Ok(json!({ "shader": asset, "wgsl": asset.wgsl(), "check": Self::summary(asset) }))
            }
            "set" => Self::set(&store, &mut manifest, args),
            "delete" => {
                let name = shader_name()?;
                let before = manifest.shaders.len();
                manifest.shaders.retain(|asset| asset.name != name);
                if before == manifest.shaders.len() {
                    bail!("no shader `{name}`");
                }
                let report = store.save(&manifest)?;
                Ok(json!({ "deleted": name, "removed": report.removed }))
            }
            other => bail!("unknown op `{other}`: use list, get, set, delete, check, parts or contract"),
        }
    }

    fn set(store: &ProjectStore, manifest: &mut ProjectManifest, args: &Value) -> Result<Value> {
        let asset = Self::parse(args)?;
        let summary = Self::summary(&asset);
        if summary["drawn"] != true {
            return Ok(json!({ "saved": false, "why": "the shader does not build or the check blocks it", "check": summary }));
        }
        match manifest.shaders.iter_mut().find(|existing| existing.name == asset.name) {
            Some(existing) => *existing = asset,
            None => manifest.shaders.push(asset),
        }
        let report = store.save(manifest)?;
        Ok(json!({ "saved": true, "written": report.written, "check": summary }))
    }

    /// The shader in `value`, with a `wgsl` shader's source from `wgsl`.
    fn parse(args: &Value) -> Result<ProjectShaderAsset> {
        let value = args.get("value").cloned().ok_or_else(|| anyhow!("requires arguments.value"))?;
        let mut asset: ProjectShaderAsset = serde_json::from_value(value).map_err(|error| anyhow!("the value is not a shader: {error} (op parts shows the layer parts)"))?;
        if asset.authoring == ShaderAuthoring::Wgsl {
            asset.wgsl_source = args.get("wgsl").and_then(Value::as_str).unwrap_or_default().to_owned();
        }
        Ok(asset)
    }

    /// Whether the shader is drawn, why not, and what it costs when that has been measured.
    fn summary(asset: &ProjectShaderAsset) -> Value {
        let mut findings: Vec<EffectFinding> = asset.blocking();
        if findings.is_empty()
            && let Err(problem) = ShaderBridge::validate(asset)
        {
            let line = problem.line.map_or_else(String::new, |line| format!("line {line}: "));
            findings.push(EffectFinding::block(format!("{line}{}", problem.message)));
        }
        let drawn = findings.is_empty();
        findings.extend(asset.warnings());
        let listed: Vec<Value> = findings.into_iter().map(|finding| {
            let level = match finding.level {
                FindingLevel::Block => "block",
                FindingLevel::Warn => "warn",
                FindingLevel::Info => "info",
            };
            json!({ "level": level, "message": finding.message })
        }).collect();
        let cost = asset.current_weight().map_or_else(|| json!("not measured for this source; Quartz Forge measures it when the shader is saved there"), |weight| json!({ "per_pixel_vs_plain_sprite": weight }));
        json!({ "name": asset.name, "drawn": drawn, "params": asset.params, "cost": cost, "findings": listed })
    }

    /// Every layer part with sensible values, as the JSON a stack takes.
    fn parts() -> Value {
        let named = |parts: Vec<(&'static str, Value)>| parts.into_iter().map(|(label, value)| json!({ "label": label, "json": value })).collect::<Vec<_>>();
        let to_json = |value: serde_json::Result<Value>| value.unwrap_or(Value::Null);
        json!({
            "layer": { "name": "Glow", "enabled": true, "pattern": "<pattern>", "warps": ["<warp>"], "shaping": ["<shaping>"], "color": "{type: tint} | {type: solid, rgb} | {type: ramp, stops: [{at, rgb}]} | {type: image}", "opacity": 1.0, "blend": "normal | add | multiply | screen | mask | cut" },
            "numbers": "Any number is a bare value or {value, by: {type: time, rate} | {type: wave, amount, speed} | {type: param, slot, amount} | {type: layer, layer, amount}}. A layer may read only layers below it.",
            "coordinates": "p runs -1..1 across the quad's shorter side from its centre, y down.",
            "patterns": named(Pattern::every().into_iter().map(|(label, part)| (label, to_json(serde_json::to_value(part)))).collect()),
            "warps": named(Warp::every().into_iter().map(|(label, part)| (label, to_json(serde_json::to_value(part)))).collect()),
            "shaping": named(Shaping::every().into_iter().map(|(label, part)| (label, to_json(serde_json::to_value(part)))).collect()),
            "example": to_json(serde_json::to_value(ProjectShaderAsset::layered("glow"))),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ShaderTools;
    use crate::core::project::ProjectManifest;
    use crate::core::store::ProjectStore;
    use serde_json::json;

    #[test]
    fn an_agent_writes_a_layered_and_a_wgsl_shader_and_a_broken_one_is_refused_with_its_line() {
        let root = std::env::temp_dir().join(format!("qf_shader_tool_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        ProjectStore::new(&root).save(&ProjectManifest::new("shaders".into())).unwrap_or_else(|error| panic!("{error}"));
        let call = |args: serde_json::Value| ShaderTools::call(ShaderTools::NAME, &args, Some(&root)).and_then(Result::ok).unwrap_or_default();
        let parts = call(json!({ "op": "parts" }));
        let example = parts["example"].clone();
        let saved = call(json!({ "op": "set", "value": example }));
        assert_eq!(saved["saved"], true, "{saved}");
        assert!(root.join("data/shaders/glow.wgsl").is_file());
        let wgsl = json!({ "name": "lens", "params": [], "authoring": { "kind": "wgsl" } });
        let source = "fn effect(input: EffectInput) -> vec4<f32> {\n    return vec4<f32>(input.tint.rgb, 1.0 - length(input.centered));\n}\n";
        assert_eq!(call(json!({ "op": "set", "value": wgsl, "wgsl": source }))["saved"], true);
        let refused = call(json!({ "op": "set", "value": wgsl, "wgsl": "fn effect(input: EffectInput) -> vec4<f32> {\n    return lens;\n}\n" }));
        assert_eq!(refused["saved"], false, "{refused}");
        assert!(refused["check"]["findings"][0]["message"].as_str().is_some_and(|message| message.starts_with("line 2:")), "{refused}");
        let listed = call(json!({ "op": "list" }));
        assert_eq!(listed["shaders"].as_array().map(Vec::len), Some(2), "{listed}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
