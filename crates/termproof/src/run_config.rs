//! A run described by a file rather than by a command line.
//!
//! Distinct from [`crate::config`], which registers plugins — action name to
//! implementation. This describes *one run*: which recipes, how to launch them,
//! where the output goes.
//!
//! Every field is optional, and an unset field means "whatever the CLI or the
//! built-in default already said". So an absent or empty config changes
//! nothing, and a caller can adopt one key at a time instead of all at once.
//!
//! # Precedence
//!
//! An explicitly-passed flag beats the config file, which beats the built-in
//! default — see [`pick`]. The distinction that makes this work is between a
//! flag the caller *passed* and one that merely *has a default*: if those look
//! alike, a config file can never override a defaulted flag, and half the
//! schema below would be dead. The CLI layer is responsible for telling them
//! apart.
//!
//! # Format
//!
//! YAML, with JSON parsing for free — YAML 1.2 is a superset of JSON, so one
//! loader reads both and callers need not care which they were handed.
//!
//! # Unknown keys are errors
//!
//! A misspelled key that is silently ignored is the worst outcome available:
//! the run succeeds and quietly does the wrong thing. `deny_unknown_fields`
//! turns that into a startup failure naming the key.
//!
//! This is deliberately unlike [`crate::config`], which preserves unknown keys
//! because it carries plugin settings it cannot know about. This schema is
//! closed, so it can afford to be strict.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;

/// The precedence rule, in one place.
///
/// Reads as what it means: the flag if one was passed, else the config, else
/// the default.
pub fn pick<T>(flag: Option<T>, configured: Option<T>, builtin: T) -> T {
    flag.or(configured).unwrap_or(builtin)
}

/// A whole run, as data.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RunConfig {
    /// Which recipes to run, and where they are found.
    pub discovery: Discovery,
    /// How the product under test is launched and driven.
    pub execution: Execution,
    /// Where results go, and what must have happened to call the run a pass.
    pub output: Output,
}

/// Which recipes to run, and where they are found.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Discovery {
    /// Directories a changed file must fall under to count as touching the
    /// framework itself. Empty means the consumer's built-in roots.
    pub roots: Vec<String>,

    /// The path fragment that marks the start of a repo-relative path, used to
    /// normalise absolute changed-file paths.
    pub repo_marker: Option<String>,

    /// Which recipes to run. `None` means the CLI decides.
    pub select: Option<Selection>,

    /// Glob patterns over recipe *names*; a match is skipped.
    ///
    /// The reason this exists: taking a flaky recipe out of CI should not need
    /// a code change and a land.
    pub exclude: Vec<String>,
}

/// The ways a run can choose its recipes — mutually exclusive by construction.
///
/// Modelled as an enum rather than four optional fields so that "all *and* a
/// priority" cannot be written down, let alone need validating.
///
/// ```yaml
/// select: {mode: all}
/// select: {mode: priority, value: P0}
/// select: {mode: names, value: [smoke, permissions]}
/// select: {mode: changed_files, value: /tmp/changed.txt}
/// ```
///
/// Adjacently tagged, not the terser `{priority: P0}`, because `serde_yaml`
/// 0.9 encodes an externally-tagged enum as a YAML *tag* — `!priority P0` —
/// which is not JSON and would cost the property that one loader reads both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "mode",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Selection {
    /// Every discovered recipe.
    All,
    /// Every recipe at this priority, e.g. `P0`.
    Priority(String),
    /// Exactly these recipes, by name.
    Names(Vec<String>),
    /// Recipes whose `ci_paths` match the changed files listed in this file,
    /// one path per line. See [`crate::selection`].
    ChangedFiles(String),
}

/// How the product under test is launched and driven.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Execution {
    /// Terminal transport, for example `pty` or `tmux`.
    pub transport: Option<String>,

    /// Renderer or renderer set to validate.
    pub renderer: Option<String>,

    /// Model identifier to run the product under test with, when it takes one.
    /// Uninterpreted here — passed through for the runner to resolve.
    pub model: Option<String>,

    /// Reasoning-effort setting to pair with [`model`](Self::model), for
    /// products that expose one. Also uninterpreted.
    pub effort: Option<String>,

    /// Where the binary under test comes from.
    pub binary: Option<BinarySource>,

    /// Environment applied to every recipe, under each recipe's own additions
    /// so a recipe can still override what it needs to.
    pub env: BTreeMap<String, String>,

    /// Multiplies every recipe's declared timeout.
    ///
    /// A scale rather than an absolute, because recipes legitimately differ —
    /// one number for all of them would either strangle the slow ones or make
    /// the fast ones useless as a hang detector. A loaded CI host needs all of
    /// them stretched by roughly the same factor.
    pub timeout_scale: Option<f64>,
}

