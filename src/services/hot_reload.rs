use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

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

        let mut command = Command::new("cargo");
        command
            .arg("run")
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        match command.spawn() {
            Ok(child) => {
                self.child = Some(child);
                self.cwd = Some(cwd.to_path_buf());
                self.state = PreviewState::Running;
                self.last_message = format!("Preview started in {}", cwd.display());
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
}

impl Drop for HotReloadService {
    fn drop(&mut self) {
        let _ = self.stop_preview();
    }
}
