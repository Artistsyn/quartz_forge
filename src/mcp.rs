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
            "guidance": codegen_api_guidance(),
        })),
        "qf_background_plugin_contract" => Ok(json!({
            "tool": tool_name,
            "contract": background_plugin_contract(paths)?,
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

fn codegen_api_guidance() -> Value {
    json!({
        "mandate": "Exhaust native Quartz API before writing custom Rust. This is a HARD RULE enforced by quartz_forge and copilot-instructions.md §5b.",
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
                "Keep scalar game state in canvas.set_var/get_var with Action::SetVar/ModVar for importer-visible game vars."
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
            "Entropy::range(int, int) — must be f32: Entropy::range(0.0, 10.0)"
        ],
        "quick_patterns": {
            "score_increment": "Action::ModVar { name: 'score'.into(), op: MathOp::Add, operand: Expr::i32(1) }",
            "lives_decrement": "Action::ModVar { name: 'lives'.into(), op: MathOp::Sub, operand: Expr::i32(1) }",
            "game_over_check": "Action::Conditional { condition: Expr::var('lives').lte(Expr::i32(0)), if_true: Box::new(Action::Custom { name: 'game_over'.into() }), if_false: None }",
            "thrust_left": "canvas.add_event(GameEvent::KeyHold { key: Key::Character('a'), action: Action::ApplyMomentum { target: Target::name('player'), value: (-THRUST, 0.0) }, target: Target::name('player'), modifiers: None }, Target::name('player'))",
            "on_collision_with_enemy": "canvas.add_event(GameEvent::Collision { action: Action::Multi { vec![Action::ModVar { .. lives -1 }, Action::CameraShake { .. }] }, target: Target::tag('enemy') }, Target::name('player'))"
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
            "Keep generated scene/component module paths relative to the scene source file layout, not hard-coded to the project root."
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
                "Action::SetPosition for movement — zeroes momentum; use Teleport or ApplyMomentum/SetMomentum"
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

    Ok(json!({
        "project_root": root,
        "project_name": state.manifest.project_name,
        "active_scene_id": state.manifest.active_scene_id,
        "scene_count": state.manifest.scenes.len(),
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

    #[test]
    fn parity_report_action_missing_set_empty_after_p6() {
        let paths = test_workspace_paths();
        let report = parity_report(&paths, "action").unwrap();
        let current = report["action"]["missing_in_forge"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert!(
            current.is_empty(),
            "Action parity should be complete after P6, but missing variants remain: {:?}",
            current
        );
    }

    #[test]
    fn parity_report_condition_missing_set_empty_after_p1() {
        let paths = test_workspace_paths();
        let report = parity_report(&paths, "condition").unwrap();
        let current = report["condition"]["missing_in_forge"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert!(
            current.is_empty(),
            "Condition parity should be complete after P1, but missing variants remain: {:?}",
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
}
