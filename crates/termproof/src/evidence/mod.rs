//! Evidence pipeline: rendering, reports, video, baselines, diff,
//! and cache.
//!
//! Every still is drawn by
//! [`screen_svg`](crate::terminal::attributed::screen_svg), so the stills from
//! one run share a visual language. The entry points differ only in what they
//! take: [`render`] takes plain text, [`screenshot`] takes an attributed grid.
//! [`render`]'s module docs carry the table.
//!
//! Video is only partly inside that. [`cast_video`] draws its frames through
//! the same renderer; [`AggFfmpegBackend`] shells out to `agg`, which brings
//! its own fonts and palette, and it is still the default backend — so a video
//! from a default run does not match the stills beside it.

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
