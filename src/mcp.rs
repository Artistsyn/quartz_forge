use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::core::project::{EditorProjectState, ProjectManifest};
use crate::services::codegen;
use crate::services::project_import;
use crate::services::persistence;
use crate::services::project_sync;

#[derive(Debug, Clone)]
struct WorkspacePaths {
    root: PathBuf,
    quartz_api_txt: PathBuf,
    quartz_action_rs: PathBuf,
    quartz_condition_rs: PathBuf,
    forge_domain_rs: PathBuf,
    mcp_dir: PathBuf,
    lock_file: PathBuf,
    heartbeat_file: PathBuf,
}

#[derive(Debug, Serialize)]
struct ToolInfo {
    name: &'static str,
    description: &'static str,
    input_schema: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

#[derive(Debug, Clone, Copy)]
enum MessageFraming {
    LineDelimited,
    ContentLength,
}

#[allow(dead_code)]
struct LockGuard {
    lock_file: PathBuf,
    heartbeat_file: PathBuf,
    heartbeat_alive: Arc<AtomicBool>,
    heartbeat_thread: Option<thread::JoinHandle<()>>,
}

#[derive(Debug, Serialize)]
struct ToolListResult {
    tools: Vec<ToolInfo>,
}

pub fn run_from_args() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let command = args.first().map(String::as_str).unwrap_or("--stdio");
    let paths = locate_workspace_paths()?;

    match command {
        "--stdio" => run_stdio(paths),
        "--health" => {
            let summary = health_report(&paths)?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
            Ok(())
        }
        "--lock-status" => {
            let summary = lock_status(&paths)?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
            Ok(())
        }
        "--help" | "-h" => {
            println!("Quartz Forge MCP server");
            println!("  --stdio        run MCP over stdio");
            println!("  --health       print workspace/tool health as JSON");
            println!("  --lock-status  print lock/heartbeat status as JSON");
            Ok(())
        }
        other => Err(anyhow!("unknown quartz_forge_mcp command: {other}")),
    }
}

fn run_stdio(paths: WorkspacePaths) -> Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut stdout = io::stdout();

    loop {
        let Some((request, framing)) = read_rpc_request(&mut reader)? else {
            break;
        };

        if let Some(response) = handle_rpc_request(&paths, request)? {
            write_rpc_response(&mut stdout, response, framing)?;
            stdout.flush()?;
        }
    }

    Ok(())
}

fn handle_rpc_request(paths: &WorkspacePaths, request: JsonRpcRequest) -> Result<Option<JsonRpcResponse>> {
    let Some(id) = request.id else {
        return Ok(None);
    };
    let _ = request.jsonrpc.as_deref();

    let response = match request.method.as_str() {
        "initialize" => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "quartz_forge_mcp",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "tools": {}
                }
            })),
            error: None,
        },
        "tools/list" => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!(ToolListResult { tools: tool_list() })),
            error: None,
        },
        "tools/call" => {
            let tool_name = request
                .params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("tools/call missing params.name"))?;
            let args = request.params.get("arguments").cloned().unwrap_or(Value::Null);
            let (result_value, is_error) = match call_tool(paths, tool_name, args) {
                Ok(v) => (v, false),
                Err(e) => (json!({"error": e.to_string()}), true),
            };
            let text = serde_json::to_string_pretty(&result_value)
                .unwrap_or_else(|e| format!("{{\"serialize_error\": \"{e}\"}}"));
            JsonRpcResponse {
                jsonrpc: "2.0",
                id,
                result: Some(json!({
                    "content": [{"type": "text", "text": text}],
                    "isError": is_error
                })),
                error: None,
            }
        }
        "ping" => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({"ok": true})),
            error: None,
        },
        other => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: format!("unknown method: {other}"),
            }),
        },
    };

    Ok(Some(response))
}

fn call_tool(paths: &WorkspacePaths, tool_name: &str, args: Value) -> Result<Value> {
    match tool_name {
        "qf_api_lookup" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("qf_api_lookup requires arguments.query"))?;
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(12) as usize;
            Ok(json!({
                "tool": tool_name,
                "query": query,
                "matches": api_lookup(paths, query, limit)?,
            }))
        }
        "qf_api_verify_snippet" => {
            let snippet = args
                .get("snippet")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("qf_api_verify_snippet requires arguments.snippet"))?;
            Ok(json!({
                "tool": tool_name,
                "verified": verify_snippet(paths, snippet)?,
            }))
        }
        "qf_text_knowledge" => Ok(json!({
            "tool": tool_name,
            "knowledge": text_knowledge(paths)?,
        })),
        "qf_project_roundtrip_contract" => Ok(json!({
            "tool": tool_name,
            "contract": project_roundtrip_contract(paths)?,
        })),
        "qf_codegen_api_guidance" => Ok(json!({
            "tool": tool_name,
            "guidance": codegen_api_guidance(paths),
        })),
        "qf_background_plugin_contract" => Ok(json!({
            "tool": tool_name,
            "contract": background_plugin_contract(paths)?,
        })),
        "qf_path_forge_background_contract" => Ok(json!({
            "tool": tool_name,
            "contract": path_forge_background_contract(paths),
        })),
        "qf_project_state_dump" => {
            let project_root = args
                .get("project_root")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("qf_project_state_dump requires arguments.project_root"))?;
            Ok(json!({
                "tool": tool_name,
                "state": project_state_dump(paths, project_root)?,
            }))
        }
        "qf_project_create" => {
            let project_root = args
                .get("project_root")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("qf_project_create requires arguments.project_root"))?;
            let project_name = args
                .get("project_name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("qf_project_create requires arguments.project_name"))?;
            let write_generated_files = args
                .get("write_generated_files")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            Ok(json!({
                "tool": tool_name,
                "result": project_create(paths, project_root, project_name, write_generated_files)?,
            }))
        }
        "qf_project_apply_state" => {
            let project_root = args
                .get("project_root")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("qf_project_apply_state requires arguments.project_root"))?;
            let manifest = args
                .get("manifest")
                .cloned()
                .ok_or_else(|| anyhow!("qf_project_apply_state requires arguments.manifest"))?;
            let write_generated_files = args
                .get("write_generated_files")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let write_sync_snapshot = args
                .get("write_sync_snapshot")
                .and_then(Value::as_bool)
                .unwrap_or(write_generated_files);
            Ok(json!({
                "tool": tool_name,
                "result": project_apply_state(paths, project_root, manifest, write_generated_files, write_sync_snapshot)?,
            }))
        }
        "qf_project_import_manual_overrides" => {
            let project_root = args
                .get("project_root")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("qf_project_import_manual_overrides requires arguments.project_root"))?;
            let files = args
                .get("files")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("qf_project_import_manual_overrides requires arguments.files"))?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            Ok(json!({
                "tool": tool_name,
                "result": project_import_manual_overrides(paths, project_root, &files)?,
            }))
        }
        "qf_project_import_semantic" => {
            let project_root = args
                .get("project_root")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("qf_project_import_semantic requires arguments.project_root"))?;
            let files = args
                .get("files")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("qf_project_import_semantic requires arguments.files"))?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let fallback_manual_overrides = args
                .get("fallback_manual_overrides")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            Ok(json!({
                "tool": tool_name,
                "result": project_import_semantic(paths, project_root, &files, fallback_manual_overrides)?,
            }))
        }
        "qf_project_sync_status" => {
            let project_root = args
                .get("project_root")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("qf_project_sync_status requires arguments.project_root"))?;
            Ok(json!({
                "tool": tool_name,
                "report": project_sync_status(paths, project_root)?,
            }))
        }
        "qf_forge_check_parity" => {
            let surface = match args.get("surface") {
                Some(Value::String(value)) => value.as_str(),
                Some(_) => {
                    return Err(anyhow!(
                        "qf_forge_check_parity requires arguments.surface as string: action|condition|wiring|all"
                    ));
                }
                None => "all",
            };
            Ok(json!({
                "tool": tool_name,
                "surface": surface,
                "report": parity_report(paths, surface)?,
            }))
        }
        "qf_spawn_audit" => Ok(json!({
            "tool": tool_name,
            "report": spawn_audit(paths)?,
        })),
        "qf_project_lint_layout" => Ok(json!({
            "tool": tool_name,
            "report": project_lint_layout(
                paths,
                args.get("project_root").and_then(Value::as_str),
            )?
        })),
        other => Err(anyhow!("unknown tool: {other}")),
    }
}

