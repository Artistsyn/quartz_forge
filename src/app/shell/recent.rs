//! Projects opened lately, newest first, kept in `~/.quartz_forge/recent.json` so the welcome
//! screen and File > Open Recent can offer them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(in crate::app) struct RecentProject {
    pub name: String,
    pub path: PathBuf,
    pub opened_utc: String,
}

#[derive(Debug, Clone, Default)]
pub(in crate::app) struct RecentProjects {
    /// Where the list is saved; `None` keeps it in memory only.
    file: Option<PathBuf>,
    pub entries: Vec<RecentProject>,
}

impl RecentProjects {
    /// The list in the user's home folder (`QF_RECENT_FILE` overrides the file).
    pub fn load_default() -> Self {
        let file = std::env::var_os("QF_RECENT_FILE").map(PathBuf::from).or_else(|| {
            std::env::var_os("HOME").map(|home| Path::new(&home).join(".quartz_forge").join("recent.json"))
        });
        file.map_or_else(Self::default, Self::at)
    }

    pub fn at(file: impl Into<PathBuf>) -> Self {
        let file = file.into();
        let entries = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<RecentProject>>(&text).ok())
            .unwrap_or_default();
        Self { file: Some(file), entries }
    }

    /// Put `path` first; returns the save error, if any, for the status line.
    pub fn note_opened(&mut self, name: &str, path: &Path) -> Option<String> {
        self.entries.retain(|entry| entry.path != path);
        self.entries.insert(
            0,
            RecentProject { name: name.to_owned(), path: path.to_path_buf(), opened_utc: chrono::Utc::now().to_rfc3339() },
        );
        self.save()
    }

    pub fn forget(&mut self, path: &Path) -> Option<String> {
        self.entries.retain(|entry| entry.path != path);
        self.save()
    }

    fn save(&self) -> Option<String> {
        let file = self.file.as_ref()?;
        let written = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(file, serde_json::to_string_pretty(&self.entries).unwrap_or_default()));
        written.err().map(|error| format!("could not save the recent projects list: {error}"))
    }
}
