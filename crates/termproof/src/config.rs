//! Typed configuration with cascading precedence and extension preservation.
//!
//! Precedence (lowest to highest, per spec §4.1 and Python `config.py`):
//! builtin < legacy user config < current user config < legacy project config
//! < current project config < explicit `--config` file.
//!
//! Values are merged via recursive deep-merge: dicts are merged key-wise,
//! scalars and lists are replaced by the overlay. This matches the Python
//! `_deep_merge` behavior and ensures a later source only overrides values it
//! provides (so user defaults do not wipe `steps` when only `docker` is set).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Legacy user config directory (`~/.config/tui-verifier/config.yaml`).
pub const LEGACY_USER_CONFIG_DIR: &str = "tui-verifier";
/// Current user config directory (`~/.config/termproof/config.yaml`).
pub const CURRENT_USER_CONFIG_DIR: &str = "termproof";
/// Legacy project config directory (`.tui-verifier/config.yaml`).
pub const LEGACY_PROJECT_CONFIG_DIR: &str = ".tui-verifier";
/// Current project config directory (`.termproof/config.yaml`).
pub const CURRENT_PROJECT_CONFIG_DIR: &str = ".termproof";

/// Docker backend settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DockerBackendConfig {
    /// Container image name (empty means host-native / no Docker).
    #[serde(default)]
    pub image: String,

    /// Workspace mount point inside the container.
    #[serde(default = "default_docker_workdir")]
    pub workdir: String,

    /// Volume mounts.
    #[serde(default = "default_docker_volumes")]
    pub volumes: Vec<serde_json::Value>,

    /// Environment variables passed into the container.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Preserve any extra Docker fields.
    #[serde(default, flatten)]
    #[cfg_attr(feature = "schema", schemars(flatten))]
    pub extension: HashMap<String, serde_json::Value>,
}

fn default_docker_workdir() -> String {
    "/workspace".to_string()
}

fn default_docker_volumes() -> Vec<serde_json::Value> {
    vec![serde_json::json!({"host": ".", "container": "/workspace"})]
}

impl Default for DockerBackendConfig {
    fn default() -> Self {
        Self {
            image: String::new(),
            workdir: default_docker_workdir(),
            volumes: default_docker_volumes(),
            env: HashMap::new(),
            extension: HashMap::new(),
        }
    }
}

/// Global defaults (post-script idle wait cap).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GlobalDefaults {
    /// Cap for the post-script idle wait in PTY mode; `None` means wait up to
    /// the recipe timeout for quiescence.
    #[serde(
        default = "default_idle_cap",
        deserialize_with = "deserialize_idle_cap",
        serialize_with = "serialize_idle_cap"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "Option<f64>"))]
    pub idle_cap_seconds: Option<f64>,

    /// Preserve unknown defaults keys.
    #[serde(default, flatten)]
    #[cfg_attr(feature = "schema", schemars(flatten))]
    pub extension: HashMap<String, serde_json::Value>,
}

fn default_idle_cap() -> Option<f64> {
    Some(3.0)
}

fn deserialize_idle_cap<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<serde_json::Value>::deserialize(deserializer)?;
    match opt {
        None => Ok(Some(3.0)),
        Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(n)) => {
            let f = n
                .as_f64()
                .ok_or_else(|| serde::de::Error::custom("idle_cap_seconds must be a number"))?;
            if !f.is_finite() || f < 0.0 {
                return Err(serde::de::Error::custom(format!(
                    "idle_cap_seconds must be a finite nonnegative number, got {f:?}"
                )));
            }
            Ok(Some(f))
        }
        Some(other) => Err(serde::de::Error::custom(format!(
            "idle_cap_seconds must be a finite nonnegative number, got {other:?}"
        ))),
    }
}

fn serialize_idle_cap<S>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(v) => serializer.serialize_some(v),
        None => serializer.serialize_none(),
    }
}

impl Default for GlobalDefaults {
    fn default() -> Self {
        Self {
            idle_cap_seconds: Some(3.0),
            extension: HashMap::new(),
        }
    }
}

/// Typed verifier configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct VerifierConfig {
    /// Step plugin map: action name → `module:Class`.
    #[serde(default = "default_steps")]
    pub steps: HashMap<String, String>,

    /// Assertion plugin map: type name → `module:Class`.
    #[serde(default = "default_assertions")]
    pub assertions: HashMap<String, String>,

    /// Agent runner plugins.
    #[serde(default = "default_agent_runners")]
    pub agent_runners: HashMap<String, String>,

    /// Execution mode plugins.
    #[serde(default = "default_execution_modes")]
    pub execution_modes: HashMap<String, String>,

    /// Reporter plugins.
    #[serde(default = "default_reporters")]
    pub reporters: HashMap<String, String>,

    /// Screen renderer plugins.
    #[serde(default = "default_screen_renderers")]
    pub screen_renderers: HashMap<String, String>,

    /// Video backend plugins.
    #[serde(default = "default_video_backends")]
    pub video_backends: HashMap<String, String>,

    /// Session backend plugin identifier.
    #[serde(default = "default_session_backend")]
    pub session_backend: String,

    /// Docker backend configuration.
    #[serde(default)]
    pub docker: DockerBackendConfig,

    /// Global defaults.
    #[serde(default)]
    pub defaults: GlobalDefaults,

    /// Extension map for unknown top-level keys.
    #[serde(default, flatten)]
    #[cfg_attr(feature = "schema", schemars(flatten))]
    pub extension: HashMap<String, serde_json::Value>,
}