fn tool_list() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            name: "qf_api_lookup",
            description: "Search local Quartz source and api.txt for exact native API signatures, constructors, and usage notes before custom code is considered.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Native Quartz keyword, symbol, or usage intent" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
                },
                "required": ["query"]
            }),
        },
        ToolInfo {
            name: "qf_api_verify_snippet",
            description: "Validate a proposed snippet against local Quartz source to catch wrong constructors, wrong action names, or custom-code drift.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "snippet": { "type": "string" }
                },
                "required": ["snippet"]
            }),
        },
        ToolInfo {
            name: "qf_text_knowledge",
            description: "Return the current Quartz Forge text-authoring guidance: preferred Text::new/Span::new construction, OnceLock font caching, SetText expectations, and known make_text pitfalls.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: "qf_project_roundtrip_contract",
            description: "Return the source-backed contract for generating a Quartz project that stays editable in quartz_forge, including manifest ownership, runtime scaffold expectations, file-routing rules, and current limitations.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: "qf_codegen_api_guidance",
            description: "Return the canonical Quartz API-first dispatch order and hard rules for AI game code generation. Call this before writing any game logic, event handlers, or on_update closures to ensure generated code uses native Quartz API (GameEvent, Action::Conditional, Action::Multi, Action::SetVar/ModVar, Expr, Condition) before falling back to custom Rust.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: "qf_background_plugin_contract",
            description: "Harvest Quartz background plugin source/README and return a source-backed design contract for a first-class background authoring window plus AI-safe code generation rules.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: "qf_path_forge_background_contract",
            description: "How to give a scene a PathForge background (an endless first-person path walked by the quartz_path_forge plugin, live or from exported frames, with transitions, forks and journeys): the manifest field to set through qf_project_apply_state, the code it generates, the RunPlugin actions and Plugin conditions that drive it, the lint rules, and the workflow with PathForge's own pf_* tools.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: "qf_project_state_dump",
            description: "Load a quartz_forge project and return its structured manifest state plus current sync report so agents can edit project data directly instead of guessing through generated Rust.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_root": { "type": "string", "description": "Absolute or workspace-relative path to the quartz_forge project root" }
                },
                "required": ["project_root"]
            }),
        },
        ToolInfo {
            name: "qf_project_create",
            description: "Create a new quartz_forge project root with default manifest/scaffold files and optionally generate the initial Quartz files plus sync snapshot.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_root": { "type": "string", "description": "Absolute or workspace-relative path where the quartz_forge project should live" },
                    "project_name": { "type": "string", "description": "Human-readable project name" },
                    "write_generated_files": { "type": "boolean", "description": "When true (default), write the initial generated scene files and sync snapshot" }
                },
                "required": ["project_root", "project_name"]
            }),
        },
        ToolInfo {
            name: "qf_project_apply_state",
            description: "Apply a structured quartz_forge manifest state to a project root, optionally regenerate project files, and return the post-apply sync report.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_root": { "type": "string", "description": "Absolute or workspace-relative path to the quartz_forge project root" },
                    "manifest": { "type": "object", "description": "Full ProjectManifest JSON object to save" },
                    "write_generated_files": { "type": "boolean", "description": "When true (default), rewrite generated Quartz files from the manifest state" },
                    "write_sync_snapshot": { "type": "boolean", "description": "When true, rewrite .quartz_forge/sync_snapshot.json after applying state" }
                },
                "required": ["project_root", "manifest"]
            }),
        },
        ToolInfo {
            name: "qf_project_import_manual_overrides",
            description: "Import selected Rust files into quartz_forge manifest metadata as ManualFileOverride blocks so manual work is preserved across regeneration and future loads.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_root": { "type": "string", "description": "Absolute or workspace-relative path to the quartz_forge project root" },
                    "files": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Project-relative or absolute file paths to import as ManualFileOverride blocks"
                    }
                },
                "required": ["project_root", "files"]
            }),
        },
        ToolInfo {
            name: "qf_project_import_semantic",
            description: "Semantically import supported quartz_forge project files back into manifest state and safely fall back to ManualFileOverride when a file goes beyond the current importer contract.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_root": { "type": "string", "description": "Absolute or workspace-relative path to the quartz_forge project root" },
                    "files": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Project-relative or absolute file paths to import"
                    },
                    "fallback_manual_overrides": { "type": "boolean", "description": "When true (default), unsupported files are preserved as ManualFileOverride blocks instead of failing the import" }
                },
                "required": ["project_root", "files"]
            }),
        },
        ToolInfo {
            name: "qf_project_sync_status",
            description: "Inspect a quartz_forge project root for save/export drift and report whether the saved project state and tracked generated files still round-trip cleanly.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_root": { "type": "string", "description": "Absolute or workspace-relative path to the quartz_forge project root" }
                },
                "required": ["project_root"]
            }),
        },
        ToolInfo {
            name: "qf_forge_check_parity",
            description: "Compare quartz_forge domain/editor/codegen support with quartz Action/Condition enums and report missing or extra variants. Also flags generated code that violates the Quartz API-first rule (custom Rust where Action/Condition/GameEvent could handle it).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "surface": {
                        "type": "string",
                        "enum": ["action", "condition", "wiring", "all"],
                        "default": "all"
                    }
                }
            }),
        },
        ToolInfo {
            name: "qf_spawn_audit",
            description: "Inspect first-class spawn-only workflow coverage, overlay readiness, and helper routing for spawn objects.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: "qf_project_lint_layout",
            description: "Check quartz_forge project layout conventions, multi-file module/use wiring, and the intended agent-facing boundaries for Quartz-native project generation.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_root": { "type": "string", "description": "Optional project root to lint. Defaults to workspace root." }
                }
            }),
        },
    ]
}

fn project_lint_layout(paths: &WorkspacePaths, project_root: Option<&str>) -> Result<Value> {
    let root = project_root
        .map(|value| resolve_project_root(paths, value))
        .unwrap_or_else(|| paths.root.clone());

    let mut errors = Vec::<String>::new();
    let mut warnings = Vec::<String>::new();

    for required in ["src", "src/scenes", "src/scripts", "assets", ".quartz_forge"] {
        if !root.join(required).exists() {
            warnings.push(format!("Missing expected directory: {required}"));
        }
    }

    let manifest_path = root.join("project.qforge.json");
    if !manifest_path.exists() {
        errors.push("Missing project.qforge.json manifest".to_owned());
    } else if let Ok(state) = persistence::load_project(&root) {
        for scene in &state.manifest.scenes {
            let source = scene.source_file.trim();
            if source.contains("_scene_scene.rs") {
                errors.push(format!(
                    "Scene '{}' has duplicate scene suffix in source_file: {}",
                    scene.name, source
                ));
            }
            if !source.starts_with("src/scenes/") {
                warnings.push(format!(
                    "Scene '{}' source_file should be under src/scenes/: {}",
                    scene.name, source
                ));
            }
            if !source.ends_with("_scene.rs") {
                warnings.push(format!(
                    "Scene '{}' source_file should end with _scene.rs: {}",
                    scene.name, source
                ));
            }

            // Pool contract: template must exist, be spawn_only, and be
            // manually controlled (gravity 0) or parked instances accumulate
            // momentum offscreen and fly on first spawn.
            for pool in &scene.pools {
                match scene.objects.iter().find(|o| o.id == pool.template_object_id) {
                    None => errors.push(format!(
                        "Scene '{}' pool '{}' references missing template object '{}'",
                        scene.name, pool.pool_tag, pool.template_object_id
                    )),
                    Some(template) => {
                        if !template.spawn_only {
                            errors.push(format!(
                                "Scene '{}' pool '{}' template '{}' must be spawn_only (template factory required)",
                                scene.name, pool.pool_tag, pool.template_object_id
                            ));
                        }
                        if template.advanced.gravity != 0.0 {
                            warnings.push(format!(
                                "Scene '{}' pool '{}' template has gravity {} — pooled objects should be \
                                 manually controlled (gravity 0.0) or they accumulate momentum while parked",
                                scene.name, pool.pool_tag, template.advanced.gravity
                            ));
                        }
                    }
                }
            }

            // PathForgePlugin has no `new()`: it is set up by a PathForge
            // background, which knows the scene or journey it plays.
            if scene.required_plugins.iter().any(|r| r.type_name == "PathForgePlugin") {
                errors.push(format!(
                    "Scene '{}' lists PathForgePlugin in required_plugins; it is registered by a PathForge background \
                     (background.path_forge with a source), and the generated PathForgePlugin::new() does not exist.",
                    scene.name
                ));
            }
            if let Some(pf) = scene.background.path_forge_active() {
                if pf.source.trim().is_empty() {
                    errors.push(format!("Scene '{}' has a PathForge background with no source (a scene, .journey.json, export, or preset:<Name>).", scene.name));
                } else if pf.mode == crate::core::project::PathForgeMode::Frames && pf.source.trim().starts_with("preset:") {
                    errors.push(format!("Scene '{}': a PathForge background in Frames mode plays exported frames; a preset needs Live mode.", scene.name));
                } else if !pf.source.trim().starts_with("preset:") && !root.join(pf.source.trim()).exists() {
                    // The game reads it at run time; missing, the background stays empty.
                    errors.push(format!("Scene '{}': PathForge background source '{}' does not exist under the project root.", scene.name, pf.source.trim()));
                }
                // The frame is stretched onto the background object (the canvas).
                let canvas = scene.canvas.virtual_width.max(1.0) / scene.canvas.virtual_height.max(1.0);
                let frame = pf.size[0].max(1) as f32 / pf.size[1].max(1) as f32;
                if pf.mode == crate::core::project::PathForgeMode::Live && ((frame / canvas).ln()).abs() > 0.1 {
                    warnings.push(format!(
                        "Scene '{}': the PathForge background renders {}x{} but the canvas is {}x{}, so the frame is stretched; give it the canvas's shape.",
                        scene.name, pf.size[0], pf.size[1], scene.canvas.virtual_width, scene.canvas.virtual_height
                    ));
                }
            }

            // Plugin-dispatch safety: PluginCall/RunPlugin against a plugin
            // that is never registered compiles and silently no-ops at
            // runtime. Walk the whole scene (events, logic trees, nested
            // Multi/Conditional) via its JSON form so no nesting is missed.
            let dispatched = collect_plugin_dispatch_names(scene);
            for dispatch_name in dispatched {
                // The composited-background pipeline registers BackgroundPlugin
                // (dispatch name "background") itself in plugin-cache mode, so
                // RunPlugin("background", "set:..|transition:..") is covered.
                let background_registered = dispatch_name == "background"
                    && scene.background.layered_active()
                    && scene.background.use_plugin_cache;
                // A PathForge background registers PathForgePlugin ("path_forge").
                let path_forge_registered = dispatch_name == "path_forge"
                    && scene.background.path_forge_active().is_some();
                let registered = background_registered
                    || path_forge_registered
                    || scene.required_plugins.iter().any(|reg| {
                        crate::core::project::PluginRegistration::known_dispatch_name(&reg.type_name)
                            .map(|n| n == dispatch_name)
                            .unwrap_or(false)
                            || reg.type_name == dispatch_name
                    });
                if !registered {
                    let hint = if dispatch_name == "path_forge" {
                        " Give the scene a PathForge background (background.path_forge); that registers it.".to_owned()
                    } else {
                        crate::core::project::PluginRegistration::type_for_dispatch_name(&dispatch_name)
                            .map(|t| format!(" Add '{t}' to the scene's required_plugins."))
                            .unwrap_or_default()
                    };
                    errors.push(format!(
                        "Scene '{}' dispatches to plugin '{}' but never registers it — \
                         this compiles and silently does nothing at runtime.{}",
                        scene.name, dispatch_name, hint
                    ));
                }
            }
        }
    }

    let status = if errors.is_empty() {
        "ok"
    } else {
        "needs_attention"
    };

    Ok(json!({
        "workspace_root": root,
        "status": status,
        "errors": errors,
        "warnings": warnings,
        "notes": [
            "quartz_forge keeps app/core/services split",
            "dedicated MCP binary available as quartz_forge_mcp",
            "generated scene composition auto-emits #[path] mod plus use module::* for external component targets",
            "scene source files should use src/scenes/*_scene.rs",
            "constants/game_state custom code defaults are src/constants.rs and src/game_state.rs"
        ]
    }))
}

// ── Pipeline layer coverage (parity is a pipeline property) ──────────────────

struct ForgeLayerSources {
    codegen: String,
    import: String,
    ui: String,
}

fn forge_layer_sources(paths: &WorkspacePaths) -> Result<ForgeLayerSources> {
    let read = |rel: &str| -> String {
        fs::read_to_string(paths.root.join(rel)).unwrap_or_default()
    };
    Ok(ForgeLayerSources {
        codegen: format!(
            "{}\n{}",
            read("quartz_forge/src/services/codegen.rs"),
            read("quartz_forge/src/services/codegen_text.rs"),
        ),
        import: read("quartz_forge/src/services/project_import.rs"),
        ui: format!(
            "{}\n{}",
            read("quartz_forge/src/app/editors.rs"),
            read("quartz_forge/src/app/condition_editor.rs"),
        ),
    })
}

fn layer_missing(variants: &[String], haystack: &str, prefix: &str) -> Vec<String> {
    variants
        .iter()
        .filter(|v| !haystack.contains(&format!("{prefix}::{v}")))
        .cloned()
        .collect()
}

