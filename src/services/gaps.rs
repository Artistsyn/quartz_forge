//! Gap reports: something a game idea needs that the engine, the editor or the tools cannot do
//! yet, with a proposed way to add it. An agent files one (`qf_gap_propose`); the user approves
//! or rejects it in the editor; nothing is built from a gap before it is approved. Kept as one
//! JSON file per report in `quartz_forge/gaps/`, beside the editor rather than in any game, since
//! a gap is about the shared tools.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Which part of the toolchain is missing something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GapArea {
    /// The engine: quartz, crystalline physics, wgpu_canvas rendering, prism.
    Engine,
    /// The Quartz Forge editor: a screen, a control, a workflow.
    Editor,
    /// The live player and project data.
    Runtime,
    /// Generated code and import.
    Codegen,
    /// MCP tools and agent guidance.
    Tools,
}

/// Where a report stands; only the user moves it out of `Proposed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GapStatus {
    Proposed,
    Approved,
    Rejected,
    Done,
}

/// One gap and its proposed fix.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GapReport {
    pub id: String,
    pub title: String,
    pub area: GapArea,
    /// The game or idea that ran into it, when there is one.
    #[serde(default)]
    pub game: String,
    /// What the game needs and why what exists does not cover it.
    pub need: String,
    /// How to close it: which crates and types change, what the editor shows.
    pub proposal: String,
    /// A rough size: "small (hours)", "medium (days)", "large (weeks)".
    pub effort: String,
    pub status: GapStatus,
    pub filed_utc: String,
    /// The user's note on approving or rejecting, or the implementer's on finishing.
    #[serde(default)]
    pub decision: String,
}

/// The fields an agent supplies when filing a report.
#[derive(Debug, Clone, Deserialize)]
pub struct GapProposal {
    pub title: String,
    pub area: GapArea,
    #[serde(default)]
    pub game: String,
    pub need: String,
    pub proposal: String,
    pub effort: String,
}

/// The folder of gap reports.
#[derive(Debug, Clone)]
pub struct GapBook {
    dir: PathBuf,
}

impl GapBook {
    /// The editor's own folder of reports (`quartz_forge/gaps`), or `QF_GAPS_DIR`.
    pub fn shared() -> Self {
        let dir = std::env::var_os("QF_GAPS_DIR")
            .map_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("gaps"), PathBuf::from);
        Self { dir }
    }

    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Every report, newest first.
    pub fn list(&self) -> Result<Vec<GapReport>> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return Ok(Vec::new()) };
        let mut reports = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                let text = std::fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
                reports.push(serde_json::from_str::<GapReport>(&text).with_context(|| format!("failed to parse {}", path.display()))?);
            }
        }
        reports.sort_by(|a, b| b.filed_utc.cmp(&a.filed_utc).then_with(|| a.id.cmp(&b.id)));
        Ok(reports)
    }

    /// File a new report as `Proposed`; returns it with its id.
    pub fn propose(&self, proposal: GapProposal) -> Result<GapReport> {
        for (field, text) in [("title", &proposal.title), ("need", &proposal.need), ("proposal", &proposal.proposal)] {
            if text.trim().is_empty() {
                bail!("a gap report needs a {field}");
            }
        }
        let id = self.fresh_id(&proposal.title);
        let report = GapReport {
            id,
            title: proposal.title,
            area: proposal.area,
            game: proposal.game,
            need: proposal.need,
            proposal: proposal.proposal,
            effort: proposal.effort,
            status: GapStatus::Proposed,
            filed_utc: chrono::Utc::now().to_rfc3339(),
            decision: String::new(),
        };
        self.write(&report)?;
        Ok(report)
    }

    /// Move a report to `status` with a note. Approving and rejecting are the user's; an agent
    /// may only mark an approved report done.
    pub fn decide(&self, id: &str, status: GapStatus, note: &str) -> Result<GapReport> {
        let mut report = self.list()?.into_iter().find(|report| report.id == id).with_context(|| format!("no gap report `{id}`"))?;
        report.status = status;
        report.decision = note.to_owned();
        self.write(&report)?;
        Ok(report)
    }

    pub fn proposed_count(&self) -> usize {
        self.list().map_or(0, |reports| reports.iter().filter(|report| report.status == GapStatus::Proposed).count())
    }

    fn write(&self, report: &GapReport) -> Result<()> {
        std::fs::create_dir_all(&self.dir).with_context(|| format!("failed to create {}", self.dir.display()))?;
        let path = self.dir.join(format!("{}.json", report.id));
        let mut text = serde_json::to_string_pretty(report)?;
        text.push('\n');
        std::fs::write(&path, text).with_context(|| format!("failed to write {}", path.display()))
    }

    fn fresh_id(&self, title: &str) -> String {
        let slug: String = title
            .to_ascii_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
            .split('_')
            .filter(|part| !part.is_empty())
            .take(6)
            .collect::<Vec<_>>()
            .join("_");
        let date = chrono::Utc::now().format("%Y%m%d");
        let base = format!("{date}_{slug}");
        let mut id = base.clone();
        let mut n = 2;
        while self.dir.join(format!("{id}.json")).exists() {
            id = format!("{base}_{n}");
            n += 1;
        }
        id
    }
}

#[cfg(test)]
mod tests {
    use super::{GapArea, GapBook, GapProposal, GapStatus};

    #[test]
    fn a_proposed_gap_waits_for_a_decision() {
        let dir = std::env::temp_dir().join(format!("qf_gaps_{}", std::process::id()));
        let book = GapBook::at(&dir);
        let filed = book
            .propose(GapProposal {
                title: "Rope physics for a grappling game".to_owned(),
                area: GapArea::Engine,
                game: "Sky Courier".to_owned(),
                need: "Swinging needs a rope that wraps around corners.".to_owned(),
                proposal: "Add a rope constraint chain to crystalline; expose it as an action.".to_owned(),
                effort: "medium (days)".to_owned(),
            })
            .unwrap();
        assert_eq!(filed.status, GapStatus::Proposed);
        assert!(filed.id.ends_with("rope_physics_for_a_grappling_game"), "{}", filed.id);
        assert_eq!(book.proposed_count(), 1);
        let approved = book.decide(&filed.id, GapStatus::Approved, "yes, rope first").unwrap();
        assert_eq!(approved.decision, "yes, rope first");
        assert_eq!(book.proposed_count(), 0);
        assert!(book.propose(GapProposal { title: String::new(), ..serde_json::from_str("{\"title\":\"x\",\"area\":\"Editor\",\"need\":\"n\",\"proposal\":\"p\",\"effort\":\"small\"}").unwrap() }).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