fn default_steps() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert(
        "wait_for_text".to_string(),
        "termproof.builtin_steps:WaitForText".to_string(),
    );
    m.insert(
        "wait_for_idle".to_string(),
        "termproof.builtin_steps:WaitForIdle".to_string(),
    );
    m.insert(
        "send_text".to_string(),
        "termproof.builtin_steps:SendText".to_string(),
    );
    m.insert(
        "send_line".to_string(),
        "termproof.builtin_steps:SendLine".to_string(),
    );
    m.insert(
        "press".to_string(),
        "termproof.builtin_steps:Press".to_string(),
    );
    m.insert(
        "sleep".to_string(),
        "termproof.builtin_steps:Sleep".to_string(),
    );
    m.insert(
        "wait_for_regex".to_string(),
        "termproof.builtin_steps:WaitForRegex".to_string(),
    );
    m
}

fn default_assertions() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert(
        "output_contains".to_string(),
        "termproof.builtin_assertions:OutputContains".to_string(),
    );
    m.insert(
        "output_not_contains".to_string(),
        "termproof.builtin_assertions:OutputNotContains".to_string(),
    );
    m.insert(
        "screen_contains".to_string(),
        "termproof.builtin_assertions:ScreenContains".to_string(),
    );
    m.insert(
        "screen_not_contains".to_string(),
        "termproof.builtin_assertions:ScreenNotContains".to_string(),
    );
    m.insert(
        "exit_code".to_string(),
        "termproof.builtin_assertions:ExitCode".to_string(),
    );
    m.insert(
        "file_exists".to_string(),
        "termproof.builtin_assertions:FileExists".to_string(),
    );
    m.insert(
        "file_contains".to_string(),
        "termproof.builtin_assertions:FileContains".to_string(),
    );
    m.insert(
        "json_schema".to_string(),
        "termproof.builtin_assertions:JsonSchema".to_string(),
    );
    m
}

fn default_agent_runners() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert(
        "codex".to_string(),
        "termproof.agent_driven:CodexCliAgentRunner".to_string(),
    );
    m
}

fn default_execution_modes() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert(
        "scripted_pty".to_string(),
        "termproof.builtin_modes:ScriptedPtyMode".to_string(),
    );
    m.insert(
        "scripted_process".to_string(),
        "termproof.builtin_modes:ScriptedProcessMode".to_string(),
    );
    m.insert(
        "agent_driven".to_string(),
        "termproof.builtin_modes:AgentDrivenMode".to_string(),
    );
    m
}

fn default_reporters() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert(
        "markdown".to_string(),
        "termproof.builtin_reporters:MarkdownReporter".to_string(),
    );
    m.insert(
        "junit_xml".to_string(),
        "termproof.builtin_reporters:JUnitXmlReporter".to_string(),
    );
    m
}

fn default_screen_renderers() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert(
        "svg".to_string(),
        "termproof.builtin_renderers:SvgRenderer".to_string(),
    );
    m.insert(
        "png".to_string(),
        "termproof.builtin_renderers:PngRenderer".to_string(),
    );
    m
}

fn default_video_backends() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert(
        "agg_ffmpeg".to_string(),
        "termproof.builtin_video:AggFfmpegBackend".to_string(),
    );
    m
}

fn default_session_backend() -> String {
    "termproof.builtin_session:PexpectAsciinemaBackend".to_string()
}

impl Default for VerifierConfig {
    fn default() -> Self {
        Self {
            steps: default_steps(),
            assertions: default_assertions(),
            agent_runners: default_agent_runners(),
            execution_modes: default_execution_modes(),
            reporters: default_reporters(),
            screen_renderers: default_screen_renderers(),
            video_backends: default_video_backends(),
            session_backend: default_session_backend(),
            docker: DockerBackendConfig::default(),
            defaults: GlobalDefaults::default(),
            extension: HashMap::new(),
        }
    }
}

impl VerifierConfig {
    /// Return a config populated entirely from built-in defaults.
    pub fn builtin() -> Self {
        Self::default()
    }