/// Live per-layer coverage scan. A variant "covered" in a layer means the
/// layer's source references it structurally; a variant missing from the
/// import layer still round-trips behaviorally (raw-blob fallback) but loses
/// visual editability in the UI.
fn pipeline_layer_coverage(
    paths: &WorkspacePaths,
    action_forge: &[String],
    condition_forge: &[String],
) -> Result<Value> {
    let sources = forge_layer_sources(paths)?;

    let layer = |variants: &[String], prefix: &str| -> Value {
        let codegen_missing = layer_missing(variants, &sources.codegen, prefix);
        let import_missing = layer_missing(variants, &sources.import, prefix);
        let ui_missing = layer_missing(variants, &sources.ui, prefix);
        json!({
            "total": variants.len(),
            "codegen_missing": codegen_missing,
            "import_missing": import_missing,
            "ui_missing": ui_missing,
        })
    };

    Ok(json!({
        "note": "Parity must hold at EVERY layer: domain enum, codegen emission, semantic import, UI editing. \
                 import_missing variants degrade to raw Expr blobs on import — behavior preserved, visual editing lost.",
        "action": layer(action_forge, "QuartzAction"),
        "condition": layer(condition_forge, "QuartzCondition"),
    }))
}

/// Count raw `Expr { raw }` action/condition blobs anywhere in a scene's JSON
/// form — the editability-loss metric surfaced by qf_project_sync_status.
fn count_raw_expr_blobs(value: &Value) -> usize {
    match value {
        Value::Object(map) => {
            let own = usize::from(
                map.len() == 1
                    && map.get("Expr")
                        .and_then(Value::as_object)
                        .is_some_and(|inner| inner.contains_key("raw")),
            );
            own + map.values().map(count_raw_expr_blobs).sum::<usize>()
        }
        Value::Array(items) => items.iter().map(count_raw_expr_blobs).sum(),
        _ => 0,
    }
}

/// Collect every plugin dispatch name (PluginCall/RunPlugin `name` fields)
/// used anywhere in a scene, at any nesting depth, by walking its JSON form.
fn collect_plugin_dispatch_names(scene: &crate::core::project::SceneDocument) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    if let Ok(value) = serde_json::to_value(scene) {
        walk_for_plugin_dispatch(&value, &mut names);
    }
    names.into_iter().collect()
}

fn walk_for_plugin_dispatch(value: &Value, names: &mut std::collections::BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                if key == "PluginCall" || key == "RunPlugin" {
                    if let Some(name) = inner.get("name").and_then(Value::as_str) {
                        if !name.trim().is_empty() {
                            names.insert(name.trim().to_owned());
                        }
                    }
                }
                walk_for_plugin_dispatch(inner, names);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_for_plugin_dispatch(item, names);
            }
        }
        _ => {}
    }
}

fn api_lookup(paths: &WorkspacePaths, query: &str, limit: usize) -> Result<Vec<Value>> {
    let query = query.trim().to_ascii_lowercase();
    let mut matches = Vec::new();
    let api_text = fs::read_to_string(&paths.quartz_api_txt)
        .with_context(|| format!("read {}", paths.quartz_api_txt.display()))?;

    for (line_no, line) in api_text.lines().enumerate() {
        if query.is_empty() || line.to_ascii_lowercase().contains(&query) {
            matches.push(json!({
                "source": paths.quartz_api_txt.display().to_string(),
                "line": line_no + 1,
                "text": line.trim()
            }));
        }
        if matches.len() >= limit {
            break;
        }
    }

    if matches.len() < limit {
        for path in [&paths.quartz_action_rs, &paths.quartz_condition_rs, &paths.forge_domain_rs] {
            let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
            for (line_no, line) in content.lines().enumerate() {
                if query.is_empty() || line.to_ascii_lowercase().contains(&query) {
                    matches.push(json!({
                        "source": path.display().to_string(),
                        "line": line_no + 1,
                        "text": line.trim()
                    }));
                }
                if matches.len() >= limit {
                    return Ok(matches);
                }
            }
        }
    }

    Ok(matches)
}

fn verify_snippet(paths: &WorkspacePaths, snippet: &str) -> Result<Value> {
    let action_text = fs::read_to_string(&paths.quartz_action_rs)?;
    let condition_text = fs::read_to_string(&paths.quartz_condition_rs)?;
    let api_text = fs::read_to_string(&paths.quartz_api_txt)?;

    let mut missing = Vec::new();
    let mut warnings = Vec::new();
    for needle in [
        "Action::",
        "Condition::",
        "Target::",
        "Location::",
        "Text::new",
        "Span::new",
        "SoundOptions::",
    ] {
        if snippet.contains(needle)
            && !action_text.contains(needle)
            && !condition_text.contains(needle)
            && !api_text.contains(needle)
        {
            missing.push(needle);
        }
    }

    if snippet.contains("canvas.make_text(") {
        warnings.push(
            "Quartz Forge text guidance prefers direct Text::new + Span::new construction; treat canvas.make_text(...) as a legacy convenience and build it before any get_game_object_mut() borrow.".to_owned()
        );
    }
    if snippet.contains("canvas.make_text(") && snippet.contains("get_game_object_mut(") {
        warnings.push(
            "make_text inside get_game_object_mut() risks the known double-borrow compile failure. Build the text value first, then mutate the object.".to_owned()
        );
    }
    if snippet.contains("Font::from_bytes(") && !snippet.contains("OnceLock") {
        warnings.push(
            "Repeated Font::from_bytes(...) parsing is slower than the preferred OnceLock<Font> cache used by ball_swing-style text helpers.".to_owned()
        );
    }
    if snippet.contains("Action::SetPosition") {
        warnings.push(
            "Action::SetPosition can zero movement semantics; prefer Teleport or ApplyMomentum/SetMomentum for movement intent.".to_owned(),
        );
    }

    let api_first_violations = codegen::api_first_static_guard_violations(snippet);

    let native_first = snippet.contains("quartz::prelude::*")
        || snippet.contains("Action::")
        || snippet.contains("Condition::")
        || snippet.contains("canvas.run(")
        || snippet.contains("Text::new")
        || snippet.contains("Span::new");

    Ok(json!({
        "native_first": native_first && api_first_violations.is_empty(),
        "api_first_guard_violations": api_first_violations,
        "missing_symbols": missing,
        "warnings": warnings,
        "line_count": snippet.lines().count(),
    }))
}

fn text_knowledge(paths: &WorkspacePaths) -> Result<Value> {
    let api_text = fs::read_to_string(&paths.quartz_api_txt)?;
    let forge_codegen = fs::read_to_string(paths.root.join("quartz_forge/src/services/codegen.rs"))?;

    Ok(json!({
        "intent": "Prefer Quartz-native text construction that matches the real ball_swing_game pattern and current quartz_forge SetText codegen.",
        "preferred_pattern": {
            "construction": "Text::new(vec![Span::new(...)], max_width, Align::{Left|Center}, None)",
            "span_fields": [
                "text string",
                "font size",
                "Some(font_size * 1.25) line height",
                "Arc<Font>",
                "Color",
                "letter spacing 0.0"
            ],
            "font_loading": "Cache Font::from_bytes(include_bytes!(...)) in OnceLock<Font>, then clone into Arc<Font> when building spans.",
            "set_text_usage": "Build the Text value first, then pass it to Action::SetText or obj.set_drawable(Box::new(text))."
        },
        "avoid": [
            "Do not rely on canvas.make_text(...) as the primary generated pattern when Text::new + Span::new is available.",
            "Do not call make_text inside get_game_object_mut(); pre-build the text first to avoid double-borrow compile failures.",
            "Do not re-parse the same font bytes every frame when a OnceLock cache can hold the Font."
        ],
        "ball_swing_references": [
            "ball_swing_game/src/objects/ui.rs",
            "ball_swing_game/src/scenes/game/build_scene.rs"
        ],
        "quartz_forge_references": [
            "quartz_forge/src/services/codegen.rs",
            "quartz_forge/src/app/mod.rs",
            "quartz_forge/src/app/editors.rs"
        ],
        "source_signals": {
            "api_has_text_new": api_text.contains("Text::new"),
            "api_has_span_new": api_text.contains("Span::new"),
            "forge_codegen_uses_direct_text_new": forge_codegen.contains("set_text_value_expr") && forge_codegen.contains("Text::new") && forge_codegen.contains("Span::new")
        },
        "example_set_text": "static HUD_FONT: std::sync::OnceLock<Font> = std::sync::OnceLock::new(); let font = std::sync::Arc::new(HUD_FONT.get_or_init(|| Font::from_bytes(include_bytes!(\"../../assets/font.ttf\")).expect(\"hud font\")).clone()); let text = Text::new(vec![Span::new(\"Ready\".to_owned(), 30.0, Some(37.5), font, Color(255, 255, 255, 255), 0.0)], None, Align::Left, None); canvas.run(Action::set_text(Target::name(\"hud_label\"), text));"
    }))
}

