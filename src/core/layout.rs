use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct ProjectLayoutPaths {
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub scenes_dir: PathBuf,
    pub scripts_dir: PathBuf,
    pub resources_dir: PathBuf,
    pub assets_images_dir: PathBuf,
    pub assets_audio_dir: PathBuf,
    pub assets_fonts_dir: PathBuf,
    pub assets_data_dir: PathBuf,
    pub build_dir: PathBuf,
    pub editor_dir: PathBuf,
}

impl ProjectLayoutPaths {
    pub fn from_root(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            manifest_path: root.join("project.qforge.json"),
            scenes_dir: root.join("src").join("scenes"),
            scripts_dir: root.join("src").join("scripts"),
            resources_dir: root.join("resources"),
            assets_images_dir: root.join("assets").join("images"),
            assets_audio_dir: root.join("assets").join("audio"),
            assets_fonts_dir: root.join("assets").join("fonts"),
            assets_data_dir: root.join("assets").join("data"),
            build_dir: root.join("build"),
            editor_dir: root.join(".quartz_forge"),
        }
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        let dirs = [
            &self.root,
            &self.scenes_dir,
            &self.scripts_dir,
            &self.resources_dir,
            &self.assets_images_dir,
            &self.assets_audio_dir,
            &self.assets_fonts_dir,
            &self.assets_data_dir,
            &self.build_dir,
            &self.editor_dir,
        ];

        for dir in dirs {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("failed to create directory: {}", dir.display()))?;
        }

        Ok(())
    }
}