/// Where the binary under test comes from.
///
/// ```yaml
/// binary: {source: installed}
/// binary: {source: build, change: D123}
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "source",
    content = "change",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum BinarySource {
    /// Use the already-installed binary.
    Installed,
    /// Build from the current checkout, labelling results with this change id.
    Build(String),
}

/// Where results go.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Output {
    /// Directory for screenshots, casts and videos.
    pub artifact_dir: Option<String>,

    /// Where the human-readable report is written.
    pub report_path: Option<String>,

    /// Where the machine-readable result JSON is written.
    pub result_json_path: Option<String>,

    /// Publishers to run, in order, each named for the consumer to resolve.
    ///
    /// Deliberately not an enum of known publishers: which ones exist is the
    /// consumer's business, and naming them here would put a specific
    /// deployment's storage systems into a layer that must not know about any.
    pub publishers: Vec<Publisher>,

    /// What has to have happened for the run to count as passing.
    pub require: Requirements,
}

/// One publisher, named, with settings this layer does not interpret.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Publisher {
    /// The publisher's name, for the consumer to resolve to an implementation.
    pub name: String,
    /// Opaque settings, passed to that implementation verbatim.
    pub settings: BTreeMap<String, String>,
}

/// What has to have happened for the run to count as passing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Requirements {
    /// Fail if evidence was not uploaded anywhere shareable.
    pub uploaded_media: bool,

    /// Fail if this specific publisher did not carry the evidence.
    ///
    /// Stronger than `uploaded_media`, which a fallback publisher satisfies.
    pub media_publisher: Option<String>,
}

impl RunConfig {
    /// Parse a config from YAML or JSON text.
    pub fn parse(text: &str) -> Result<Self, String> {
        // An empty file is a config that sets nothing, not a parse error:
        // `serde_yaml` decodes it as null, which will not deserialize into a
        // struct however defaulted its fields are.
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_yaml::from_str(text).map_err(|e| format!("invalid config: {e}"))
    }