fn codegen_api_guidance(paths: &WorkspacePaths) -> Value {
    // Live import-coverage: computed from the import parser source at call
    // time so this guidance can never drift from reality. Variants in the
    // blob-degrading list still round-trip behaviorally but import as raw
    // Expr blobs (no structured UI editing) — prefer covered variants.
    let import_coverage = enum_variants(&paths.forge_domain_rs, "QuartzAction")
        .ok()
        .and_then(|actions| {
            let conditions = enum_variants(&paths.forge_domain_rs, "QuartzCondition").ok()?;
            let sources = forge_layer_sources(paths).ok()?;
            let blob_actions = layer_missing(&actions, &sources.import, "QuartzAction");
            let blob_conditions = layer_missing(&conditions, &sources.import, "QuartzCondition");
            Some(json!({
                "note": "Computed live from project_import.rs. Actions/conditions listed under blob_degrading \
                         still round-trip behaviorally but arrive as raw Expr blobs in the editor (no structured \
                         visual editing). Everything else imports fully structured.",
                "action_total": actions.len(),
                "blob_degrading_actions": blob_actions,
                "condition_total": conditions.len(),
                "blob_degrading_conditions": blob_conditions,
                "normalized_on_import": ["CollisionWith -> Collision", "VarCompare -> Compare"],
            }))
        })
        .unwrap_or_else(|| json!({"note": "coverage scan unavailable"}));

    json!({
        "mandate": "Exhaust native Quartz API before writing custom Rust. This is a HARD RULE enforced by quartz_forge and copilot-instructions.md §5b.",
        "import_coverage": import_coverage,
        "plugin_registration_rule": "Plugins do NOT auto-register. Any scene using Action::PluginCall or Action::RunPlugin MUST emit canvas.add_plugin(<Plugin>::new(...)) at the top of setup_scene (imported into SceneDocument.required_plugins). qf_project_lint_layout reports dispatch-without-registration as an error.",
        "pool_contract": "Pools: emit canvas.create_pool(\"tag\", spawn_<template_id>(canvas), count) at the END of setup_scene with a spawn_only template object (imports into SceneDocument.pools). Template MUST be manually controlled (gravity 0.0) or parked instances accumulate momentum offscreen and fly on first spawn. pool_acquire resets ONLY position + momentum — reset rotation/color/scale/animation in the spawner. Release with canvas.pool_release(name).",
        "gui_codegen_shape_contract": {
            "intent": "AI-generated game source should mirror quartz_forge GUI export shape so semantic import can recover structured entities reliably.",
            "required_functions": [
                "pub fn setup_scene(canvas: &mut Canvas)",
                "pub fn register_logic(canvas: &mut Canvas)",
                "pub fn register_events(canvas: &mut Canvas)"
            ],
            "preferred_file_layout": [
                "Scene entry: src/scenes/*_scene.rs",
                "Object/event/update component files: src/scripts/*.rs",
                "Constants: src/constants.rs",
                "Game vars: src/game_state.rs"
            ],
            "authoring_rules": [
                "Use canvas.add_event(GameEvent::...) for event entities instead of ad-hoc dispatch in on_update.",
                "Use canvas.on_update(|canvas| { canvas.run(Action::...) ... }) for update loop entities.",
                "Use canvas.register_custom_event(name, |canvas| {...}) for custom-event entities.",
                "Define GameObject::build chains directly in setup_scene or spawn_* helpers so object importer can recover object blueprints.",
                "Keep scalar game state in canvas.set_var/get_var with Action::SetVar/ModVar for importer-visible game vars.",
                "For setup_scene generation, use three phases: build local objects first, emit setup_runtime/game_state blocks while locals are alive, then call canvas.add_game_object for each local.",
                "Never emit local-object mutations after add_game_object has moved the local; this causes E0382 borrow-of-moved errors in generated Rust.",
                "Preserve setup_runtime statements that target builder locals by rewriting local variable names to object ids token-aware (identifier boundaries), not substring matching."
            ]
        },
        "dispatch_priority_order": [
            {
                "step": 1,
                "api": "canvas.add_event(GameEvent::KeyHold/KeyPress/Collision/BoundaryCollision/Tick, target, action)",
                "use_when": "Wiring any input, collision, tick, or boundary event to an object. Always the first choice."
            },
            {
                "step": 2,
                "api": "Action::Conditional { condition: Condition, if_true: Box<Action>, if_false: Option<Box<Action>> }",
                "use_when": "ALL runtime branching. Never write `if canvas.get_bool(...)` in on_update when Action::Conditional can handle it."
            },
            {
                "step": 3,
                "api": "Action::Multi { vec![action1, action2, ...] }",
                "use_when": "Multiple actions that must fire from a single event or condition branch."
            },
            {
                "step": 4,
                "api": "Action::SetVar { name, value: Expr } / Action::ModVar { name, op: MathOp, operand: Expr } + canvas.set_var/get_var/get_f32/get_bool",
                "use_when": "All scalar game state (score, lives, speed, timers, flags). Lives in canvas.game_vars, NOT a custom Rust struct or Arc<Mutex<T>>."
            },
            {
                "step": 5,
                "api": "Expr::var/add/sub/mul/div/f32/i32/bool + Condition::Compare/SpeedAbove/Grounded/HasTag/KeyHeld/Collision",
                "use_when": "Computed conditions and expressions in Action::Conditional and Action::SetVar without dropping into custom Rust."
            },
            {
                "step": 6,
                "api": "canvas.on_update(|cv| { ... })",
                "use_when": "ONLY for per-frame logic that genuinely cannot be expressed with events — e.g. random spawning with Entropy, reading positions to drive visual state, multi-object query patterns."
            },
            {
                "step": 7,
                "api": "canvas.register_custom_event(name, handler)",
                "use_when": "ONLY for named triggers required by scene wiring or multi-system coordination. Not a substitute for Action::Conditional."
            }
        ],
        "hard_violations": [
            "Arc<Mutex<State>> for scalars that fit in game_vars — always use game_vars instead",
            "if/match inside on_update to branch what GameEvent + Action::Conditional can express",
            "Direct plugin method calls in on_update — use Action::PluginCall",
            "Action::SetPosition for movement — zeroes momentum; use Action::ApplyMomentum, Action::SetMomentum, or Action::Teleport",
            "collision_layer(0) — silently disables collision; use named non-zero layer constants",
            "Entropy::range(int, int) — must be f32: Entropy::range(0.0, 10.0)",
            "Emit setup_runtime local mutations after add_game_object moves object locals — causes E0382 borrow/assign-after-move compile failures",
            "Detect unresolved local references using substring checks (for example overlay in game_over_overlay) — causes false drops of valid setup_runtime statements"
        ],
        "quick_patterns": {
            "score_increment": "Action::ModVar { name: 'score'.into(), op: MathOp::Add, operand: Expr::i32(1) }",
            "lives_decrement": "Action::ModVar { name: 'lives'.into(), op: MathOp::Sub, operand: Expr::i32(1) }",
            "game_over_check": "Action::Conditional { condition: Expr::var('lives').lte(Expr::i32(0)), if_true: Box::new(Action::Custom { name: 'game_over'.into() }), if_false: None }",
            "thrust_left": "canvas.add_event(GameEvent::KeyHold { key: Key::Character('a'), action: Action::ApplyMomentum { target: Target::name('player'), value: (-THRUST, 0.0) }, target: Target::name('player'), modifiers: None }, Target::name('player'))",
            "on_collision_with_enemy": "canvas.add_event(GameEvent::Collision { action: Action::Multi { vec![Action::ModVar { .. lives -1 }, Action::CameraShake { .. }] }, target: Target::tag('enemy') }, Target::name('player'))"
        },
        "synful_lighting": {
            "note": "SYNFUL-ONLY API. This engine fork adds a `quartz::lighting` module (LightSource/LightType/LightEffect/LightingConfig/AmbientLight, all in quartz::prelude) plus GPU lighting and post-processing on Canvas. Official quartz has NONE of this — do not emit it against a non-synful project. Authored in the forge via the Lighting & Post-FX window (SceneDocument.lighting + .post_fx).",
            "enable_lighting": "canvas.enable_lighting(LightingConfig { ambient: AmbientLight { color: Color(10,10,25,255), strength: 0.06 }, max_lights: 64 }); — ambient at full strength (1.0) hides all lights. Presets: LightingConfig::night()/indoor()/day(), AmbientLight::dark()/dim()/bright().",
            "add_light": "canvas.add_light(LightSource::new(\"torch\", (x, y), Color(255,180,80,255), radius, intensity)); chain .with_shadows(false) and .with_effect(LightEffect::Flicker{..}). LightSource::new defaults casts_shadows=true. light_type and enabled are plain fields (no builder) — set them on a `let mut __light = ...; __light.light_type = LightType::Spot{..};` local because struct-update syntax fails (private effect_time field, E0451). Presets: LightSource::torch/campfire/moonlight/neon/lantern/spotlight/sun.",
            "attach_light": "canvas.attach_light(\"light_id\", \"object_name\", (offset_x, offset_y)); — light follows the object every frame. Emit AFTER the object exists in the canvas.",
            "post_fx": "Bloom is a separate pass: canvas.enable_bloom(BloomSettings { threshold, strength }) (BloomSettings is at the crate root: `use quartz::BloomSettings;`, NOT in the prelude). One active post override at a time: canvas.enable_vignette(s,r,soft) | enable_chromatic_aberration(px) | enable_night_mode_shader(bt,bs,vs,vr,vsoft,ca) | register_shader_source(id,label,wgsl)+set_post_override(id, vec![params]).",
            "object_flags": "obj.unlit and obj.shadow_caster are public FIELDS on GameObject (obj.unlit = true;), NOT builder methods — there is no .unlit(). (`.casts_shadow()` builder exists but the forge emits the field for uniformity.) Mark large backgrounds/terrain unlit: they are tinted uniformly from center so per-position lighting looks wrong.",
            "hard_rules": [
                "Never emit lighting/post-fx against official (non-synful) quartz — the module does not exist there.",
                "Ambient strength 1.0 = fully lit = individual lights invisible. Use ~0.06 (dark) to make lights read.",
                "BloomSettings needs `use quartz::BloomSettings;` (crate root); the LightSource/LightType/etc types are in the prelude.",
                "enable_air_barrier is a per-frame gameplay effect (11 runtime args: time, player speed, player UV, facing) — author it in custom code, not scene setup."
            ]
        }
    })
}