    /// Load configuration with full cascading precedence.
    ///
    /// When `user_path` is `Some`, exactly that file is used for user config
    /// and implicit user-location discovery is skipped (mirrors Python
    /// `load_config(user_path=...)`). When `config_path` is `Some`, it is
    /// applied last and wins.
    pub fn load(
        project_path: Option<&Path>,
        user_path: Option<&Path>,
        config_path: Option<&Path>,
    ) -> Result<Self, crate::error::CoreError> {
        // Start from builtin deep-merged empty dict.
        let mut merged = serde_json::to_value(Self::default()).expect("builtin serializes");

        let project_root = project_path
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let user_files: Vec<PathBuf> = if let Some(p) = user_path {
            vec![p.to_path_buf()]
        } else {
            let home = dirs_home();
            if let Some(home) = home {
                vec![
                    home.join(".config")
                        .join(LEGACY_USER_CONFIG_DIR)
                        .join("config.yaml"),
                    home.join(".config")
                        .join(CURRENT_USER_CONFIG_DIR)
                        .join("config.yaml"),
                ]
            } else {
                vec![]
            }
        };

        let project_files = [
            project_root
                .join(LEGACY_PROJECT_CONFIG_DIR)
                .join("config.yaml"),
            project_root
                .join(CURRENT_PROJECT_CONFIG_DIR)
                .join("config.yaml"),
        ];

        let explicit_files: Vec<PathBuf> = config_path
            .map(|p| vec![p.to_path_buf()])
            .unwrap_or_default();

        for path in user_files
            .iter()
            .chain(project_files.iter())
            .chain(explicit_files.iter())
        {
            if path.exists() {
                let overlay = load_yaml_as_value(path)?;
                merged = deep_merge(merged, overlay);
            }
        }

        // Now deserialize back, with strict idle_cap validation already applied
        // via custom deserializer. If it fails, surface as InvalidConfig.
        let config: Self =
            serde_json::from_value(merged).map_err(|e| crate::error::CoreError::InvalidConfig {
                field: "config".to_string(),
                message: e.to_string(),
            })?;
        Ok(config)
    }

    /// Convenience wrapper matching Python `load_config(project_path, user_path, config_path)`.
    pub fn from_paths(
        project_path: Option<&Path>,
        user_path: Option<&Path>,
        config_path: Option<&Path>,
    ) -> Result<Self, crate::error::CoreError> {
        Self::load(project_path, user_path, config_path)
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn load_yaml_as_value(path: &Path) -> Result<serde_json::Value, crate::error::CoreError> {
    let content = std::fs::read_to_string(path).map_err(|e| crate::error::CoreError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    if content.trim().is_empty() {
        return Ok(serde_json::Value::Object(Default::default()));
    }
    let yaml_value: serde_yaml::Value =
        serde_yaml::from_str(&content).map_err(|e| crate::error::CoreError::Parse {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
    // Convert YAML value to JSON value for uniform merging.
    let json_value =
        serde_json::to_value(yaml_value).map_err(|e| crate::error::CoreError::Parse {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
    // If the file top-level is not an object, treat as empty (mirrors Python's `return data if isinstance(data, dict) else {}`).
    if json_value.is_object() {
        Ok(json_value)
    } else {
        Ok(serde_json::Value::Object(Default::default()))
    }
}

/// Recursively deep-merge `overlay` into `base`; overlay leaves win.
fn deep_merge(base: serde_json::Value, overlay: serde_json::Value) -> serde_json::Value {
    match (base, overlay) {
        (serde_json::Value::Object(mut base_map), serde_json::Value::Object(overlay_map)) => {
            for (k, v) in overlay_map {
                let base_val = base_map.remove(&k).unwrap_or(serde_json::Value::Null);
                // Recurse only when both sides are objects.
                let merged = if base_val.is_object() && v.is_object() {
                    deep_merge(base_val, v)
                } else {
                    v
                };
                base_map.insert(k, merged);
            }
            serde_json::Value::Object(base_map)
        }
        (_, overlay) => overlay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_has_all_fields() {
        let c = VerifierConfig::builtin();
        assert!(c.steps.contains_key("wait_for_text"));
        assert!(c.assertions.contains_key("output_contains"));
        assert_eq!(c.docker.workdir, "/workspace");
        assert_eq!(c.defaults.idle_cap_seconds, Some(3.0));
    }

    #[test]
    fn deep_merge_overlay_wins_at_leaf() {
        let base = serde_json::json!({"a": {"x": 1, "y": 2}, "b": 3});
        let overlay = serde_json::json!({"a": {"y": 99, "z": 100}, "c": 4});
        let merged = deep_merge(base, overlay);
        assert_eq!(merged["a"]["x"], 1);
        assert_eq!(merged["a"]["y"], 99);
        assert_eq!(merged["a"]["z"], 100);
        assert_eq!(merged["b"], 3);
        assert_eq!(merged["c"], 4);
    }

    #[test]
    fn load_cascades_project_over_user() {
        let tmp = std::env::temp_dir().join(format!("termproof-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let project_dir = tmp.join("project");
        std::fs::create_dir_all(project_dir.join(".termproof")).unwrap();
        std::fs::write(
            project_dir.join(".termproof/config.yaml"),
            "docker:\n  image: project-image\n  workdir: /project\n",
        )
        .unwrap();
        let user_yaml = tmp.join("user.yaml");
        std::fs::write(
            &user_yaml,
            "docker:\n  image: user-image\n  env:\n    FROM_USER: '1'\n",
        )
        .unwrap();

        let cfg = VerifierConfig::load(Some(&project_dir), Some(&user_yaml), None).unwrap();
        assert_eq!(cfg.docker.image, "project-image");
        assert_eq!(cfg.docker.workdir, "/project");
        assert_eq!(cfg.docker.env.get("FROM_USER").unwrap(), "1");
    }
}
