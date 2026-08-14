//! Video backend — agg + ffmpeg adapter with loud missing-video failure.
//!
//! Mirrors `termproof/builtin_video.py` and `termproof/evidence.py:render_mp4`
//! but fixes #77: a requested video whose tools are missing is an explicit
//! failure, not a silent no-op.

use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
/// Video backend error.
pub enum VideoError {
    #[error("agg not found: {0}")]
    /// `agg` binary is missing.
    AggMissing(String),
    #[error("ffmpeg not found: {0}")]
    /// `ffmpeg` binary is missing.
    FfmpegMissing(String),
    #[error("video render failed: {0}")]
    /// Rendering command failed.
    RenderFailed(String),
}

/// Trait for video backends (object-safe for plugin use).
pub trait VideoBackend: Send + Sync {
    /// Human-readable backend name.
    fn name(&self) -> &str;
    /// Render `cast_path` to `output_path` at `fps`.  Returns error on failure.
    fn render(&self, cast_path: &Path, output_path: &Path, fps: u32) -> Result<(), VideoError>;
}

/// Default agg + ffmpeg backend (the `agg_ffmpeg` backend).
pub struct AggFfmpegBackend;

impl VideoBackend for AggFfmpegBackend {
    fn name(&self) -> &str {
        "agg_ffmpeg"
    }

    fn render(&self, cast_path: &Path, output_path: &Path, fps: u32) -> Result<(), VideoError> {
        render_mp4(cast_path, output_path, fps)
    }
}

/// Resolve `agg` binary location.
pub fn resolve_agg() -> Option<PathBuf> {
    // Check AGg env override first (used in tests with a sentinel).
    if let Ok(p) = std::env::var("TERM_PROOF_AGG") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    // Search PATH.
    let which = which_agg();
    if let Some(p) = which {
        return Some(p);
    }
    None
}

fn which_agg() -> Option<PathBuf> {
    let path_var = std::env::var("PATH").unwrap_or_default();
    for dir in path_var.split(':') {
        let candidate = Path::new(dir).join("agg");
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let candidate_exe = Path::new(dir).join("agg.exe");
            if candidate_exe.is_file() {
                return Some(candidate_exe);
            }
        }
    }
    None
}

/// Resolve `ffmpeg` binary location.
pub fn resolve_ffmpeg() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TERM_PROOF_FFMPEG") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    // Try `which ffmpeg`.
    let path_var = std::env::var("PATH").unwrap_or_default();
    for dir in path_var.split(':') {
        let candidate = Path::new(dir).join("ffmpeg");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Render MP4 via agg → GIF → ffmpeg.
///
/// This is the free function used by `AggFfmpegBackend`.  Callers that want
/// the loud-failure contract should use `render_mp4_or_fail` when
/// `render_video` was explicitly requested.
pub fn render_mp4(cast_path: &Path, mp4_path: &Path, fps: u32) -> Result<(), VideoError> {
    let agg = resolve_agg().ok_or_else(|| {
        VideoError::AggMissing(
            "agg not found in PATH and TERM_PROOF_AGG not set; install https://github.com/asciinema/agg".to_string(),
        )
    })?;
    let ffmpeg = resolve_ffmpeg().ok_or_else(|| {
        VideoError::FfmpegMissing(
            "ffmpeg not found in PATH and TERM_PROOF_FFMPEG not set; install ffmpeg".to_string(),
        )
    })?;
    let gif_path = mp4_path.with_extension("agg.gif");
    // agg → gif
    let status = Command::new(&agg)
        .args(["--quiet", "--fps-cap", &fps.to_string()])
        .arg(cast_path)
        .arg(&gif_path)
        .status()
        .map_err(|e| VideoError::RenderFailed(format!("failed to launch agg: {e}")))?;
    if !status.success() {
        return Err(VideoError::RenderFailed(format!(
            "agg exited with {status}"
        )));
    }
    // gif → mp4
    let status = Command::new(&ffmpeg)
        .args(["-y", "-loglevel", "error", "-i"])
        .arg(&gif_path)
        .args([
            "-vf",
            &format!("fps={fps},scale=trunc(iw/2)*2:trunc(ih/2)*2"),
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
        ])
        .arg(mp4_path)
        .status()
        .map_err(|e| VideoError::RenderFailed(format!("failed to launch ffmpeg: {e}")))?;
    let _ = std::fs::remove_file(&gif_path);
    if !status.success() {
        return Err(VideoError::RenderFailed(format!(
            "ffmpeg exited with {status}"
        )));
    }
    Ok(())
}

/// Loud-failure wrapper: when `render_video` was requested but tools are
/// missing, return an error instead of silently succeeding (fixes #77).
pub fn render_mp4_or_fail(
    cast_path: &Path,
    mp4_path: &Path,
    fps: u32,
    render_video: bool,
) -> Result<Option<PathBuf>, VideoError> {
    if !render_video {
        return Ok(None);
    }
    // When video is explicitly requested, missing tools are a hard error.
    render_mp4(cast_path, mp4_path, fps)?;
    Ok(Some(mp4_path.to_path_buf()))
}
