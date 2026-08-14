//! Run cache — content-addressed by recipe file hash + run options.
//!
//! Mirrors `termproof/run_cache.py` but with Rust-idiomatic atomic storage
//! and SHA-256 hashing via `sha2`.

use crate::result::RunResult;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Compute a stable cache key for a recipe file + options.
///
/// Returns `None` when the recipe has no source path (e.g. synthetic).
#[allow(clippy::too_many_arguments)]
pub fn cache_key(
    recipe_source: Option<&Path>,
    recipe_bytes: Option<&[u8]>,
    renderer: &str,
    renderer_argv: &[String],
    out_dir: &Path,
    screen_renderer: &str,
    video_backend: &str,
    render_video: bool,
    video_fps: u32,
    ci_paths: &[String],
    ci_base: Option<&Path>,
) -> Option<String> {
    let source = recipe_source?;
    // If we have raw bytes, hash them; otherwise read from disk.
    let mut hasher = Sha256::new();
    hasher.update(source.to_string_lossy().as_bytes());
    if let Some(bytes) = recipe_bytes {
        hasher.update(bytes);
    } else if source.is_file() {
        let data = std::fs::read(source).ok()?;
        hasher.update(&data);
    } else {
        hasher.update(b"<missing>");
    }

    // Hash ci_paths relative to recipe directory.
    let mut sorted_ci = ci_paths.to_vec();
    sorted_ci.sort();
    for ci in &sorted_ci {
        let candidate = Path::new(ci);
        let resolved = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else if let Some(base) = ci_base {
            base.join(candidate)
        } else {
            source.parent().unwrap_or(Path::new(".")).join(candidate)
        };
        hasher.update(resolved.to_string_lossy().as_bytes());
        if resolved.is_file() {
            if let Ok(data) = std::fs::read(&resolved) {
                hasher.update(&data);
            }
        } else if resolved.is_dir() {
            // Hash sorted file list recursively.
            if let Ok(entries) = collect_files(&resolved) {
                for path in entries {
                    hasher.update(path.to_string_lossy().as_bytes());
                    if let Ok(data) = std::fs::read(&path) {
                        hasher.update(&data);
                    }
                }
            }
        } else {
            hasher.update(b"<missing>");
        }
    }

    let payload = serde_json::json!({
        "renderer": renderer,
        "renderer_argv": renderer_argv,
        "out_dir": out_dir.to_string_lossy(),
        "screen_renderer": screen_renderer,
        "render_video": render_video,
        "video_backend": if render_video { video_backend } else { "" },
        "video_fps": if render_video { video_fps } else { 0 },
    });
    hasher.update(serde_json::to_string(&payload).unwrap().as_bytes());
    Some(hex::encode(hasher.finalize()))
}

fn collect_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_files(&path)?);
        } else if path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

// Minimal hex encoding without extra crate (avoid adding `hex` dep).
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }
}

fn sanitize(value: &str) -> String {
    let s: String = value
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    if s.is_empty() {
        "default".into()
    } else {
        s
    }
}

fn cache_path(cache_dir: &Path, recipe_name: &str, renderer: &str) -> PathBuf {
    cache_dir
        .join(sanitize(recipe_name))
        .join(format!("{}.json", sanitize(renderer)))
}

/// Try to load a cached result. Returns `None` on miss, stale key, or
/// missing artifacts.
pub fn load_cached(
    cache_dir: &Path,
    recipe_name: &str,
    renderer: &str,
    expected_key: &str,
) -> Option<RunResult> {
    let path = cache_path(cache_dir, recipe_name, renderer);
    let data = std::fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&data).ok()?;
    let key = value.get("key")?.as_str()?;
    if key != expected_key {
        return None;
    }
    let result_value = value.get("result")?;
    let mut result: RunResult = serde_json::from_value(result_value.clone()).ok()?;
    if !result.passed {
        return None;
    }
    // Check that core artifacts still exist.
    for k in ["cast", "screenshot", "screen_text"] {
        if let Some(v) = result.artifacts.get(k) {
            if !Path::new(v).exists() {
                return None;
            }
        }
    }
    result.duration_seconds = 0.0;
    let mut artifacts = result.artifacts.clone();
    artifacts.insert("cache".to_string(), path.to_string_lossy().to_string());
    result.artifacts = artifacts;
    Some(result)
}

/// Store a passing result in the cache atomically.
pub fn store_cached(
    cache_dir: &Path,
    recipe_name: &str,
    renderer: &str,
    key: &str,
    result: &RunResult,
) -> std::io::Result<()> {
    if !result.passed {
        return Ok(());
    }
    let path = cache_path(cache_dir, recipe_name, renderer);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let payload = serde_json::json!({ "key": key, "result": result });
    let content = serde_json::to_string_pretty(&payload).unwrap() + "\n";
    crate::store::atomic_write_text(&path, &content)
}

/// Simple cache key helper for tests (hash raw bytes).
pub fn key_from_bytes(
    recipe_name: &str,
    renderer: &str,
    extra: &BTreeMap<String, String>,
    recipe_bytes: &[u8],
) -> String {
    let _ = recipe_name;
    let _ = renderer;
    let mut hasher = Sha256::new();
    hasher.update(recipe_bytes);
    let payload = serde_json::to_value(extra).unwrap();
    hasher.update(serde_json::to_string(&payload).unwrap().as_bytes());
    hex::encode(hasher.finalize())
}
