//! Evidence pipeline: rendering, reports, video, baselines, diff,
//! and cache.
//!
//! Every SVG still is drawn by
//! [`screen_svg`](crate::terminal::attributed::screen_svg), so those stills
//! share a visual language and the entry points differ only in what they take:
//! [`render_svg`] takes plain text, [`screenshot`] takes an attributed grid.
//!
//! Two things sit outside that, and both are deliberate.
//! [`render_png`] bundles no font, so it paints a block per occupied cell
//! rather than glyphs; it shares the canvas, grid and palette and nothing
//! else, and it is the only way to a PNG without `rsvg-convert`.
//! [`AggFfmpegBackend`] shells out to `agg`, which brings its own fonts and
//! palette, and it is still the default video backend — so a video from a
//! default run does not match the stills beside it. [`cast_video`] is the
//! backend that does draw frames through
//! [`screen_svg`](crate::terminal::attributed::screen_svg) — matching a still
//! in appearance, though not always in column layout, since cast playback
//! measures widths with a different `unicode-width` major.
//!
//! [`render`]'s module docs carry the full table.

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