fn project_roundtrip_contract(paths: &WorkspacePaths) -> Result<Value> {
    let project_text = fs::read_to_string(paths.root.join("quartz_forge/src/core/project.rs"))?;
    let persistence_text = fs::read_to_string(paths.root.join("quartz_forge/src/services/persistence.rs"))?;
    let app_text = fs::read_to_string(paths.root.join("quartz_forge/src/app/mod.rs"))?;

    Ok(json!({
        "intent": "Create a quartz_forge-native Quartz project that still round-trips through the editor, not a Rust-only crate that quartz_forge can no longer understand.",
        "editor_source_of_truth": {
            "manifest_file": "project.qforge.json",
            "scenes_live_in_manifest": project_text.contains("pub scenes: Vec<SceneDocument>"),
            "objects_live_in_scene_documents": project_text.contains("pub objects: Vec<QuartzObjectBlueprint>"),
            "logic_live_in_scene_documents": project_text.contains("pub logic_trees: Vec<LogicTree>"),
            "events_live_in_scene_documents": project_text.contains("pub events: Vec<QuartzEventBinding>"),
            "custom_code_blocks_live_in_scene_documents": project_text.contains("pub custom_code_blocks: Vec<CustomCodeBlock>")
        },
        "runtime_scaffold": {
            "cargo_toml_managed": persistence_text.contains("fn ensure_cargo_toml"),
            "main_rs_contains_ramp_run": persistence_text.contains("ramp::run!"),
            "lib_rs_contains_build_app": persistence_text.contains("pub fn build_app(ctx: &mut Context) -> impl Drawable"),
            "lib_rs_tracks_scene_module_path": persistence_text.contains("mod generated_scene;"),
            "lib_rs_tracks_canvas_mode": persistence_text.contains("CanvasMode::Landscape") && persistence_text.contains("CanvasMode::Portrait"),
            "managed_entrypoint_markers_present": persistence_text.contains("quartz_forge-managed: main entrypoint") && persistence_text.contains("quartz_forge-managed: build_app scaffold")
        },
        "roundtrip_rules": [
            "Update project.qforge.json alongside generated Rust or quartz_forge will not reflect the change in the editor.",
            "Scene source_file paths should live under src/scenes and follow *_scene.rs naming to match quartz_project_layout conventions.",
            "Prefer scene source_file and per-surface output_file routing instead of inventing ad-hoc module layouts outside the manifest.",
            "AI codegen should mirror quartz_forge GUI export shape (setup_scene/register_logic/register_events + component routing) for reliable semantic import.",
            "For static images, prefer canvas.load_image_cached/load_image_sized_cached over repeated quartz::sprite::load_image calls when reuse is expected.",
            "Use custom code blocks or ManualFileOverride for user-owned Rust sections that must survive regeneration.",
            "Keep generated scene/component module paths relative to the scene source file layout, not hard-coded to the project root.",
            "When exporting setup_scene, generate in three phases: build object locals -> emit setup_runtime/game_state -> move locals into canvas with add_game_object.",
            "Treat no_collision objects as collision_layer(0) + collision_mask(0) + non_platform so HUD/overlay/background objects do not become accidental colliders.",
            "setup_runtime preservation must rewrite builder-local references token-aware to object ids; do not use naive substring matching."
        ],
        "ai_generation_rules": {
            "api_first_mandate": "Exhaust native Quartz API before writing custom Rust. This is a HARD RULE — treat violations as blocking.",
            "dispatch_priority_order": [
                "1. canvas.add_event(GameEvent::KeyHold/KeyPress/Collision/BoundaryCollision/Tick, target, action) — wire all input and world events declaratively",
                "2. Action::Conditional { condition, if_true, if_false } — ALL branching; never write if/match in on_update when Action::Conditional can handle it",
                "3. Action::Multi { vec![...] } — batch multiple actions from a single event trigger",
                "4. Action::SetVar / Action::ModVar + canvas.set_var/get_var/get_f32/get_bool — ALL scalar game state lives in game_vars, not a custom Rust struct",
                "5. Expr::var/add/sub/mul/div + Condition::Compare/SpeedAbove/Grounded/HasTag — computed guards without custom Rust",
                "6. canvas.on_update(|cv| { ... }) — ONLY for per-frame logic that cannot be expressed with events (random spawning, positional reads that drive visual state)",
                "7. canvas.register_custom_event — ONLY for named triggers required by scene wiring; not a substitute for Action::Conditional"
            ],
            "violations_to_reject": [
                "Arc<Mutex<State>> for scalars that fit in game_vars — use game_vars instead",
                "if/match in on_update to dispatch what GameEvent variants can handle — use canvas.add_event",
                "if canvas.get_bool(...) to branch what Action::Conditional can handle — use Action::Conditional",
                "Direct plugin method calls in on_update — use Action::PluginCall dispatch",
                "Action::SetPosition for movement — zeroes momentum; use Teleport or ApplyMomentum/SetMomentum",
                "Mutating local object variables after add_game_object has moved them into canvas",
                "Dropping valid setup_runtime statements because local-name detection is substring-based instead of identifier-boundary-based"
            ]
        },
        "editor_surfaces_agents_should_respect": {
            "scene_source_file_is_user_editable": app_text.contains("Scene File (relative)") && app_text.contains("source_file_picker"),
            "component_output_file_routing_present": app_text.contains("component_target_path") && project_text.contains("output_file"),
            "manual_file_override_supported": project_text.contains("ManualFileOverride")
        },
        "current_limitations": [
            "The current runtime scaffold wraps the active scene module, not a full multi-scene Scene::new/add_scene/load_scene app like ball_swing_game.",
            "Editing Rust without updating the manifest breaks round-tripping back into quartz_forge's scene/object/event editors.",
            "Plugin registration and richer runtime bootstrap logic still need explicit code or future MCP/tooling support."
        ],
        "relevant_files": [
            "quartz_forge/src/core/project.rs",
            "quartz_forge/src/services/persistence.rs",
            "quartz_forge/src/app/mod.rs",
            "ball_swing_game/src/lib.rs"
        ]
    }))
}

fn path_forge_background_contract(paths: &WorkspacePaths) -> Value {
    let crate_dir = paths.root.join("quartz_path_forge");
    json!({
        "plugin_crate": {
            "path": crate_dir.display().to_string(),
            "installed": crate_dir.join("src/lib.rs").exists(),
            "readme": crate_dir.join("README.md").display().to_string(),
            "type": "quartz_path_forge::PathForgePlugin",
            "dispatch_name": "path_forge",
        },
        "manifest_field": {
            "where": "scenes[i].background (enabled: true) .path_forge",
            "shape": {"source": "string: relative to the project root; a PathForge scene .json or .journey.json (Live), what `pf journey --formats png` wrote (Frames), or preset:<Name> (Live)",
                      "mode": "Live | Frames", "size": "[w, h] pixels rendered (Live); give it the canvas's shape", "speed": "m/s or omitted (each scene's own)", "render_fps": "Live, default 30"},
            "example": {"enabled": true, "object_id": "background", "render_layer": -100, "camera_pinned": true,
                        "path_forge": {"source": "assets/backgrounds/game.journey.json", "mode": "Live", "size": [270, 480], "render_fps": 30.0}},
            "note": "When path_forge is set the layers are ignored. Do NOT add PathForgePlugin to required_plugins; the background registers it.",
        },
        "generated_code": [
            "use quartz_path_forge::PathForgePlugin;",
            "let mut background = GameObject::build(\"background\").size(W, H).position(0.0, 0.0).layer(-100).screen_space().finish();",
            "background.unlit = true;   // the frame carries its own lighting",
            "let __background_path_forge = PathForgePlugin::live(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/assets/backgrounds/game.journey.json\"), \"background\", (270u32, 480u32)).map(|p| p.with_render_fps(30.0));",
            "match __background_path_forge { Ok(p) => canvas.add_plugin(p), Err(e) => eprintln!(..) }",
        ],
        "cargo_toml": "Saving the project adds quartz_path_forge = { path = \"../quartz_path_forge\" } and opt-level 3 for path_forge/image/png in dev builds (the project must sit in synful_quartz/, like ../quartz).",
        "actions": {
            "form": "QuartzAction::RunPlugin { name: \"path_forge\", data }",
            "data": {"next": "go on to where the journey leads (a transition, or a fork)", "choose:left": "take the left branch of the fork ahead", "choose:right": "take the right branch",
                     "stop": "stand still (flames keep moving)", "walk": "walk on", "speed:3.5": "walking speed m/s", "finish": "cut to the end of the walk under way",
                     "transition:<file>[|json]": "Live: walk into another scene", "fork:<left>|<right>[|json]": "Live: fork into two scenes", "scene:<file>": "Live: cut", "journey:<file>": "Live: load a journey"},
        },
        "conditions": {
            "form": "QuartzCondition::Plugin { name: \"path_forge\", arg }",
            "args": {"walking": "walking", "walking_on": "a transition or fork is under way", "fork_ahead": "a fork is coming and nothing is chosen yet (ask the player now)",
                     "arrived": "true for one tick after reaching a new stop", "ended": "at a stop that leads nowhere", "at:<stop>": "at that journey stop", "scene:<name>": "in that scene"},
        },
        "lint": [
            "RunPlugin \"path_forge\" counts as registered only when the scene has a PathForge background.",
            "Errors: empty source; Frames mode with preset:; a source file missing under the project root; PathForgePlugin in required_plugins.",
            "Warning: a Live render size not in the canvas's shape (the frame is stretched).",
        ],
        "workflow": [
            "Design the places with PathForge's MCP (pf_new_scene / pf_edit_scene / pf_contact_sheet), the walks between them with pf_transition, and the whole game's map with pf_journey (op write) into the project's assets/backgrounds/.",
            "Live: point background.path_forge.source at the .journey.json. Frames: pf_journey op export with formats [\"png\"] into assets/, and point the source at <name>.journey.json there.",
            "qf_project_apply_state with the background and events (e.g. KeyPress or a gameplay event -> RunPlugin path_forge next; fork_ahead -> show a choice; choose:left/right).",
            "qf_project_lint_layout, then build the game.",
        ],
    })
}

fn background_plugin_contract(paths: &WorkspacePaths) -> Result<Value> {
    let plugin_mod = paths
        .root
        .join("quartz/src/plugin/background/mod.rs");
    let plugin_readme = paths
        .root
        .join("quartz/src/plugin/background/README.md");
    let quartz_lib = paths.root.join("quartz/src/lib.rs");

    let mod_text = if plugin_mod.exists() {
        Some(fs::read_to_string(&plugin_mod).with_context(|| format!("read {}", plugin_mod.display()))?)
    } else {
        None
    };
    let readme_text = if plugin_readme.exists() {
        Some(fs::read_to_string(&plugin_readme).with_context(|| format!("read {}", plugin_readme.display()))?)
    } else {
        None
    };
    let lib_text = if quartz_lib.exists() {
        Some(fs::read_to_string(&quartz_lib).with_context(|| format!("read {}", quartz_lib.display()))?)
    } else {
        None
    };

    let installed = mod_text.is_some();
    let mod_text_ref = mod_text.as_deref().unwrap_or("");
    let readme_ref = readme_text.as_deref().unwrap_or("");
    let lib_ref = lib_text.as_deref().unwrap_or("");

    Ok(json!({
        "installed": installed,
        "source_files": {
            "mod_rs": plugin_mod,
            "readme_md": plugin_readme,
            "quartz_lib_rs": quartz_lib
        },
        "api_presence": {
            "background_layer_enum": mod_text_ref.contains("pub enum BackgroundLayer"),
            "layered_background_builder": mod_text_ref.contains("pub struct LayeredBackground") && mod_text_ref.contains("with_layer") && mod_text_ref.contains("build(self"),
            "background_plugin": mod_text_ref.contains("pub struct BackgroundPlugin") && mod_text_ref.contains("pub fn set_background"),
            "plugin_action_set": mod_text_ref.contains("strip_prefix(\"set:\")"),
            "plugin_action_transition": mod_text_ref.contains("strip_prefix(\"transition:\")"),
            "disk_cache_path": mod_text_ref.contains("load_or_build_cached") || readme_ref.contains("disk cache"),
            "feature_gate_signal": lib_ref.contains("plugin_background")
        },
        "supported_layers_from_source": [
            "Solid",
            "GradientVertical",
            "GradientHorizontal",
            "GradientFourCorner",
            "Starfield",
            "Nebula",
            "Image",
            "Raw"
        ],
        "window_blueprint": {
            "goal": "author plugin-backed layered backgrounds in Quartz Forge without hand-writing plugin glue code",
            "minimum_controls": [
                "background key",
                "canvas width/height",
                "layer stack editor (ordered)",
                "per-layer parameter editors by variant",
                "cache dir toggle/path",
                "transition authoring (from,to,duration)",
                "preview + generated snippet"
            ],
            "ai_generation_rules": [
                "Prefer LayeredBackground::new().with_layer(...) chains over custom pixel loops.",
                "Use BackgroundPlugin::set_background for registration; use Action::run_plugin(\"background\", \"set:key\") for runtime switching.",
                "Use transition payload format 'transition:from,to,duration_secs' when crossfading.",
                "Gate generated background plugin code with #[cfg(plugin_background)] to avoid compile breaks when plugin is absent."
            ],
            "roundtrip_storage_recommendation": [
                "Store designer state in manifest JSON (background docs + layer definitions).",
                "Generate plugin glue into TopLevel custom code blocks so semantic/manual import can preserve user edits.",
                "Treat unresolved custom layer expressions as ManualFileOverride fallback, not destructive rewrite."
            ]
        },
        "limitations_and_risks": [
            "BackgroundLayer::Image currently expects static bytes in plugin source API; dynamic runtime file pickers should emit include_bytes-backed paths in generated code.",
            "Raw layer is not disk-cache eligible; generated workflows should prefer deterministic layer variants for stable rebuilds.",
            "README naming may drift from mod.rs method names; prefer mod.rs signatures as source of truth."
        ]
    }))
}

