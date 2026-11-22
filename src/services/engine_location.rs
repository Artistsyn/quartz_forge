//! Where the engine crates are, seen from a game project that can live in any folder.
//!
//! A game no longer sits inside the engine repository (`path = "../quartz"` used to force that):
//! its `Cargo.toml` points at the engine by a relative path worked out from wherever the game is,
//! and every game builds into one shared folder, so a new game costs neither engine-repository
//! space nor a second engine build.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};

/// The engine crates a generated game depends on, with the package name each is published as.
const ENGINE_CRATES: [(&str, &str); 3] = [("quartz", "quartz"), ("ramp", "ramp2"), ("quartz_path_forge", "quartz_path_forge")];

/// The engine checkout (the folder holding `quartz/`, `ramp/`, `quartz_path_forge/`).
#[derive(Debug, Clone)]
pub struct EngineLocation {
    root: PathBuf,
}

impl EngineLocation {
    /// `QF_ENGINE_ROOT` when set, else the checkout this editor was built from.
    pub fn find() -> Self {
        let root = std::env::var_os("QF_ENGINE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join(".."));
        Self { root: Self::absolute(&root) }
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: Self::absolute(&root.into()) }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where new games go unless the user picks elsewhere: `games/` beside the engine checkout.
    pub fn games_folder(&self) -> PathBuf {
        self.root.parent().map_or_else(|| self.root.join("games"), |parent| parent.join("games"))
    }

    /// The one build folder every game shares.
    pub fn shared_target(&self) -> PathBuf {
        self.games_folder().join(".target")
    }

    /// The `path = ".."` for engine crate `name` in the `Cargo.toml` of the game at `project`.
    pub fn crate_path(&self, project: &Path, name: &str) -> String {
        let target = self.root.join(name);
        let from = Self::absolute(project);
        Self::relative(&from, &target).unwrap_or(target).to_string_lossy().replace('\\', "/")
    }

    /// The dependency lines of a new game's `Cargo.toml`.
    pub fn dependency_lines(&self, project: &Path) -> String {
        ENGINE_CRATES[..2]
            .iter()
            .map(|(name, package)| self.dependency_line(project, name, package))
            .collect()
    }

    pub fn dependency_line(&self, project: &Path, name: &str, package: &str) -> String {
        let path = self.crate_path(project, name);
        if name == package {
            format!("{name} = {{ path = \"{path}\" }}\n")
        } else {
            format!("{name} = {{ package = \"{package}\", path = \"{path}\" }}\n")
        }
    }

    /// `cargo_toml` with every engine dependency whose path does not lead to the engine from
    /// `project` pointed at it again (a game moved, or made before games had their own folders).
    /// `None` when nothing needs changing.
    pub fn repair_dependencies(&self, project: &Path, cargo_toml: &str) -> Option<String> {
        let mut changed = false;
        let lines: Vec<String> = cargo_toml
            .lines()
            .map(|line| match self.repaired_line(project, line) {
                Some(repaired) => {
                    changed = true;
                    repaired
                }
                None => line.to_owned(),
            })
            .collect();
        changed.then(|| lines.join("\n") + "\n")
    }

    /// `.cargo/config.toml` for a game: build into the shared folder.
    pub fn ensure_shared_target(&self, project: &Path) -> Result<()> {
        let config = project.join(".cargo").join("config.toml");
        if config.exists() {
            return Ok(());
        }
        let target = self.shared_target();
        let shown = Self::relative(&Self::absolute(project), &target).unwrap_or(target);
        let text = format!(
            "# Every Quartz Forge game builds into one shared folder, so the engine is built once.\n[build]\ntarget-dir = \"{}\"\n",
            shown.to_string_lossy().replace('\\', "/")
        );
        std::fs::create_dir_all(project.join(".cargo")).context("failed to create .cargo")?;
        std::fs::write(&config, text).with_context(|| format!("failed to write {}", config.display()))
    }

    fn repaired_line(&self, project: &Path, line: &str) -> Option<String> {
        let (name, package) = ENGINE_CRATES.iter().find(|(name, _)| line.trim_start().starts_with(&format!("{name} =")))?;
        let start = line.find("path = \"")? + "path = \"".len();
        let length = line[start..].find('"')?;
        let current = &line[start..start + length];
        if project.join(current).join("Cargo.toml").is_file() {
            return None;
        }
        Some(self.dependency_line(project, name, package).trim_end().to_owned())
    }

    /// `to` as seen from `from`; `None` when they share no root (another drive).
    fn relative(from: &Path, to: &Path) -> Option<PathBuf> {
        let from_parts: Vec<Component<'_>> = from.components().collect();
        let to_parts: Vec<Component<'_>> = to.components().collect();
        if from_parts.first() != to_parts.first() {
            return None;
        }
        let shared = from_parts.iter().zip(&to_parts).take_while(|(a, b)| a == b).count();
        let mut path: PathBuf = std::iter::repeat_n(Component::ParentDir, from_parts.len() - shared).collect();
        path.extend(&to_parts[shared..]);
        Some(path)
    }

    /// The real absolute path: symlinks resolved (macOS `/var` is `/private/var`, and a `..`
    /// counted from the link would climb out of the wrong folder). A folder that does not exist
    /// yet is resolved through its nearest existing ancestor.
    fn absolute(path: &Path) -> PathBuf {
        let joined = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let mut lexical = PathBuf::new();
        for part in joined.components() {
            match part {
                Component::ParentDir => {
                    lexical.pop();
                }
                Component::CurDir => {}
                other => lexical.push(other),
            }
        }
        let mut existing = lexical.as_path();
        let mut missing = Vec::new();
        while !existing.exists() {
            let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else { return lexical };
            missing.push(name.to_owned());
            existing = parent;
        }
        let mut resolved = std::fs::canonicalize(existing).unwrap_or_else(|_| existing.to_path_buf());
        resolved.extend(missing.iter().rev());
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::EngineLocation;
    use std::path::Path;

    #[test]
    fn a_game_anywhere_points_back_at_the_engine() {
        let engine = EngineLocation::at("/work/arty/synful_quartz");
        assert_eq!(engine.crate_path(Path::new("/work/arty/games/sky"), "quartz"), "../../synful_quartz/quartz");
        assert_eq!(engine.crate_path(Path::new("/work/arty/synful_quartz/old_game"), "quartz"), "../quartz");
        assert_eq!(engine.games_folder(), Path::new("/work/arty/games"));
        let lines = engine.dependency_lines(Path::new("/work/arty/games/sky"));
        assert!(lines.contains("ramp = { package = \"ramp2\", path = \"../../synful_quartz/ramp\" }"), "{lines}");
    }

    #[test]
    fn a_moved_game_gets_its_engine_paths_repaired() {
        let engine = EngineLocation::find();
        let game = std::env::temp_dir().join(format!("qf_moved_game_{}", std::process::id()));
        std::fs::create_dir_all(&game).unwrap();
        let old = "[dependencies]\nramp = { package = \"ramp2\", path = \"../ramp\" }\nquartz = { path = \"../quartz\" }\nrand = \"0.8\"\n";
        let repaired = engine.repair_dependencies(&game, old).unwrap();
        assert!(!repaired.contains("\"../quartz\""), "{repaired}");
        assert!(repaired.contains("rand = \"0.8\""));
        let quartz_line = repaired.lines().find(|line| line.starts_with("quartz =")).unwrap();
        let path = quartz_line.split('"').nth(1).unwrap();
        assert!(game.join(path).join("Cargo.toml").is_file(), "{path} does not reach the engine");
        assert_eq!(engine.repair_dependencies(&game, &repaired), None, "a repaired file needs nothing more");
        let _ = std::fs::remove_dir_all(&game);
    }
}
