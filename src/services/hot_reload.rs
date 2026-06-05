use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewLaunchMode {
    DirectBinary,
    CargoRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewState {
    Stopped,
    Running,
    Exited,
    Failed,
}

pub struct HotReloadService {
    child: Option<Child>,
    cwd: Option<PathBuf>,
    pub state: PreviewState,
    pub last_message: String,
}

impl Default for HotReloadService {
    fn default() -> Self {
        Self {
            child: None,
            cwd: None,
            state: PreviewState::Stopped,
            last_message: "Preview idle".to_owned(),
        }
    }
}

impl HotReloadService {
    pub fn start_preview(&mut self, cwd: &Path) -> Result<(), String> {
        if self.child.is_some() {
            return Err("preview is already running".to_owned());
        }

        match self.spawn_preview_process(cwd) {
            Ok((child, launch_mode)) => {
                self.child = Some(child);
                self.cwd = Some(cwd.to_path_buf());
                self.state = PreviewState::Running;
                self.last_message = match launch_mode {
                    PreviewLaunchMode::DirectBinary => {
                        format!("Preview started from prebuilt binary in {}", cwd.display())
                    }
                    PreviewLaunchMode::CargoRun => {
                        format!("Preview started via cargo run in {}", cwd.display())
                    }
                };
                Ok(())
            }
            Err(err) => {
                self.state = PreviewState::Failed;
                self.last_message = format!("Failed to start preview: {err}");
                Err(self.last_message.clone())
            }
        }
    }

    pub fn stop_preview(&mut self) -> Result<(), String> {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
            self.state = PreviewState::Stopped;
            self.last_message = "Preview stopped".to_owned();
            return Ok(());
        }
        Err("preview is not running".to_owned())
    }

    pub fn poll(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };

        match child.try_wait() {
            Ok(Some(status)) => {
                self.child = None;
                self.state = if status.success() {
                    PreviewState::Exited
                } else {
                    PreviewState::Failed
                };
                self.last_message = format!("Preview exited with status: {status}");
            }
            Ok(None) => {}
            Err(err) => {
                self.child = None;
                self.state = PreviewState::Failed;
                self.last_message = format!("Preview polling failed: {err}");
            }
        }
    }

    pub fn working_dir(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    fn spawn_preview_process(&self, cwd: &Path) -> Result<(Child, PreviewLaunchMode), String> {
        if let Some(binary) = self.preview_binary_path(cwd) {
            let mut direct = Command::new(&binary);
            direct
                .current_dir(cwd)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());

            if let Ok(child) = direct.spawn() {
                return Ok((child, PreviewLaunchMode::DirectBinary));
            }
        }

        let mut cargo = Command::new("cargo");
        cargo
            .arg("run")
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let child = cargo
            .spawn()
            .map_err(|err| format!("failed to launch preview via cargo: {err}"))?;
        Ok((child, PreviewLaunchMode::CargoRun))
    }

    fn preview_binary_path(&self, cwd: &Path) -> Option<PathBuf> {
        let package_name = read_package_name(cwd.join("Cargo.toml"))?;
        let mut filename = package_name;
        #[cfg(windows)]
        {
            filename.push_str(".exe");
        }

        let candidate = cwd.join("target").join("debug").join(filename);
        if candidate.exists() {
            Some(candidate)
        } else {
            None
        }
    }
}

fn read_package_name(cargo_toml_path: PathBuf) -> Option<String> {
    let text = std::fs::read_to_string(cargo_toml_path).ok()?;
    let mut in_package = false;

    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = line.strip_prefix("name") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let value = rest.trim();
                if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                    return Some(value[1..value.len() - 1].to_owned());
                }
            }
        }
    }

    None
}

impl Drop for HotReloadService {
    fn drop(&mut self) {
        let _ = self.stop_preview();
    }
}
