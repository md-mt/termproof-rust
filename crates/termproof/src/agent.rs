//! Agent-driven execution: prompt building and bounded JSON parsing.
//!
//! Mirrors Python `termproof.agent_driven` with Rust-idiomatic bounds:
//! bounded input, fenced code blocks, and truncation handling.

use std::collections::HashMap;

use serde_json::Value;

use crate::models::Recipe;

/// Maximum bytes of agent output to parse (1 MiB).
pub const MAX_AGENT_OUTPUT_BYTES: usize = 1024 * 1024;

/// Maximum characters to include from prompt context in the generated prompt.
pub const MAX_PROMPT_CONTEXT_CHARS: usize = 50_000;

/// Build the agent prompt for a recipe.
///
/// Contains target command, checks, terminal size, and the full recipe JSON
/// (truncated if needed). Matches Python `build_agent_prompt` structure.
pub fn build_agent_prompt(recipe: &Recipe) -> String {
    let checks = if recipe.checks.is_empty() {
        vec!["Codex operator completed the verification".to_string()]
    } else {
        recipe.checks.clone()
    };
    let target = shell_quote(&recipe.command.argv);
    let mut data = serde_json::json!({
        "recipe": recipe.name,
        "description": recipe.description,
        "intent": recipe.intent,
        "target_command": recipe.command.argv,
        "cwd": recipe.command.cwd,
        "pty": recipe.command.pty,
        "terminal": {"cols": recipe.cols, "rows": recipe.rows},
        "checks": checks,
        "steps": recipe.steps,
        "assertions": recipe.assertions,
        "expect_exit_code": recipe.expect_exit_code,
    });
    // Truncate if too large.
    let mut json_str = serde_json::to_string_pretty(&data).unwrap_or_default();
    if json_str.len() > MAX_PROMPT_CONTEXT_CHARS {
        json_str.truncate(MAX_PROMPT_CONTEXT_CHARS);
        // Ensure we end with valid truncation marker.
        json_str.push_str("\n... [truncated]");
        // Also truncate data representation.
        if let Some(obj) = data.as_object_mut() {
            obj.insert("truncated".to_string(), Value::Bool(true));
        }
        json_str = serde_json::to_string_pretty(&data).unwrap_or_default();
        if json_str.len() > MAX_PROMPT_CONTEXT_CHARS {
            json_str.truncate(MAX_PROMPT_CONTEXT_CHARS);
            json_str.push_str("... [truncated]");
        }
    }
    let mut prompt = String::new();
    prompt.push_str("You are the Codex operator for an evidence-first TUI verification run.\n");
    prompt.push('\n');
    prompt
        .push_str("Exercise the target terminal workflow and decide whether each check passes.\n");
    prompt.push_str("Do not modify files; only inspect or run commands needed for verification.\n");
    prompt.push_str("The harness will turn your transcript into asciinema evidence, screenshots, videos, and reports.\n");
    prompt.push('\n');
    prompt.push_str(&format!("Target command: `{target}`\n"));
    prompt.push('\n');
    prompt.push_str("Recipe context:\n");
    prompt.push_str("```json\n");
    prompt.push_str(&json_str);
    prompt.push_str("\n```\n");
    prompt.push('\n');
    prompt.push_str("Return JSON only with this schema:\n");
    prompt.push_str("```json\n");
    prompt.push_str(
        r#"{"assertions":{"check name":true},"transcript":"what you observed","notes":"optional"}"#,
    );
    prompt.push_str("\n```\n");
    prompt
}

/// Outcome of parsing agent output.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedAgentOutput {
    /// Map of check name -> pass/fail.
    pub assertions: HashMap<String, bool>,
    /// Transcript string.
    pub transcript: String,
    /// Additional metadata (e.g. notes).
    pub metadata: HashMap<String, Value>,
}

/// Parse agent output with bounding, fence handling, and truncation.
///
/// Mirrors Python `parse_agent_output` / `_load_json`:
/// - bounds input to `MAX_AGENT_OUTPUT_BYTES` (truncates tail)
/// - extracts JSON from fenced ``` blocks
/// - tries each `{` position via `serde_json::Deserializer::raw_decode` style
/// - prefers objects containing `assertions` or `transcript`
pub fn parse_agent_output(output: &str) -> ParsedAgentOutput {
    // Bound input.
    let bounded = if output.len() > MAX_AGENT_OUTPUT_BYTES {
        &output[..MAX_AGENT_OUTPUT_BYTES]
    } else {
        output
    };
    let data = load_json(bounded);
    match data {
        None => ParsedAgentOutput {
            assertions: HashMap::new(),
            transcript: bounded.to_string(),
            metadata: HashMap::new(),
        },
        Some(map) => {
            let assertions = match map.get("assertions") {
                Some(Value::Object(obj)) => obj
                    .iter()
                    .map(|(k, v)| (k.clone(), v.as_bool().unwrap_or(false)))
                    .collect(),
                _ => HashMap::new(),
            };
            let transcript = match map.get("transcript") {
                Some(Value::String(s)) => s.clone(),
                _ => bounded.to_string(),
            };
            let mut metadata = HashMap::new();
            for (k, v) in map {
                if k != "assertions" && k != "transcript" {
                    metadata.insert(k, v);
                }
            }
            ParsedAgentOutput {
                assertions,
                transcript,
                metadata,
            }
        }
    }
}

