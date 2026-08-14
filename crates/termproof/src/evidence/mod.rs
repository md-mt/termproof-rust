//! Evidence pipeline: rendering, reports, video, baselines, diff,
//! and cache.
//!
//! Every still and every video frame is drawn by
//! [`screen_svg`](crate::terminal::attributed::screen_svg), so all the
//! evidence from one run shares a visual language. The entry points differ
//! only in what they take: [`render`] takes plain text, [`screenshot`] takes
//! an attributed grid, [`cast_video`] takes a cast. [`render`]'s module docs
//! carry the table. The exception is [`AggFfmpegBackend`], which shells out to
//! `agg` and brings its own fonts and palette — see [`cast_video`] for the
//! consistent alternative.

pub mod cast_video;
pub mod dedup;
pub mod diff;
pub mod render;
pub mod report;
pub mod screenshot;
pub mod uploader;
pub mod video;

pub use diff::apply_visual_diff;
pub use render::{normalize_text, render_by_extension, render_png, render_svg};
pub use report::{
    generate_junit, generate_markdown, generate_markdown_single, validate_duration,
    validate_recipe_json,
};
pub use video::{
    render_mp4, render_mp4_or_fail, resolve_agg, resolve_ffmpeg, AggFfmpegBackend, VideoBackend,
    VideoError,
};
