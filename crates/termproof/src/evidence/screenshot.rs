//! Rendering a screen to an image.
//!
//! This takes an [`AttributedScreen`] and emits what the terminal actually
//! showed: per-cell foreground, background, bold, italic, underline, reverse.
//! Reach for it whenever the transport can supply a grid.
//! [`render_svg`](crate::evidence::render::render_svg) is the text-only entry
//! point, for when it cannot — same renderer underneath, but plain text
//! carries no colour to draw. Its sibling
//! [`render_png`](crate::evidence::render::render_png) is *not* the same
//! renderer: it paints blocks, not glyphs, and exists to reach a PNG without
//! `rsvg-convert`. [`evidence::render`](crate::evidence::render)'s module docs
//! have the full table.
//!
//! SVG first, then `rsvg-convert` for the PNG. Going through SVG rather than
//! drawing pixels keeps the output resolution-independent and the intermediate
//! inspectable — when a screenshot looks wrong, the SVG says why and a PNG does
//! not.
//!
//! # The failure this shape prevents
//!
//! An SVG is XML, and C0 control characters are not valid in PCDATA. Emitting
//! through a cell grid means no escape sequence can reach the document.
//! `rsvg-convert` rejects a file that contains one, which surfaces as a
//! zero-byte PNG rather than as an error — an entire run of empty screenshots
//! with nothing to say why. `evidence::render` once wrote its SVG straight
//! from the text and had exactly that hole; it now builds a grid and draws
//! through the same renderer, so both paths are closed.
//! `no_control_characters_reach_the_svg` guards this one.

use std::fs;
use std::io::Write;
use std::process::Command;
use std::time::Duration;

use crate::terminal::attributed::attributed_screen_from_text;
use crate::terminal::attributed::screen_svg;
use crate::terminal::attributed::AttributedScreen;
use crate::terminal::attributed::SvgMetrics;
use crate::terminal::attributed::DEFAULT_CELL_H;
use crate::terminal::attributed::DEFAULT_CELL_W;
use crate::terminal::attributed::DEFAULT_COLUMNS;
use crate::terminal::attributed::DEFAULT_FONT_PX;
use crate::terminal::attributed::DEFAULT_PADDING;
use crate::terminal::attributed::DEFAULT_ROWS;
use crate::terminal::proc::combined_output;
use crate::terminal::proc::run_with_timeout;

const RSVG_CONVERT: &str = "/usr/bin/rsvg-convert";
const TOOL_TIMEOUT: Duration = Duration::from_secs(30);

/// Runs an external rasterizer: `(executable, args, timeout)` -> `Ok(())` or an
/// error message. Injected in tests.
pub type ToolRunner = Box<dyn Fn(&str, &[String], Duration) -> Result<(), String> + Send + Sync>;

/// Default runner: invoke the tool, error on non-zero exit.
pub fn default_runner() -> ToolRunner {
    Box::new(|executable, args, timeout| {
        let mut cmd = Command::new(executable);
        cmd.args(args);
        let output = run_with_timeout(cmd, timeout).map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "{} exited {}: {}",
                executable,
                output.status.code().unwrap_or(-1),
                combined_output(&output),
            ));
        }
        Ok(())
    })
}

/// Renders a captured screen into a PNG image.
pub struct ScreenshotRenderer {
    /// Grid width, in cells.
    pub columns: usize,
    /// Grid height, in cells.
    pub rows: usize,
    /// Cell width, in SVG units.
    pub cell_w: f64,
    /// Cell height, in SVG units.
    pub cell_h: f64,
    /// Font size, in SVG units.
    pub font_px: u32,
    /// Margin around the grid, in SVG units.
    pub padding: u32,
    runner: ToolRunner,
}

impl Default for ScreenshotRenderer {
    fn default() -> Self {
        ScreenshotRenderer {
            columns: DEFAULT_COLUMNS,
            rows: DEFAULT_ROWS,
            cell_w: DEFAULT_CELL_W,
            cell_h: DEFAULT_CELL_H,
            font_px: DEFAULT_FONT_PX,
            padding: DEFAULT_PADDING,
            runner: default_runner(),
        }
    }
}

impl ScreenshotRenderer {
    /// A renderer with the default grid, metrics and rasterisation tool.
    pub fn new() -> Self {
        Self::default()
    }

    /// A default renderer that shells out through `runner` instead of the
    /// real rasteriser. This is the seam the tests use.
    pub fn with_runner(runner: ToolRunner) -> Self {
        ScreenshotRenderer {
            runner,
            ..Self::default()
        }
    }

    /// Canvas geometry, derived from the grid rather than fixed, so a cell
    /// keeps a real terminal aspect (~1:2.2).
    pub fn metrics(&self) -> SvgMetrics {
        let mut metrics = SvgMetrics {
            columns: self.columns,
            rows: self.rows,
            cell_w: self.cell_w,
            cell_h: self.cell_h,
            font_px: self.font_px,
            padding: self.padding,
            width: 0,
            height: 0,
        };
        metrics.recompute();
        metrics
    }

    /// Canvas width, in pixels.
    pub fn width(&self) -> usize {
        self.metrics().width
    }

    /// Canvas height, in pixels.
    pub fn height(&self) -> usize {
        self.metrics().height
    }

