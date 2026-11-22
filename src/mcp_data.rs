//! MCP tools over a project's data files (`project.qforge.json` + `data/scenes/<scene>/...`):
//! the files the editor, the Forge Player and the code generator all read. An agent edits one
//! object, event or logic tree at a time, each checked against its type before it is written.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use crate::core::project::{ProjectManifest, SceneDocument};
use crate::core::quartz_domain::{LogicTree, QuartzEventBinding, QuartzObjectBlueprint, QuartzTargetRef};
use crate::core::samples::Samples;
use crate::core::store::ProjectStore;

/// The data-file tools: their MCP descriptions and their calls.
pub struct DataTools;

impl DataTools {
    /// `(name, description, input schema)` for each tool.
    pub fn definitions() -> Vec<(&'static str, &'static str, Value)> {
        let root = json!({ "type": "string", "description": "Absolute or workspace-relative project root" });
        vec![
            (
                "qf_project_data_check",
                "Load a project's data files the way the editor and the Forge Player do: per scene, which file holds each object, event and logic tree, plus every problem found (a listed part with no file, an unreadable file). Run after editing data files by hand.",
                json!({ "type": "object", "properties": { "project_root": root }, "required": ["project_root"] }),
            ),
            (
                "qf_project_entity",
                "Get, set or delete one object, event or logic tree in a scene's data files. `set` checks the JSON against the type and writes only the files that change; the running Forge Player and an open editor pick the change up.",
                json!({
                    "type": "object",
                    "properties": {
                        "project_root": root,
                        "op": { "type": "string", "enum": ["get", "set", "delete"] },
                        "kind": { "type": "string", "enum": ["object", "event", "logic_tree"] },
                        "id": { "type": "string" },
                        "scene": { "type": "string", "description": "Scene id or name; default the active scene" },
                        "value": { "type": "object", "description": "For set: the whole entity (its `id` must match)" }
                    },
                    "required": ["project_root", "op", "kind", "id"]
                }),
            ),
            (
                "qf_project_examples",
                "A valid JSON example of every action and condition, an object, an event binding and a logic tree, exactly as the data files hold them. Copy from these instead of guessing a shape.",
                json!({ "type": "object", "properties": {} }),
            ),
        ]
    }

    /// The tool's answer, or `None` when `name` is not one of these tools.
    pub fn call(name: &str, args: &Value, root: Option<PathBuf>) -> Option<Result<Value>> {
        let needs_root = || root.clone().ok_or_else(|| anyhow!("{name} requires arguments.project_root"));
        Some(match name {
            "qf_project_data_check" => needs_root().and_then(|root| Self::check(&root)),
            "qf_project_entity" => needs_root().and_then(|root| Self::entity(&root, args)),
            "qf_project_examples" => Ok(Self::examples()),
            _ => return None,
        })
    }

    fn check(root: &Path) -> Result<Value> {
        let store = ProjectStore::new(root);
        let (manifest, report) = store.load().with_context(|| format!("failed to load {}", root.display()))?;
        let dirs = ProjectStore::scene_dir_names(&manifest);
        let scenes: Vec<Value> = manifest
            .scenes
            .iter()
            .zip(&dirs)
            .map(|(scene, dir)| {
                let base = format!("data/scenes/{dir}");
                json!({
                    "id": scene.id,
                    "name": scene.name,
                    "scene_file": format!("{base}/scene.json"),
                    "objects": scene.objects.iter().map(|object| json!({ "id": object.id, "file": format!("{base}/objects/{}.json", object.id) })).collect::<Vec<_>>(),
                    "events": scene.events.iter().map(|event| json!({ "id": event.id, "file": format!("{base}/events/{}.json", event.id) })).collect::<Vec<_>>(),
                    "logic_trees": scene.logic_trees.iter().map(|tree| json!({ "id": tree.id, "file": format!("{base}/logic/{}.json", tree.id) })).collect::<Vec<_>>(),
                    "variables": scene.variables.len(),
                })
            })
            .collect();
        Ok(json!({
            "project_root": root,
            "active_scene_id": manifest.active_scene_id,
            "scenes": scenes,
            "warnings": report.warnings,
            "older_single_file_project": report.legacy,
        }))
    }

    fn entity(root: &Path, args: &Value) -> Result<Value> {
        let text = |key: &str| args.get(key).and_then(Value::as_str).ok_or_else(|| anyhow!("qf_project_entity requires arguments.{key}"));
        let (op, kind, id) = (text("op")?, text("kind")?, text("id")?);
        let store = ProjectStore::new(root);
        let (mut manifest, _) = store.load().with_context(|| format!("failed to load {}", root.display()))?;
        let scene = Self::scene_mut(&mut manifest, args.get("scene").and_then(Value::as_str))?;
        let answer = match op {
            "get" => Self::get(scene, kind, id)?,
            "set" => Self::set(scene, kind, id, args.get("value").cloned().ok_or_else(|| anyhow!("set requires arguments.value"))?)?,
            "delete" => Self::delete(scene, kind, id)?,
            other => bail!("unknown op `{other}`: use get, set or delete"),
        };
        if op == "get" {
            return Ok(answer);
        }
        let report = store.save(&manifest)?;
        Ok(json!({ "result": answer, "written": report.written, "removed": report.removed }))
    }