fn project_state_dump(paths: &WorkspacePaths, project_root: &str) -> Result<Value> {
    let root = resolve_project_root(paths, project_root);
    let (state, report) = persistence::load_project_with_sync(&root)
        .with_context(|| format!("failed to load quartz_forge project at {}", root.display()))?;

    Ok(json!({
        "project_root": root,
        "project_name": state.manifest.project_name,
        "active_scene_index": state.active_scene_index,
        "manifest": state.manifest,
        "sync_report": sync_report_json(&report),
    }))
}

fn project_create(
    paths: &WorkspacePaths,
    project_root: &str,
    project_name: &str,
    write_generated_files: bool,
) -> Result<Value> {
    let root = resolve_project_root(paths, project_root);
    let state = persistence::create_new_project(project_name.to_owned(), &root)
        .with_context(|| format!("failed to create quartz_forge project at {}", root.display()))?;

    if write_generated_files {
        project_sync::write_all_generated_files_from_state(&state, &root)?;
        persistence::write_sync_snapshot(&state, &root)?;
    }

    let report = persistence::validate_project_sync(&state, &root)?;
    Ok(json!({
        "project_root": root,
        "project_name": state.manifest.project_name,
        "write_generated_files": write_generated_files,
        "sync_report": sync_report_json(&report),
    }))
}

fn project_apply_state(
    paths: &WorkspacePaths,
    project_root: &str,
    manifest_value: Value,
    write_generated_files: bool,
    write_sync_snapshot: bool,
) -> Result<Value> {
    let root = resolve_project_root(paths, project_root);
    let mut manifest: ProjectManifest = serde_json::from_value(manifest_value)
        .context("failed to deserialize ProjectManifest from arguments.manifest")?;
    manifest.ensure_default_scene();
    let active_scene_index = manifest.active_scene_index().unwrap_or(0);
    let mut state = EditorProjectState {
        manifest,
        active_scene_index,
        dirty: false,
    };

    persistence::save_project(&mut state, &root)
        .with_context(|| format!("failed to save quartz_forge project at {}", root.display()))?;
    if write_generated_files {
        project_sync::write_all_generated_files_from_state(&state, &root).with_context(|| {
            format!("failed to rewrite generated project files under {}", root.display())
        })?;
    }
    if write_sync_snapshot {
        persistence::write_sync_snapshot(&state, &root)
            .with_context(|| format!("failed to write sync snapshot for {}", root.display()))?;
    }

    let report = persistence::validate_project_sync(&state, &root)?;
    Ok(json!({
        "project_root": root,
        "project_name": state.manifest.project_name,
        "write_generated_files": write_generated_files,
        "write_sync_snapshot": write_sync_snapshot,
        "sync_report": sync_report_json(&report),
    }))
}

fn project_import_manual_overrides(
    paths: &WorkspacePaths,
    project_root: &str,
    files: &[String],
) -> Result<Value> {
    let root = resolve_project_root(paths, project_root);
    let (mut state, _report) = persistence::load_project_with_sync(&root)
        .with_context(|| format!("failed to load quartz_forge project at {}", root.display()))?;

    let mut imported = Vec::new();
    let mut failed = Vec::new();
    for file in files {
        let Some(rel_path) = normalize_project_rel_path(&root, file) else {
            failed.push(format!("{file} (path is not inside project root)"));
            continue;
        };
        let path = root.join(&rel_path);
        match fs::read_to_string(&path) {
            Ok(content) => {
                if state.track_manual_override_for_file(&rel_path, &content).is_some() {
                    imported.push(rel_path);
                } else {
                    failed.push(format!("{file} (no owning scene could be resolved)"));
                }
            }
            Err(err) => failed.push(format!("{file} ({err})")),
        }
    }

    if !imported.is_empty() {
        persistence::save_project(&mut state, &root)?;
        persistence::write_sync_snapshot(&state, &root)?;
    }
    let report = persistence::validate_project_sync(&state, &root)?;

    Ok(json!({
        "project_root": root,
        "imported_files": imported,
        "failed_files": failed,
        "sync_report": sync_report_json(&report),
    }))
}

fn project_import_semantic(
    paths: &WorkspacePaths,
    project_root: &str,
    files: &[String],
    fallback_manual_overrides: bool,
) -> Result<Value> {
    let root = resolve_project_root(paths, project_root);
    let (mut state, _report) = persistence::load_project_with_sync(&root)
        .with_context(|| format!("failed to load quartz_forge project at {}", root.display()))?;

    let import_report = project_import::import_files_into_state(
        &mut state,
        &root,
        files,
        fallback_manual_overrides,
    )?;

    persistence::save_project(&mut state, &root)?;
    persistence::write_sync_snapshot(&state, &root)?;
    let sync_report = persistence::validate_project_sync(&state, &root)?;

    Ok(json!({
        "project_root": root,
        "imported_files": import_report.imported_files,
        "imported_object_count": import_report.imported_object_count,
        "imported_logic_tree_count": import_report.imported_logic_tree_count,
        "imported_event_count": import_report.imported_event_count,
        "imported_custom_block_count": import_report.imported_custom_block_count,
        "fallback_manual_override_files": import_report.fallback_manual_override_files,
        "unsupported_files": import_report.unsupported_files,
        "notes": import_report.notes,
        "sync_report": sync_report_json(&sync_report),
    }))
}

fn project_sync_status(paths: &WorkspacePaths, project_root: &str) -> Result<Value> {
    let root = resolve_project_root(paths, project_root);
    let (state, report) = persistence::load_project_with_sync(&root)
        .with_context(|| format!("failed to load quartz_forge project at {}", root.display()))?;

    // Editability health: how much of this project is opaque to the visual
    // editor. Raw Expr blobs and ManualFileOverride blocks round-trip
    // behaviorally but cannot be edited structurally in the UI.
    let mut raw_blob_count = 0usize;
    let mut manual_override_files = Vec::<String>::new();
    for scene in &state.manifest.scenes {
        if let Ok(value) = serde_json::to_value(scene) {
            raw_blob_count += count_raw_expr_blobs(&value);
        }
        for block in &scene.custom_code_blocks {
            if block.kind == crate::core::quartz_domain::CustomCodeKind::ManualFileOverride {
                manual_override_files.push(block.output_file.clone());
            }
        }
    }

    Ok(json!({
        "project_root": root,
        "project_name": state.manifest.project_name,
        "active_scene_id": state.manifest.active_scene_id,
        "scene_count": state.manifest.scenes.len(),
        "editability": {
            "raw_expr_blob_count": raw_blob_count,
            "manual_override_files": manual_override_files,
            "note": "Blobs and overrides preserve behavior but are invisible to structured editing. \
                     Rising counts mean AI-generated code is drifting outside the importable surface — \
                     check qf_codegen_api_guidance import_coverage.",
        },
        "status": match report.status {
            persistence::ProjectSyncStatus::MissingSnapshot => "missing_snapshot",
            persistence::ProjectSyncStatus::InSync => "in_sync",
            persistence::ProjectSyncStatus::SavedProjectAheadOfFiles => "saved_project_ahead_of_files",
            persistence::ProjectSyncStatus::FilesChangedOutsideQuartzForge => "files_changed_outside_quartz_forge",
            persistence::ProjectSyncStatus::Diverged => "diverged",
        },
        "summary": report.summary,
        "modified_files": report.modified_files,
        "missing_files": report.missing_files,
        "extra_files": report.extra_files,
        "can_restore_project_from_last_export": report.can_restore_project_from_last_export,
        "can_rewrite_files_from_project": report.can_rewrite_files_from_project,
        "snapshot_generated_at_utc": report.snapshot_generated_at_utc,
    }))
}

fn resolve_project_root(paths: &WorkspacePaths, project_root: &str) -> PathBuf {
    let candidate = PathBuf::from(project_root);
    if candidate.is_absolute() {
        candidate
    } else {
        paths.root.join(candidate)
    }
}

fn normalize_project_rel_path(root: &Path, file: &str) -> Option<String> {
    let candidate = PathBuf::from(file);
    let path = if candidate.is_absolute() {
        candidate
    } else {
        root.join(candidate)
    };

    path.strip_prefix(root)
        .ok()
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
}

fn sync_report_json(report: &persistence::ProjectSyncReport) -> Value {
    json!({
        "status": match report.status {
            persistence::ProjectSyncStatus::MissingSnapshot => "missing_snapshot",
            persistence::ProjectSyncStatus::InSync => "in_sync",
            persistence::ProjectSyncStatus::SavedProjectAheadOfFiles => "saved_project_ahead_of_files",
            persistence::ProjectSyncStatus::FilesChangedOutsideQuartzForge => "files_changed_outside_quartz_forge",
            persistence::ProjectSyncStatus::Diverged => "diverged",
        },
        "summary": report.summary,
        "modified_files": report.modified_files,
        "missing_files": report.missing_files,
        "extra_files": report.extra_files,
        "can_restore_project_from_last_export": report.can_restore_project_from_last_export,
        "can_rewrite_files_from_project": report.can_rewrite_files_from_project,
        "snapshot_generated_at_utc": report.snapshot_generated_at_utc,
    })
}