    /// Render a screen to a PNG at `png_path` and return the path.
    ///
    /// `attributed_screen` is the captured grid when the transport could supply
    /// one; without it the text is rendered in default colours.
    pub fn render(
        &self,
        screen_text: &str,
        png_path: &str,
        attributed_screen: Option<&AttributedScreen>,
    ) -> Result<String, String> {
        if let Some(parent) = std::path::Path::new(png_path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let fallback;
        let screen = match attributed_screen {
            Some(screen) => screen,
            None => {
                fallback = attributed_screen_from_text(screen_text, self.columns, self.rows);
                &fallback
            }
        };
        let svg = screen_svg(screen, &self.metrics());

        let mut tmp = tempfile::Builder::new()
            .prefix("termproof_shot_")
            .suffix(".svg")
            .tempfile()
            .map_err(|e| e.to_string())?;
        tmp.write_all(svg.as_bytes()).map_err(|e| e.to_string())?;
        let svg_path = tmp.path().to_string_lossy().to_string();

        let result = (self.runner)(
            RSVG_CONVERT,
            &["--output".to_string(), png_path.to_string(), svg_path],
            TOOL_TIMEOUT,
        );
        // tmp is dropped (deleted) here.
        result?;
        Ok(png_path.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::sync::Mutex;

    use super::*;
    use crate::terminal::attributed::attributed_screen_from_ansi_text;
    use crate::terminal::attributed::DEFAULT_BG;
    use crate::terminal::attributed::DEFAULT_FG;

    /// A renderer whose runner records the SVG handed to `rsvg-convert`.
    fn capturing_renderer() -> (ScreenshotRenderer, Arc<Mutex<String>>) {
        let svg = Arc::new(Mutex::new(String::new()));
        let sink = svg.clone();
        let renderer = ScreenshotRenderer::with_runner(Box::new(move |_, args, _| {
            *sink.lock().expect("poisoned") = fs::read_to_string(&args[2]).expect("svg written");
            Ok(())
        }));
        (renderer, svg)
    }

    fn render_to_svg(renderer: &ScreenshotRenderer, text: &str, screen: Option<&AttributedScreen>) {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("out.png").to_string_lossy().to_string();
        renderer.render(text, &png, screen).unwrap();
    }

    #[test]
    fn render_invokes_runner_and_writes_svg() {
        let called = Arc::new(AtomicBool::new(false));
        let called2 = called.clone();
        let renderer = ScreenshotRenderer::with_runner(Box::new(move |exe, args, _| {
            called2.store(true, Ordering::SeqCst);
            assert_eq!(exe, RSVG_CONVERT);
            // args: --output <png> <svg>
            assert_eq!(args[0], "--output");
            assert!(args[2].ends_with(".svg"));
            Ok(())
        }));
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("out.png").to_string_lossy().to_string();
        let path = renderer.render("hello\nworld", &png, None).unwrap();
        assert_eq!(path, png);
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn render_propagates_runner_error() {
        let renderer = ScreenshotRenderer::with_runner(Box::new(|_, _, _| Err("boom".to_string())));
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("out.png").to_string_lossy().to_string();
        assert!(renderer.render("x", &png, None).is_err());
    }

    #[test]
    fn plain_text_renders_in_default_colors() {
        let (renderer, svg) = capturing_renderer();
        render_to_svg(&renderer, "hi", None);
        let svg = svg.lock().unwrap().clone();
        assert!(svg.contains(&format!("fill=\"{}\">h</text>", DEFAULT_FG)));
        // No per-cell background rect, only the page.
        assert_eq!(svg.matches("<rect").count(), 1);
    }

    #[test]
    fn attributed_screen_wins_over_the_text() {
        let (renderer, svg) = capturing_renderer();
        let screen = attributed_screen_from_ansi_text("\x1b[31mZ", DEFAULT_COLUMNS, DEFAULT_ROWS);
        // The text argument says something else; the grid is what gets drawn.
        render_to_svg(&renderer, "hi", Some(&screen));
        let svg = svg.lock().unwrap().clone();
        assert!(svg.contains("fill=\"#ff7b72\">Z</text>"));
        assert!(!svg.contains(">h</text>"));
    }

    #[test]
    fn no_control_characters_reach_the_svg() {
        // The failure the module doc describes: one control character and
        // `rsvg-convert` rejects the document, which arrives as a zero-byte
        // PNG rather than as an error. The screen text a caller passes when it
        // has no attributed grid is the way one gets in.
        let (renderer, svg) = capturing_renderer();
        render_to_svg(&renderer, "ok \x1b[31mred\x1b[0m\x07 done", None);
        let svg = svg.lock().unwrap().clone();
        assert!(
            !svg.chars().any(char::is_control),
            "control character reached the SVG"
        );
    }

    #[test]
    fn canvas_is_derived_from_the_grid() {
        let renderer = ScreenshotRenderer::new();
        assert_eq!(renderer.width(), 120 * 10 + 20);
        assert_eq!(renderer.height(), (40.0 * 22.0) as usize + 20);
        let (renderer, svg) = capturing_renderer();
        render_to_svg(&renderer, "hi", None);
        let svg = svg.lock().unwrap().clone();
        assert!(svg.contains("width=\"1220\" height=\"900\""));
        assert!(svg.contains(&format!("fill=\"{}\"", DEFAULT_BG)));
    }
}
