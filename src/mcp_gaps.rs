//! MCP tools for gap reports (`services::gaps`). An agent files and lists reports and marks an
//! approved one done once built; approving or rejecting belongs to the user, in the editor.

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::services::gaps::{GapBook, GapProposal, GapStatus};

/// The gap tools: their MCP descriptions and their calls.
pub struct GapTools;

impl GapTools {
    /// `(name, description, input schema)` for each tool.
    pub fn definitions() -> Vec<(&'static str, &'static str, Value)> {
        vec![
            (
                "qf_gap_propose",
                "File a gap report when a game idea needs something the engine, editor, live player, codegen or tools cannot do yet. Say what the game needs, why what exists does not cover it, and how to add it (crates, types, editor screens). Do not build it: the user approves or rejects reports in the editor (Tools > Gap Reports); build only approved ones.",
                json!({
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "area": { "type": "string", "enum": ["Engine", "Editor", "Runtime", "Codegen", "Tools"] },
                        "game": { "type": "string", "description": "The game or idea that ran into it" },
                        "need": { "type": "string" },
                        "proposal": { "type": "string" },
                        "effort": { "type": "string", "description": "small (hours) / medium (days) / large (weeks)" }
                    },
                    "required": ["title", "area", "need", "proposal", "effort"]
                }),
            ),
            (
                "qf_gap_list",
                "List gap reports, newest first, optionally by status (Proposed, Approved, Rejected, Done). Check before filing a duplicate, and before building anything a report covers: only Approved reports may be built.",
                json!({ "type": "object", "properties": { "status": { "type": "string", "enum": ["Proposed", "Approved", "Rejected", "Done"] } } }),
            ),
            (
                "qf_gap_mark_done",
                "Mark an Approved gap report Done after building it, with a note of what changed and how it was verified.",
                json!({ "type": "object", "properties": { "id": { "type": "string" }, "note": { "type": "string" } }, "required": ["id", "note"] }),
            ),
        ]
    }

    /// The tool's answer, or `None` when `name` is not one of these tools.
    pub fn call(name: &str, args: &Value) -> Option<Result<Value>> {
        let book = GapBook::shared();
        Some(match name {
            "qf_gap_propose" => serde_json::from_value::<GapProposal>(args.clone())
                .map_err(|error| anyhow!("qf_gap_propose: {error}"))
                .and_then(|proposal| book.propose(proposal))
                .map(|report| json!(report)),
            "qf_gap_list" => Self::list(&book, args),
            "qf_gap_mark_done" => Self::mark_done(&book, args),
            _ => return None,
        })
    }

    fn list(book: &GapBook, args: &Value) -> Result<Value> {
        let wanted: Option<GapStatus> = args.get("status").map(|status| serde_json::from_value(status.clone())).transpose()?;
        let reports: Vec<_> = book.list()?.into_iter().filter(|report| wanted.is_none_or(|status| report.status == status)).collect();
        Ok(json!({ "reports": reports }))
    }

    fn mark_done(book: &GapBook, args: &Value) -> Result<Value> {
        let text = |key: &str| args.get(key).and_then(Value::as_str).ok_or_else(|| anyhow!("qf_gap_mark_done requires {key}"));
        let id = text("id")?;
        let current = book.list()?.into_iter().find(|report| report.id == id).ok_or_else(|| anyhow!("no gap report `{id}`"))?;
        if current.status != GapStatus::Approved {
            bail!("gap `{id}` is {:?}: only an Approved report can be marked done", current.status);
        }
        Ok(json!(book.decide(id, GapStatus::Done, text("note")?)?))
    }
}