fn parity_report(paths: &WorkspacePaths, surface: &str) -> Result<Value> {
    if !matches!(surface, "action" | "condition" | "wiring" | "all") {
        anyhow::bail!(
            "qf_forge_check_parity invalid arguments.surface '{}' expected one of: action|condition|wiring|all",
            surface
        );
    }

    let action_quartz = enum_variants(&paths.quartz_action_rs, "Action")?;
    let action_forge = enum_variants(&paths.forge_domain_rs, "QuartzAction")?;
    let condition_quartz = enum_variants(&paths.quartz_condition_rs, "Condition")?;
    let condition_forge = enum_variants(&paths.forge_domain_rs, "QuartzCondition")?;

    let action_missing: Vec<_> = action_quartz
        .iter()
        .filter(|v| !action_forge.contains(v))
        .cloned()
        .collect();
    let condition_missing: Vec<_> = condition_quartz
        .iter()
        .filter(|v| !condition_forge.contains(v))
        .cloned()
        .collect();

    // Parity is a PIPELINE property, not an enum property. The domain enum
    // having a variant means nothing if codegen cannot emit it, import cannot
    // re-parse it (visual editability degrades to a raw blob), or the UI
    // cannot edit it. Measure every layer from source, live.
    let pipeline = pipeline_layer_coverage(paths, &action_forge, &condition_forge)?;

    let mut report = serde_json::Map::new();
    if surface == "action" || surface == "all" {
        report.insert(
            "action".to_owned(),
            json!({
                "quartz_count": action_quartz.len(),
                "forge_count": action_forge.len(),
                "missing_in_forge": action_missing,
            }),
        );
    }
    if surface == "condition" || surface == "all" {
        report.insert(
            "condition".to_owned(),
            json!({
                "quartz_count": condition_quartz.len(),
                "forge_count": condition_forge.len(),
                "missing_in_forge": condition_missing,
            }),
        );
    }
    if surface == "all" || surface == "action" || surface == "condition" {
        report.insert("pipeline".to_owned(), pipeline);
    }
    if surface == "wiring" || surface == "all" {
        let codegen_text = fs::read_to_string(paths.root.join("quartz_forge/src/services/codegen.rs"))
            .unwrap_or_default();
        let editors_text = fs::read_to_string(paths.root.join("quartz_forge/src/app/editors.rs"))
            .unwrap_or_default();
        let domain_text = fs::read_to_string(&paths.forge_domain_rs).unwrap_or_default();

        let new_variants = [
            "SetVar",
            "ModVar",
            "Spawn",
            "PluginCall",
            "SetPosition",
            "SpawnObject",
            "SetText",
        ];
        let wiring: Vec<_> = new_variants.iter().map(|v| {
            json!({
                "variant": v,
                "in_domain": domain_text.contains(&format!("{v} {{")) || domain_text.contains(&format!("{v},")),
                "in_codegen": codegen_text.contains(&format!("QuartzAction::{v}")),
                "in_editors": editors_text.contains(&format!("QuartzAction::{v}")),
            })
        }).collect();
        report.insert(
            "new_action_wiring".to_owned(),
            json!({
                "variants_checked": new_variants,
                "status": wiring,
                "settext_prelude_hoisting": codegen_text.contains("action_expr_with_prelude") && codegen_text.contains("emit_action_lines"),
                "spawn_template_body_present": codegen_text.contains("spawn_template_body"),
            }),
        );
    }

    Ok(Value::Object(report))
}

fn spawn_audit(paths: &WorkspacePaths) -> Result<Value> {
    let app_text = fs::read_to_string(paths.root.join("quartz_forge/src/app/mod.rs"))?;
    let project_text = fs::read_to_string(paths.root.join("quartz_forge/src/core/project.rs"))?;
    let codegen_text = fs::read_to_string(paths.root.join("quartz_forge/src/services/codegen.rs"))?;
    let domain_text = fs::read_to_string(&paths.forge_domain_rs)?;

    Ok(json!({
        "spawn_only_field_present": domain_text.contains("spawn_only"),
        "spawn_overlay_toggle_present": app_text.contains("show_spawn_overlay"),
        "spawn_object_creator_present": project_text.contains("add_spawn_only_object_to_active_scene"),
        "spawn_helper_generation_present": codegen_text.contains("spawn_only"),
        "first_class_spawn_variant_present": domain_text.contains("Spawn {")
            && codegen_text.contains("QuartzAction::Spawn")
            && codegen_text.contains("Action::Spawn"),
        "first_class_plugincall_variant_present": domain_text.contains("PluginCall {")
            && codegen_text.contains("QuartzAction::PluginCall")
            && codegen_text.contains("Action::PluginCall"),
        "notes": [
            "spawn-only objects are omitted from setup_scene registration",
            "spawn-only objects can be drawn as ghost overlays when the overlay toggle is enabled",
            "spawn helpers are still emitted so runtime spawn actions can target them"
        ]
    }))
}

fn enum_variants(path: &Path, enum_name: &str) -> Result<Vec<String>> {
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut inside = false;
    let mut depth = 0i32;
    let mut variants = Vec::new();

    for line in content.lines() {
        if !inside {
            if line.contains(&format!("pub enum {enum_name}")) {
                inside = true;
                depth += line.matches('{').count() as i32;
            }
            continue;
        }

        depth += line.matches('{').count() as i32;
        depth -= line.matches('}').count() as i32;

        if let Some(name) = line
            .trim()
            .split(|c: char| c == ' ' || c == '{' || c == '(' || c == ',')
            .next()
            .filter(|s| !s.is_empty())
        {
            if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                variants.push(name.to_owned());
            }
        }

        if depth <= 0 {
            break;
        }
    }

    variants.sort();
    variants.dedup();
    Ok(variants)
}

fn locate_workspace_paths() -> Result<WorkspacePaths> {
    let mut roots = Vec::new();

    if let Ok(flowmake_root) = env::var("FLOWMAKE_WORKSPACE_ROOT") {
        roots.push(PathBuf::from(flowmake_root));
    }

    if let Ok(cwd) = env::current_dir() {
        roots.push(cwd);
    }

    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.to_path_buf());
        }
    }

    for start in roots {
        if let Some(paths) = find_workspace_paths_from(&start) {
            return Ok(paths);
        }
    }

    Err(anyhow!(
        "could not locate FlowMake workspace root (checked FLOWMAKE_WORKSPACE_ROOT, current_dir, and current_exe ancestors)"
    ))
}

fn find_workspace_paths_from(start: &Path) -> Option<WorkspacePaths> {
    let mut current = start.to_path_buf();

    loop {
        let api = current.join("quartz").join("api.txt");
        let action = current.join("quartz").join("src").join("types").join("action.rs");
        let condition = current.join("quartz").join("src").join("types").join("condition.rs");
        let forge_domain = current.join("quartz_forge").join("src").join("core").join("quartz_domain.rs");

        if api.exists() && action.exists() && condition.exists() && forge_domain.exists() {
            let mcp_dir = current.join(".quartz_forge").join("mcp");
            return Some(WorkspacePaths {
                root: current,
                quartz_api_txt: api,
                quartz_action_rs: action,
                quartz_condition_rs: condition,
                forge_domain_rs: forge_domain,
                mcp_dir: mcp_dir.clone(),
                lock_file: mcp_dir.join("server.lock"),
                heartbeat_file: mcp_dir.join("heartbeat.json"),
            });
        }

        if !current.pop() {
            break;
        }
    }

    None
}

fn health_report(paths: &WorkspacePaths) -> Result<Value> {
    fs::create_dir_all(&paths.mcp_dir)?;

    Ok(json!({
        "workspace_root": paths.root,
        "api_txt": paths.quartz_api_txt.exists(),
        "action_rs": paths.quartz_action_rs.exists(),
        "condition_rs": paths.quartz_condition_rs.exists(),
        "forge_domain_rs": paths.forge_domain_rs.exists(),
        "mcp_dir": paths.mcp_dir.exists(),
        "lock_status": lock_status(paths)?,
        "tools": tool_list().iter().map(|tool| tool.name).collect::<Vec<_>>()
    }))
}

fn lock_status(paths: &WorkspacePaths) -> Result<Value> {
    let lock_exists = paths.lock_file.exists();
    let heartbeat_exists = paths.heartbeat_file.exists();
    let heartbeat_age_s = if heartbeat_exists {
        let modified = fs::metadata(&paths.heartbeat_file)?.modified()?;
        SystemTime::now()
            .duration_since(modified)
            .unwrap_or(Duration::from_secs(0))
            .as_secs_f32()
    } else {
        -1.0
    };

    Ok(json!({
        "lock_exists": lock_exists,
        "heartbeat_exists": heartbeat_exists,
        "heartbeat_age_s": heartbeat_age_s,
        "lock_file": paths.lock_file,
        "heartbeat_file": paths.heartbeat_file,
    }))
}

#[allow(dead_code)]
fn acquire_lock(paths: &WorkspacePaths) -> Result<LockGuard> {
    fs::create_dir_all(&paths.mcp_dir)?;
    let pid = process::id();
    let lock_body = json!({
        "pid": pid,
        "started_at": SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    });

    if paths.lock_file.exists() {
        if let Ok(status) = lock_status(paths) {
            let stale = status
                .get("heartbeat_age_s")
                .and_then(Value::as_f64)
                .map(|age| age > 10.0)
                .unwrap_or(true);
            if !stale {
                return Err(anyhow!("quartz_forge_mcp lock is already held by another active process"));
            }
        }
    }

    fs::write(&paths.lock_file, serde_json::to_string_pretty(&lock_body)?)?;
    write_heartbeat(&paths.heartbeat_file, pid)?;

    let heartbeat_alive = Arc::new(AtomicBool::new(true));
    let heartbeat_file = paths.heartbeat_file.clone();
    let heartbeat_alive_clone = Arc::clone(&heartbeat_alive);
    let heartbeat_thread = thread::spawn(move || {
        while heartbeat_alive_clone.load(Ordering::SeqCst) {
            let _ = write_heartbeat(&heartbeat_file, pid);
            thread::sleep(Duration::from_secs(2));
        }
    });

    Ok(LockGuard {
        lock_file: paths.lock_file.clone(),
        heartbeat_file: paths.heartbeat_file.clone(),
        heartbeat_alive,
        heartbeat_thread: Some(heartbeat_thread),
    })
}

#[allow(dead_code)]
fn write_heartbeat(path: &Path, pid: u32) -> Result<()> {
    fs::write(
        path,
        serde_json::to_string_pretty(&json!({
            "pid": pid,
            "heartbeat_at": SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        }))?,
    )?;
    Ok(())
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        self.heartbeat_alive.store(false, Ordering::SeqCst);
        if let Some(handle) = self.heartbeat_thread.take() {
            let _ = handle.join();
        }
        let _ = fs::remove_file(&self.lock_file);
        let _ = fs::remove_file(&self.heartbeat_file);
    }
}

fn read_rpc_request(reader: &mut impl BufRead) -> Result<Option<(JsonRpcRequest, MessageFraming)>> {
    let mut line = String::new();

    loop {
        line.clear();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            return Ok(None);
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);

        // Support newline-delimited JSON transport used by quartz-ctx/VS Code MCP hosts.
        if trimmed.starts_with('{') {
            let request: JsonRpcRequest = serde_json::from_str(trimmed)?;
            return Ok(Some((request, MessageFraming::LineDelimited)));
        }

        if trimmed.is_empty() {
            continue;
        }

        let mut content_length = parse_content_length_header(trimmed);

        loop {
            line.clear();
            let bytes = reader.read_line(&mut line)?;
            if bytes == 0 {
                return Err(anyhow!("unexpected EOF while reading MCP headers"));
            }
            let header = line.trim_end_matches(['\r', '\n']);
            if header.is_empty() {
                break;
            }
            if content_length.is_none() {
                content_length = parse_content_length_header(header);
            }
        }

        let len = content_length.ok_or_else(|| anyhow!("missing Content-Length header"))?;
        let mut body = vec![0u8; len];
        reader.read_exact(&mut body)?;
        let request: JsonRpcRequest = serde_json::from_slice(&body)?;
        return Ok(Some((request, MessageFraming::ContentLength)));
    }
}

fn parse_content_length_header(header: &str) -> Option<usize> {
    let (name, value) = header.split_once(':')?;
    if !name.trim().eq_ignore_ascii_case("content-length") {
        return None;
    }
    value.trim().parse::<usize>().ok()
}