/// Try to locate a JSON object in the output, preferring those with
/// `assertions`/`transcript` keys. Handles fenced blocks and truncated input.
fn load_json(output: &str) -> Option<HashMap<String, Value>> {
    let mut candidates: Vec<String> = Vec::new();
    let trimmed = output.trim().to_string();
    if !trimmed.is_empty() {
        candidates.push(trimmed);
    }
    // Each line reversed (as Python does).
    for line in output.lines().rev() {
        let t = line.trim();
        if !t.is_empty() {
            candidates.push(t.to_string());
        }
    }
    // Fenced code blocks.
    if output.contains("```") {
        let chunks: Vec<&str> = output.split("```").collect();
        for chunk in chunks.iter().rev() {
            let mut c = chunk.trim();
            if c.to_ascii_lowercase().starts_with("json") {
                c = c[4..].trim();
            }
            if !c.is_empty() {
                candidates.push(c.to_string());
            }
        }
    }
    // Try each `{` position using serde_json parsing.
    let mut values: Vec<HashMap<String, Value>> = Vec::new();
    for cand in &candidates {
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(cand) {
            let hm: HashMap<String, Value> = map.into_iter().collect();
            values.push(hm);
        }
    }
    // Raw decode from each `{` offset (handles trailing truncation / extra text).
    // Iterate from last `{` backwards.
    let bytes = output.as_bytes();
    for idx in (0..bytes.len()).rev() {
        if bytes[idx] == b'{' {
            let slice = &output[idx..];
            // Try to parse with streaming, allowing truncated tail to be ignored
            // by trimming to last `}` if present.
            if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(slice) {
                let hm: HashMap<String, Value> = map.into_iter().collect();
                values.push(hm);
                continue;
            }
            // Try truncated repair: find last `}` and try substring.
            if let Some(last_brace) = slice.rfind('}') {
                let repaired = &slice[..=last_brace];
                if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(repaired) {
                    let hm: HashMap<String, Value> = map.into_iter().collect();
                    values.push(hm);
                }
            }
        }
    }
    // Prefer values containing assertions/transcript.
    for v in &values {
        if v.contains_key("assertions") || v.contains_key("transcript") {
            return Some(v.clone());
        }
    }
    values.into_iter().next()
}

/// Shell-quote argv for display in prompt.
fn shell_quote(argv: &[String]) -> String {
    argv.iter()
        .map(|s| {
            if s.contains(' ') || s.contains('"') || s.contains('\'') {
                format!("'{}'", s.replace('\'', "'\\''"))
            } else {
                s.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe(name: &str, checks: Vec<&str>) -> Recipe {
        Recipe {
            name: name.to_string(),
            command: crate::models::CommandSpec {
                argv: vec!["pi".to_string(), "--help".to_string()],
                ..Default::default()
            },
            checks: checks.into_iter().map(|s| s.to_string()).collect(),
            execution: "agent-driven".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn prompt_includes_target_and_checks() {
        let r = recipe("pi-agent", vec!["Pi launcher banner renders"]);
        let p = build_agent_prompt(&r);
        assert!(p.contains("pi --help"), "prompt missing target");
        assert!(p.contains("Pi launcher banner renders"));
        assert!(p.contains("Return JSON only"));
    }

    #[test]
    fn parse_fenced_json() {
        let out =
            "```json\n{\"assertions\":{\"ok\":true},\"transcript\":\"done\",\"notes\":\"n\"}\n```";
        let parsed = parse_agent_output(out);
        assert_eq!(parsed.assertions.get("ok"), Some(&true));
        assert_eq!(parsed.transcript, "done");
        assert_eq!(
            parsed.metadata.get("notes").and_then(|v| v.as_str()),
            Some("n")
        );
    }

    #[test]
    fn parse_truncated_output_still_finds_json() {
        let out = "noise {\"assertions\":{\"a\":true},\"transcript\":\"t\"} trailing garbage ...";
        let parsed = parse_agent_output(out);
        assert_eq!(parsed.assertions.get("a"), Some(&true));
    }

    #[test]
    fn parse_bounded_truncates_large_output() {
        let big = "x".repeat(MAX_AGENT_OUTPUT_BYTES + 100) + r#"{"assertions":{"ok":true}}"#;
        // The JSON is beyond the bound, so it should not be found; transcript is bounded prefix.
        let parsed = parse_agent_output(&big);
        // Should return raw bounded prefix as transcript when no JSON found within bound.
        assert!(parsed.transcript.len() <= MAX_AGENT_OUTPUT_BYTES);
    }

    #[test]
    fn parse_malformed_returns_raw() {
        let out = "not json at all";
        let parsed = parse_agent_output(out);
        assert!(parsed.assertions.is_empty());
        assert_eq!(parsed.transcript, out);
    }
}