    /// Read and parse a config file.
    pub fn from_path(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read config {}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The publisher settings for `name`, if it is configured.
    pub fn publisher(&self, name: &str) -> Option<&Publisher> {
        self.output.publishers.iter().find(|p| p.name == name)
    }
}

impl Discovery {
    /// Whether `recipe_name` is excluded.
    ///
    /// An unparseable pattern excludes nothing rather than failing the run: a
    /// bad glob in a skip list should not take down validation that would
    /// otherwise have run.
    pub fn excludes(&self, recipe_name: &str) -> bool {
        self.exclude.iter().any(|pattern| {
            globset::Glob::new(pattern)
                .map(|g| g.compile_matcher().is_match(recipe_name))
                .unwrap_or(false)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_config_asks_for_nothing() {
        let c = RunConfig::default();
        assert_eq!(c.discovery.select, None);
        assert_eq!(c.execution.renderer, None);
        assert_eq!(c.output.artifact_dir, None);
        assert!(c.output.publishers.is_empty());
        assert!(!c.output.require.uploaded_media);
    }

    #[test]
    fn an_empty_file_is_an_empty_config_not_an_error() {
        // A config file that exists but has been commented out is a normal
        // state, and failing the run over it would be surprising.
        for text in ["", "   ", "\n\n", "# nothing here\n"] {
            assert_eq!(RunConfig::parse(text).unwrap(), RunConfig::default());
        }
    }

    #[test]
    fn a_flag_beats_the_config_which_beats_the_default() {
        assert_eq!(pick(Some("flag"), Some("cfg"), "builtin"), "flag");
        assert_eq!(pick(None, Some("cfg"), "builtin"), "cfg");
        assert_eq!(pick(None, None, "builtin"), "builtin");
    }

    #[test]
    fn the_ci_workflows_flags_are_all_expressible() {
        // Every flag the shipped workflow passes, in one config. If a flag
        // stops being expressible this fails, which is the whole point.
        let text = r#"
discovery:
  roots: ["some/framework/root/"]
  repo_marker: "/repo/"
  select: {mode: names, value: [smoke, prompt-response, plugins]}
execution:
  transport: pty
  renderer: some-renderer
  binary: {source: installed}
output:
  artifact_dir: /tmp/evidence
  report_path: /tmp/report.md
  result_json_path: /tmp/results.json
  publishers:
    - name: object-store
      settings:
        bucket: some-bucket
        media_path: run/evidence
        report_path: run/report
  require:
    uploaded_media: true
    media_publisher: image-host
"#;
        let c = RunConfig::parse(text).unwrap();
        assert_eq!(
            c.discovery.select,
            Some(Selection::Names(vec![
                "smoke".to_string(),
                "prompt-response".to_string(),
                "plugins".to_string(),
            ]))
        );
        assert_eq!(c.execution.binary, Some(BinarySource::Installed));
        assert_eq!(c.output.report_path.as_deref(), Some("/tmp/report.md"));
        assert_eq!(
            c.publisher("object-store").unwrap().settings["bucket"],
            "some-bucket"
        );
        assert_eq!(
            c.output.require.media_publisher.as_deref(),
            Some("image-host")
        );
    }

    #[test]
    fn every_way_of_selecting_recipes_round_trips() {
        for (text, expected) in [
            ("select: {mode: all}", Selection::All),
            (
                "select: {mode: priority, value: P0}",
                Selection::Priority("P0".into()),
            ),
            (
                "select: {mode: names, value: [a]}",
                Selection::Names(vec!["a".to_string()]),
            ),
            (
                "select: {mode: changed_files, value: /tmp/c.txt}",
                Selection::ChangedFiles("/tmp/c.txt".into()),
            ),
        ] {
            let d: Discovery = serde_yaml::from_str(text).unwrap();
            assert_eq!(d.select, Some(expected));
        }
    }

    #[test]
    fn a_second_selection_has_nowhere_to_go() {
        // There is one `mode`, so "all *and* a priority" is not a thing the
        // schema can express — no validation pass to forget to call. What is
        // worth testing is the near miss: someone reaching for the terser
        // `{priority: P0, names: [a]}` shape gets an error rather than having
        // half of it quietly dropped.
        let e = RunConfig::parse("discovery:\n  select: {mode: priority, value: P0, names: [a]}\n")
            .unwrap_err();
        assert!(e.contains("names"), "{e}");
    }

    #[test]
    fn a_misspelled_selection_mode_lists_the_real_ones() {
        let e = RunConfig::parse("discovery:\n  select: {mode: prioritie}\n").unwrap_err();
        assert!(e.contains("prioritie") && e.contains("priority"), "{e}");
    }

    #[test]
    fn json_parses_too() {
        // Not a separate code path — YAML 1.2 is a JSON superset. Worth a test
        // because it is a property callers will rely on, not an accident we
        // are free to lose.
        let c = RunConfig::parse(r#"{"execution": {"renderer": "alternate"}}"#).unwrap();
        assert_eq!(c.execution.renderer.as_deref(), Some("alternate"));
    }

    #[test]
    fn a_misspelled_key_is_an_error_not_a_shrug() {
        let e = RunConfig::parse("output:\n  artifactdir: /tmp/x\n").unwrap_err();
        assert!(e.contains("artifactdir"), "{e}");
    }

    #[test]
    fn a_misspelled_section_is_an_error_too() {
        let e = RunConfig::parse("executon:\n  renderer: some-renderer\n").unwrap_err();
        assert!(e.contains("executon"), "{e}");
    }

    #[test]
    fn building_from_a_checkout_carries_the_change_id() {
        let c = RunConfig::parse("execution:\n  binary: {source: build, change: D123}\n").unwrap();
        assert_eq!(c.execution.binary, Some(BinarySource::Build("D123".into())));
    }

    #[test]
    fn excluding_a_recipe_by_glob() {
        let d = Discovery {
            exclude: vec!["slow-*".to_string(), "plugins".to_string()],
            ..Default::default()
        };
        assert!(d.excludes("slow-startup"));
        assert!(d.excludes("plugins"));
        assert!(!d.excludes("smoke"));
    }

    #[test]
    fn an_unparseable_exclude_pattern_excludes_nothing() {
        // Better to run a recipe someone meant to skip than to fail the run
        // over a typo in a skip list.
        let d = Discovery {
            exclude: vec!["[".to_string()],
            ..Default::default()
        };
        assert!(!d.excludes("smoke"));
    }

    #[test]
    fn nothing_is_excluded_by_default() {
        assert!(!Discovery::default().excludes("smoke"));
    }

    #[test]
    fn a_missing_file_says_which_file() {
        let e = RunConfig::from_path(Path::new("/nonexistent/config.yaml")).unwrap_err();
        assert!(e.contains("/nonexistent/config.yaml"), "{e}");
    }

    #[test]
    fn a_config_round_trips_through_serialisation() {
        // The schema is also an output format: a run can record the config it
        // resolved, and that record has to be loadable.
        let original = RunConfig {
            discovery: Discovery {
                select: Some(Selection::Priority("P0".into())),
                exclude: vec!["flaky-*".to_string()],
                ..Default::default()
            },
            execution: Execution {
                renderer: Some("all".into()),
                timeout_scale: Some(1.5),
                env: BTreeMap::from([("KEY".to_string(), "value".to_string())]),
                ..Default::default()
            },
            output: Output {
                artifact_dir: Some("/tmp/e".into()),
                ..Default::default()
            },
        };
        let text = serde_yaml::to_string(&original).unwrap();
        assert_eq!(RunConfig::parse(&text).unwrap(), original);
    }
}