    fn scene_mut<'a>(manifest: &'a mut ProjectManifest, wanted: Option<&str>) -> Result<&'a mut SceneDocument> {
        let chosen = wanted.map(str::to_owned).or_else(|| manifest.active_scene_id.clone());
        let index = match chosen {
            Some(key) => manifest.scenes.iter().position(|scene| scene.id == key || scene.name == key),
            None => (!manifest.scenes.is_empty()).then_some(0),
        };
        let index = index.ok_or_else(|| anyhow!("no such scene; scenes: {:?}", manifest.scenes.iter().map(|s| &s.id).collect::<Vec<_>>()))?;
        Ok(&mut manifest.scenes[index])
    }

    fn get(scene: &SceneDocument, kind: &str, id: &str) -> Result<Value> {
        let found = match kind {
            "object" => scene.objects.iter().find(|item| item.id == id).map(serde_json::to_value),
            "event" => scene.events.iter().find(|item| item.id == id).map(serde_json::to_value),
            "logic_tree" => scene.logic_trees.iter().find(|item| item.id == id).map(serde_json::to_value),
            other => bail!("unknown kind `{other}`"),
        };
        found.ok_or_else(|| anyhow!("no {kind} `{id}` in scene `{}`", scene.id))?.map_err(Into::into)
    }

    fn set(scene: &mut SceneDocument, kind: &str, id: &str, value: Value) -> Result<Value> {
        let created = match kind {
            "object" => Self::upsert(&mut scene.objects, id, value, |item: &QuartzObjectBlueprint| &item.id)?,
            "event" => Self::upsert(&mut scene.events, id, value, |item: &QuartzEventBinding| &item.id)?,
            "logic_tree" => Self::upsert(&mut scene.logic_trees, id, value, |item: &LogicTree| &item.id)?,
            other => bail!("unknown kind `{other}`"),
        };
        Ok(json!({ "kind": kind, "id": id, "created": created }))
    }

    /// Replace the item with `id`, or append it; true when it is new.
    fn upsert<T: serde::de::DeserializeOwned>(items: &mut Vec<T>, id: &str, value: Value, id_of: impl Fn(&T) -> &String) -> Result<bool> {
        let item: T = serde_json::from_value(value)
            .map_err(|error| anyhow!("the value does not match this kind: {error} (qf_project_examples shows valid shapes)"))?;
        if id_of(&item) != id {
            bail!("value.id `{}` differs from id `{id}`", id_of(&item));
        }
        match items.iter().position(|existing| id_of(existing) == id) {
            Some(index) => {
                items[index] = item;
                Ok(false)
            }
            None => {
                items.push(item);
                Ok(true)
            }
        }
    }

    fn delete(scene: &mut SceneDocument, kind: &str, id: &str) -> Result<Value> {
        let before = match kind {
            "object" => Self::remove(&mut scene.objects, |item| item.id == id),
            "event" => Self::remove(&mut scene.events, |item| item.id == id),
            "logic_tree" => Self::remove(&mut scene.logic_trees, |item| item.id == id),
            other => bail!("unknown kind `{other}`"),
        };
        if !before {
            bail!("no {kind} `{id}` in scene `{}`", scene.id);
        }
        Ok(json!({ "kind": kind, "id": id, "deleted": true }))
    }

    fn remove<T>(items: &mut Vec<T>, matches: impl Fn(&T) -> bool) -> bool {
        let before = items.len();
        items.retain(|item| !matches(item));
        items.len() != before
    }

    fn examples() -> Value {
        let samples = Samples {
            target: QuartzTargetRef::Name("player".to_owned()),
            other: QuartzTargetRef::Name("crate".to_owned()),
            template_id: "bullet".to_owned(),
            animation_asset: "assets/player_run.gif".to_owned(),
        };
        let mut tree = LogicTree::new("logic_example".to_owned(), "example".to_owned());
        tree.nodes = samples.actions().into_iter().take(2).map(crate::core::quartz_domain::LogicNode::Action).collect();
        json!({
            "object": QuartzObjectBlueprint::new("player".to_owned(), "player".to_owned()),
            "event": QuartzEventBinding::new("event_jump".to_owned(), "jump".to_owned(), samples.target.clone()),
            "logic_tree": tree,
            "actions": samples.actions(),
            "conditions": samples.conditions(),
            "notes": [
                "Targets are {\"Name\": id}, {\"Tag\": tag} or {\"Id\": id}; an object's name in the game is its id.",
                "Spawn's template_id names an object with spawn_only: true.",
                "ModVar changes only a variable that exists: declare it in the scene's `variables` (scene.json).",
                "A logic tree runs every frame; an event runs its action, or its linked logic tree, when it fires."
            ]
        })
    }
}
