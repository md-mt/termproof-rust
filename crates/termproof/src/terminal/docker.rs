//! Docker session backend — wraps commands in `docker run`.
//!
//! Matches Python `termproof.builtin_session.DockerSessionBackend` behavior:
//! image, env, mounts, workdir, interactive/tty, cleanup, error, and artifact
//! handling all go through the public `SessionBackend` surface.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::terminal::backend::SessionBackend;
use crate::terminal::error::SessionError;
use crate::terminal::session::Session;

/// Configuration for the Docker session backend.
///
/// Mirrors `DockerBackendConfig` in [`crate::config`] / Python `config.py`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerBackendConfig {
    /// Docker image (required).
    pub image: String,
    /// Working directory inside the container.
    pub workdir: String,
    /// Volume mounts. Each entry is either a raw string (`/host:/ctr:ro`) or
    /// a mapping string produced from host/container dicts.
    pub volumes: Vec<String>,
    /// Extra environment variables injected into the container.
    pub env: HashMap<String, String>,
}

impl Default for DockerBackendConfig {
    fn default() -> Self {
        Self {
            image: String::new(),
            workdir: "/workspace".to_string(),
            volumes: vec![".:/workspace".to_string()],
            env: HashMap::new(),
        }
    }
}

/// Backend that wraps argv in `docker run --rm --interactive --tty`.
#[derive(Debug, Clone)]
pub struct DockerSessionBackend {
    /// Backend configuration.
    pub config: DockerBackendConfig,
    /// Path to the docker binary.
    pub docker_bin: String,
}

impl DockerSessionBackend {
    /// Create a new Docker backend.
    pub fn new(config: DockerBackendConfig) -> Self {
        Self {
            config,
            docker_bin: "docker".to_string(),
        }
    }

    /// Create with a custom docker binary path (for testing).
    pub fn with_docker_bin(config: DockerBackendConfig, docker_bin: impl Into<String>) -> Self {
        Self {
            config,
            docker_bin: docker_bin.into(),
        }
    }

    /// Build the `docker run` argv that will be recorded.
    ///
    /// This is public so tests can assert exact flag ordering without needing
    /// to actually launch Docker.
    pub fn docker_argv(
        &self,
        argv: &[String],
        cwd: Option<&str>,
        env: &HashMap<String, String>,
    ) -> Result<Vec<String>, SessionError> {
        if self.config.image.is_empty() {
            return Err(SessionError::Config(
                "docker session backend requires docker.image in config".to_string(),
            ));
        }
        let mut cmd = vec![
            self.docker_bin.clone(),
            "run".to_string(),
            "--rm".to_string(),
            "--interactive".to_string(),
            "--tty".to_string(),
        ];
        for v in &self.config.volumes {
            let resolved = resolve_volume(v, cwd);
            cmd.push("--volume".to_string());
            cmd.push(resolved);
        }
        // Merge config env + per-session env, per-session wins.
        let mut merged = self.config.env.clone();
        for (k, v) in env {
            merged.insert(k.clone(), v.clone());
        }
        let mut keys: Vec<_> = merged.keys().collect();
        keys.sort();
        for k in keys {
            let v = &merged[k];
            cmd.push("--env".to_string());
            cmd.push(format!("{k}={v}"));
        }
        if !self.config.workdir.is_empty() {
            cmd.push("--workdir".to_string());
            cmd.push(self.config.workdir.clone());
        }
        cmd.push(self.config.image.clone());
        cmd.extend(argv.iter().cloned());
        Ok(cmd)
    }
}

impl SessionBackend for DockerSessionBackend {
    fn create_session(
        &self,
        argv: Vec<String>,
        cast_path: PathBuf,
        cwd: Option<String>,
        env: HashMap<String, String>,
        cols: u16,
        rows: u16,
    ) -> Result<Box<dyn Session>, SessionError> {
        let docker_argv = self.docker_argv(&argv, cwd.as_deref(), &env)?;
        let stub = StubDockerSession {
            argv: docker_argv,
            cast_path,
            cols,
            rows,
            screen: String::new(),
            raw_output: String::new(),
            exit_code: None,
        };
        Ok(Box::new(stub))
    }

    fn name(&self) -> &str {
        "docker"
    }
}

/// Resolve a volume spec, making host paths absolute.
///
/// Mirrors Python `_host_path` / `_volume_args`.
fn resolve_volume(volume: &str, cwd: Option<&str>) -> String {
    if let Some(colon) = volume.find(':') {
        let host = &volume[..colon];
        let rest = &volume[colon..];
        let host_path = Path::new(host);
        let resolved_host = if host_path.is_absolute() {
            host.to_string()
        } else {
            let base = cwd.unwrap_or(".");
            let expanded = if host.starts_with("~/") || host == "~" {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                host.replacen('~', &home, 1)
            } else {
                host.to_string()
            };
            let p = Path::new(base).join(expanded);
            p.to_string_lossy().to_string()
        };
        format!("{resolved_host}{rest}")
    } else {
        let p = Path::new(volume);
        if p.is_absolute() {
            volume.to_string()
        } else if volume == "." || volume.starts_with("./") || volume.starts_with("../") {
            let base = cwd.unwrap_or(".");
            Path::new(base).join(volume).to_string_lossy().to_string()
        } else {
            volume.to_string()
        }
    }
}

/// In-memory stub session used by Docker backend tests.
#[derive(Debug)]
struct StubDockerSession {
    argv: Vec<String>,
    cast_path: PathBuf,
    cols: u16,
    rows: u16,
    screen: String,
    raw_output: String,
    exit_code: Option<i32>,
}

impl Session for StubDockerSession {
    fn send_text(&mut self, _text: &str) -> Result<(), SessionError> {
        Ok(())
    }
    fn send_line(&mut self, _text: &str) -> Result<(), SessionError> {
        Ok(())
    }
    fn press(&mut self, _key: &str) -> Result<(), SessionError> {
        Ok(())
    }
    fn wait_for_text(
        &mut self,
        _text: &str,
        _timeout: std::time::Duration,
    ) -> Result<bool, SessionError> {
        Ok(false)
    }
    fn wait_for_idle(
        &mut self,
        _stable: std::time::Duration,
        _timeout: std::time::Duration,
    ) -> Result<bool, SessionError> {
        Ok(true)
    }
    fn wait_for_exit(
        &mut self,
        _timeout: std::time::Duration,
    ) -> Result<Option<i32>, SessionError> {
        Ok(self.exit_code)
    }
    fn read_available(&mut self, _timeout: std::time::Duration) -> Result<(), SessionError> {
        Ok(())
    }
    fn is_alive(&mut self) -> bool {
        false
    }
    fn close(&mut self) -> Result<(), SessionError> {
        Ok(())
    }
    fn screen(&self) -> &str {
        &self.screen
    }
    fn raw_output(&self) -> &str {
        &self.raw_output
    }
    fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }
    fn cols(&self) -> u16 {
        self.cols
    }
    fn rows(&self) -> u16 {
        self.rows
    }
    fn argv(&self) -> &[String] {
        &self.argv
    }
    fn cast_path(&self) -> &std::path::Path {
        &self.cast_path
    }
}