fn write_rpc_response(
    writer: &mut impl Write,
    response: JsonRpcResponse,
    framing: MessageFraming,
) -> Result<()> {
    let body = serde_json::to_vec(&response)?;

    match framing {
        MessageFraming::LineDelimited => {
            writer.write_all(&body)?;
            writer.write_all(b"\n")?;
        }
        MessageFraming::ContentLength => {
            write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
            writer.write_all(&body)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    /// The slim MCP server (mcp_server/) builds this library with the `gui`
    /// feature off. Code outside the gui-gated modules must therefore not use
    /// the gui-only crates, or that build breaks while the editor build (which
    /// is the one people run) stays green. Gated modules: app, and the three
    /// preview services (see lib.rs and services/mod.rs).
    #[test]
    fn code_outside_the_gui_modules_needs_no_gui_crates() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let gated = ["app", "services/background_preview.rs", "services/lighting_preview.rs", "services/path_forge_preview.rs", "main.rs", "bin/qf_verify_generated.rs"];
        let mut offenders = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                let rel = p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
                if gated.iter().any(|g| rel == *g || rel.starts_with(&format!("{g}/"))) { continue; }
                if p.is_dir() { stack.push(p); continue; }
                if p.extension().is_none_or(|x| x != "rs") { continue; }
                for (n, line) in std::fs::read_to_string(&p).unwrap().lines().enumerate() {
                    let code = line.split("//").next().unwrap_or("");
                    // Crate paths in code, not inside string literals.
                    let outside_strings: String = code.split('"').step_by(2).collect();
                    for c in ["egui::", "eframe::", "rfd::", "image::", "path_forge::"] {
                        let hit = outside_strings.match_indices(c).any(|(k, _)| !outside_strings[..k].ends_with(|ch: char| ch.is_alphanumeric() || ch == '_'));
                        if hit { offenders.push(format!("src/{rel}:{}: {}", n + 1, line.trim())); }
                    }
                }
            }
        }
        assert!(offenders.is_empty(), "gui-only crates used outside the gui modules (gate the module with #[cfg(feature = \"gui\")] or move the code):\n{}", offenders.join("\n"));
    }

    use super::*;

    fn test_workspace_paths() -> WorkspacePaths {
        let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = crate_root
            .parent()
            .expect("quartz_forge crate should live under FlowMake root")
            .to_path_buf();
        let mcp_dir = root.join(".quartz_forge").join("mcp");
        WorkspacePaths {
            quartz_api_txt: root.join("quartz").join("api.txt"),
            quartz_action_rs: root.join("quartz").join("src").join("types").join("action.rs"),
            quartz_condition_rs: root
                .join("quartz")
                .join("src")
                .join("types")
                .join("condition.rs"),
            forge_domain_rs: root
                .join("quartz_forge")
                .join("src")
                .join("core")
                .join("quartz_domain.rs"),
            root,
            mcp_dir: mcp_dir.clone(),
            lock_file: mcp_dir.join("server.lock"),
            heartbeat_file: mcp_dir.join("heartbeat.json"),
        }
    }

    /// SYNFUL fork divergence: synful quartz promotes grapple to first-class
    /// `Action` variants (AttachGrapple/ReleaseGrapple/SetGrapple*), whereas
    /// official quartz keeps grapple as a plugin driven by `Action::PluginCall`
    /// with `GrappleCommand`. The forge domain mirrors the official surface, so
    /// these synful-only actions show as "missing" — a KNOWN, documented gap,
    /// not a regression. They remain authorable via PluginCall / custom code.
    /// The test asserts the missing set is EXACTLY this known set, so any NEW
    /// divergence still fails.
    const SYNFUL_KNOWN_MISSING_ACTIONS: &[&str] = &[
        "AttachGrapple",
        "ReleaseGrapple",
        "SetGrappleAnchor",
        "SetGrappleAnchorObject",
        "SetGrappleDamping",
        "SetGrappleLength",
        "SetGrappleStiffness",
        "SetGrappleSwingBias",
    ];
    const SYNFUL_KNOWN_MISSING_CONDITIONS: &[&str] = &["HasGrapple", "NoGrapple"];

    #[test]
    fn parity_report_action_missing_set_empty_after_p6() {
        let paths = test_workspace_paths();
        let report = parity_report(&paths, "action").unwrap();
        let mut current = report["action"]["missing_in_forge"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .filter(|v| !SYNFUL_KNOWN_MISSING_ACTIONS.contains(v))
            .collect::<Vec<_>>();
        current.sort_unstable();
        assert!(
            current.is_empty(),
            "Action parity should be complete (aside from the known synful grapple-action gap), \
             but new missing variants remain: {:?}",
            current
        );
    }

    /// Parity is a PIPELINE property. The domain enum being complete means
    /// nothing if codegen cannot emit a variant or import degrades it to a
    /// raw blob. This is the honest version of the P6 assertion above — it
    /// would have caught the 21-action import gap that shipped under a green
    /// domain-parity light.
    #[test]
    fn pipeline_parity_codegen_and_import_fully_cover_actions() {
        let paths = test_workspace_paths();
        let report = parity_report(&paths, "all").unwrap();
        let pipeline = &report["pipeline"]["action"];

        let codegen_missing: Vec<_> = pipeline["codegen_missing"]
            .as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert!(
            codegen_missing.is_empty(),
            "codegen cannot emit these actions: {codegen_missing:?}"
        );

        let import_missing: Vec<_> = pipeline["import_missing"]
            .as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert!(
            import_missing.is_empty(),
            "these actions degrade to raw blobs on import (visual editing lost): {import_missing:?}"
        );

        let ui_missing: Vec<_> = pipeline["ui_missing"]
            .as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert!(
            ui_missing.is_empty(),
            "these actions have no structured UI editing: {ui_missing:?}"
        );
    }

    #[test]
    fn parity_report_condition_missing_set_empty_after_p1() {
        let paths = test_workspace_paths();
        let report = parity_report(&paths, "condition").unwrap();
        let mut current = report["condition"]["missing_in_forge"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .filter(|v| !SYNFUL_KNOWN_MISSING_CONDITIONS.contains(v))
            .collect::<Vec<_>>();
        current.sort_unstable();
        assert!(
            current.is_empty(),
            "Condition parity should be complete (aside from the known synful grapple-condition \
             gap), but new missing variants remain: {:?}",
            current
        );
    }

    #[test]
    fn parity_report_action_cluster_a_removed_from_missing() {
        let paths = test_workspace_paths();
        let report = parity_report(&paths, "action").unwrap();
        let current = report["action"]["missing_in_forge"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();

        let cluster_a = [
            "ApplyForce",
            "ApplyImpulse",
            "FreezeBody",
            "UnfreezeBody",
            "WakeBody",
            "SetMaterial",
            "SetDensity",
            "SetElasticity",
            "SetFriction",
            "SetPhysicsQuality",
            "SetCollisionMode",
            "SetSlope",
            "SetSurfaceNormal",
            "TransferMomentum",
        ];

        assert!(
            current
                .iter()
                .all(|variant| !cluster_a.contains(variant)),
            "Action parity still missing cluster A variants: {:?}",
            current
                .iter()
                .filter(|variant| cluster_a.contains(variant))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn mcp_condition_paths_return_deterministic_errors() {
        let paths = test_workspace_paths();

        let invalid_surface = call_tool(
            &paths,
            "qf_forge_check_parity",
            json!({ "surface": "conditions" }),
        )
        .unwrap_err()
        .to_string();
        assert!(
            invalid_surface
                .contains("qf_forge_check_parity invalid arguments.surface 'conditions' expected one of: action|condition|wiring|all")
        );

        let wrong_surface_type = call_tool(
            &paths,
            "qf_forge_check_parity",
            json!({ "surface": 42 }),
        )
        .unwrap_err()
        .to_string();
        assert!(
            wrong_surface_type
                .contains("qf_forge_check_parity requires arguments.surface as string: action|condition|wiring|all")
        );
    }

    #[test]
    fn parity_report_all_surface_shape_is_stable() {
        let paths = test_workspace_paths();
        let report = parity_report(&paths, "all").unwrap();

        assert!(report.get("action").is_some());
        assert!(report.get("condition").is_some());
        assert!(report.get("new_action_wiring").is_some());
        assert!(report["new_action_wiring"]["status"].is_array());

        let status_rows = report["new_action_wiring"]["status"].as_array().unwrap();
        for row in status_rows {
            assert!(row.get("variant").is_some());
            assert!(row.get("in_domain").is_some());
            assert!(row.get("in_codegen").is_some());
            assert!(row.get("in_editors").is_some());
        }
    }

    #[test]
    fn spawn_audit_reports_first_class_spawn_support() {
        let paths = test_workspace_paths();
        let report = spawn_audit(&paths).unwrap();

        assert_eq!(report["first_class_spawn_variant_present"], json!(true));
        assert_eq!(report["first_class_plugincall_variant_present"], json!(true));
    }

    #[test]
    fn mcp_requires_project_root_for_state_dump() {
        let paths = test_workspace_paths();
        let err = call_tool(&paths, "qf_project_state_dump", json!({}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("qf_project_state_dump requires arguments.project_root"));
    }

    #[test]
    fn mcp_requires_manifest_for_apply_state() {
        let paths = test_workspace_paths();
        let err = call_tool(
            &paths,
            "qf_project_apply_state",
            json!({
                "project_root": "./tmp_project"
            }),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("qf_project_apply_state requires arguments.manifest"));
    }

    #[test]
    fn mcp_requires_files_for_import_semantic() {
        let paths = test_workspace_paths();
        let err = call_tool(
            &paths,
            "qf_project_import_semantic",
            json!({
                "project_root": "./tmp_project"
            }),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("qf_project_import_semantic requires arguments.files"));
    }

    #[test]
    fn mcp_requires_files_for_import_manual_overrides() {
        let paths = test_workspace_paths();
        let err = call_tool(
            &paths,
            "qf_project_import_manual_overrides",
            json!({
                "project_root": "./tmp_project"
            }),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("qf_project_import_manual_overrides requires arguments.files"));
    }

    /// SYNFUL: the codegen guidance must document the synful-only lighting +
    /// post-fx API and its traps so the AI emits correct synful game code.
    #[test]
    fn codegen_guidance_documents_synful_lighting() {
        let paths = test_workspace_paths();
        let guidance = codegen_api_guidance(&paths);
        let lighting = &guidance["synful_lighting"];
        assert!(lighting.is_object(), "guidance must carry a synful_lighting section");
        let text = serde_json::to_string(lighting).unwrap();
        assert!(text.contains("enable_lighting"), "must document enable_lighting");
        assert!(text.contains("add_light"), "must document add_light");
        assert!(text.contains("attach_light"), "must document attach_light");
        assert!(text.contains("BloomSettings"), "must warn about BloomSettings use-path");
        assert!(text.contains("unlit"), "must document the unlit field trap");
        assert!(text.contains("E0451"), "must document the struct-update / private-field trap");
    }
}
