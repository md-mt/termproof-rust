//! Evidence rendering: plain screen text to an image file.
//!
//! # Which renderer do I want?
//!
//! Several entry points turn a screen into a picture, and they differ in what
//! they take, not in how they look — every one of them draws through
//! [`screen_svg`], so a still from any of them is the same visual language.
//!
//! | I have | I want | Use |
//! | --- | --- | --- |
//! | plain text | an SVG or PNG file | [`render_by_extension`], or [`render_svg`] / [`render_png`] directly |
//! | an [`AttributedScreen`] | a PNG file | [`ScreenshotRenderer`](crate::evidence::screenshot::ScreenshotRenderer) |
//! | an [`AttributedScreen`] | an SVG document, in memory | [`screen_svg`] |
//! | a cast | an MP4 | [`CastVideoConverter`](crate::evidence::cast_video::CastVideoConverter) |
//!
//! Reach for this module only when all you have is text. It renders in the
//! default foreground on the default background, because plain text carries no
//! colour — it cannot recover what the terminal actually showed. When the
//! transport can supply an [`AttributedScreen`], render that instead and the
//! evidence keeps its colours.
//!
//! [`AttributedScreen`]: crate::terminal::attributed::AttributedScreen

use std::io::Write;
use std::path::Path;

use crate::terminal::attributed::attributed_screen_from_text;
use crate::terminal::attributed::cell_colors;
use crate::terminal::attributed::screen_svg;
use crate::terminal::attributed::AttributedScreen;
use crate::terminal::attributed::SvgMetrics;
use crate::terminal::attributed::DEFAULT_BG;

// --- text -------------------------------------------------------------------

/// Normalize screen text: trim trailing whitespace per line, drop trailing empty lines.
pub fn normalize_text(text: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(|l| l.trim_end().to_string()).collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

// --- SVG --------------------------------------------------------------------

/// Render `text` to SVG at `output_path` on a `cols`×`rows` grid.
///
/// The text becomes a default-coloured [`AttributedScreen`] and is drawn by
/// [`screen_svg`], so the document is identical to what
/// [`ScreenshotRenderer`](crate::evidence::screenshot::ScreenshotRenderer)
/// produces for the same text. See the module docs for which renderer to reach
/// for.
pub fn render_svg(text: &str, output_path: &Path, cols: u16, rows: u16) -> std::io::Result<()> {
    let screen = screen_from_text(text, cols, rows);
    let content = screen_svg(&screen, &metrics(cols, rows)) + "\n";
    // Atomic write via temp file in same dir.
    atomic_write(output_path, content.as_bytes())
}

/// Canvas geometry for a `cols`×`rows` grid, at the shared cell metrics.
fn metrics(cols: u16, rows: u16) -> SvgMetrics {
    let mut metrics = SvgMetrics {
        columns: cols as usize,
        rows: rows as usize,
        // `..Default::default()` copies the *default* canvas, hence recompute.
        ..Default::default()
    };
    metrics.recompute();
    metrics
}

fn screen_from_text(text: &str, cols: u16, rows: u16) -> AttributedScreen {
    attributed_screen_from_text(text, cols as usize, rows as usize)
}

// --- PNG --------------------------------------------------------------------

// This does not bundle a TTF, so it cannot draw glyphs; it draws a block per
// occupied cell instead. What it does guarantee is that the canvas, the grid
// and the palette are the ones `render_svg` uses, so the two agree on
// everything except glyph shape and a visual diff that compares sizes stays
// meaningful. Where fidelity matters the SVG is the canonical still, and
// `ScreenshotRenderer` rasterises it properly through `rsvg-convert`.

/// Render `text` to PNG at `output_path` on a `cols`×`rows` grid.
///
/// A block per occupied cell rather than glyphs, on the same canvas and
/// palette [`render_svg`] uses. See the module docs for which renderer to
/// reach for.
pub fn render_png(text: &str, output_path: &Path, cols: u16, rows: u16) -> std::io::Result<()> {
    let metrics = metrics(cols, rows);
    let width = metrics.width as u32;
    let height = metrics.height as u32;

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut img = image::RgbImage::new(width, height);
    for pixel in img.pixels_mut() {
        *pixel = image::Rgb(rgb(DEFAULT_BG));
    }

    // Inset so adjacent blocks stay distinguishable rather than fusing into a
    // solid bar, which is the only thing a reviewer can read off this image.
    let block_w = (metrics.cell_w * 0.7).round() as u32;
    let block_h = (metrics.cell_h * 0.6).round() as u32;
    let screen = screen_from_text(text, cols, rows);
    for (row_idx, row) in screen.rows.iter().take(metrics.rows).enumerate() {
        let y_base = metrics.padding + (row_idx as f64 * metrics.cell_h) as u32;
        for (col_idx, cell) in row.iter().take(metrics.columns).enumerate() {
            if cell.width == 0 || cell.text.trim().is_empty() {
                continue;
            }
            let (fg, _) = cell_colors(cell);
            let color = image::Rgb(rgb(&fg));
            let x = metrics.padding + (col_idx as f64 * metrics.cell_w) as u32;
            let span = block_w + metrics.cell_w as u32 * (cell.width as u32 - 1);
            for dy in 0..block_h {
                for dx in 0..span {
                    let px = x + dx;
                    let py = y_base + dy;
                    if px < width && py < height {
                        img.put_pixel(px, py, color);
                    }
                }
            }
        }
    }

    // Atomic write: save to temp then rename.
    let dir = output_path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(std::io::Error::other)?;
    img.write_to(&mut tmp, image::ImageFormat::Png)
        .map_err(std::io::Error::other)?;
    tmp.flush()?;
    match tmp.persist(output_path) {
        Ok(_) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(output_path);
            e.file
                .persist(output_path)
                .map(|_| ())
                .map_err(|e2| std::io::Error::other(e2.error))
        }
    }
}

// --- helpers ----------------------------------------------------------------

/// A `#rrggbb` CSS colour as an RGB triple. [`cell_colors`] resolves every
/// cell to that form, so the black fallback exists only to keep this total.
fn rgb(css: &str) -> [u8; 3] {
    let hex = css.strip_prefix('#').unwrap_or(css);
    if hex.len() != 6 {
        return [0, 0, 0];
    }
    u32::from_str_radix(hex, 16)
        .map(|v| [(v >> 16) as u8, (v >> 8) as u8, v as u8])
        .unwrap_or([0, 0, 0])
}

fn atomic_write(dest: &Path, content: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let dir = dest.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(content)?;
    tmp.flush()?;
    match tmp.persist(dest) {
        Ok(_) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(dest);
            e.file.persist(dest).map(|_| ()).map_err(|e2| e2.error)
        }
    }
}

/// Unified entry point: render `text` to `path` using extension to select renderer.
pub fn render_by_extension(text: &str, path: &Path, cols: u16, rows: u16) -> std::io::Result<()> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("svg")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => render_png(text, path, cols, rows),
        _ => render_svg(text, path, cols, rows),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::attributed::attributed_screen_from_ansi_text;
    use crate::terminal::attributed::DEFAULT_FG;

    fn rendered(text: &str, cols: u16, rows: u16) -> String {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.svg");
        render_svg(text, &path, cols, rows).unwrap();
        std::fs::read_to_string(&path).unwrap()
    }

    #[test]
    fn normalize_text_trims_lines_and_trailing_blanks() {
        assert_eq!(normalize_text("a  \nb\t\n\n\n"), "a\nb");
    }

    #[test]
    fn svg_is_the_document_the_attributed_renderer_would_have_produced() {
        // The whole point of #19: one renderer behind both entry points.
        let text = "$ ls\nREADME.md";
        let expected = screen_svg(&attributed_screen_from_text(text, 80, 24), &metrics(80, 24));
        assert_eq!(rendered(text, 80, 24), expected + "\n");
    }

    #[test]
    fn a_wide_glyph_occupies_two_columns_here_too() {
        // Sharing the renderer is not enough on its own: the grid handed to it
        // has to be laid out the same way. A text path that gives every scalar
        // one column draws `x` on top of the second half of the wide glyph,
        // and the still still does not match the attributed one.
        let text = "你x";
        let attributed = screen_svg(
            &attributed_screen_from_ansi_text(text, 80, 24),
            &metrics(80, 24),
        );
        assert_eq!(rendered(text, 80, 24), attributed + "\n");
        // Stated in coordinates as well, so the assertion above cannot pass by
        // both paths being wrong together: cell 0 is wide, so `x` is at cell 2.
        assert!(rendered(text, 80, 24).contains("<text x=\"30.0\" y=\"25.8\""));
    }

    #[test]
    fn canvas_is_derived_from_the_grid() {
        // No floor: a 20x5 grid gets a 20x5 canvas, where the previous
        // renderer clamped every small screen up to 320x160.
        assert!(rendered("x", 20, 5).contains("width=\"220\" height=\"130\""));
        assert!(rendered("x", 120, 40).contains("width=\"1220\" height=\"900\""));
    }

    #[test]
    fn control_characters_never_reach_the_document() {
        // A rejected SVG surfaces as a zero-byte PNG rather than an error, so
        // an unparsed escape in the caller's text used to produce a directory
        // of empty screenshots. See `evidence::screenshot`.
        let out = rendered("ok \x1b[31mred\x1b[0m\x07 done", 80, 24);
        assert!(
            !out.chars().any(|c| c.is_control() && c != '\n'),
            "control character reached the SVG"
        );
    }

    #[test]
    fn glyphs_are_positioned_per_cell_not_per_line() {
        let out = rendered("ab", 10, 3);
        assert!(out.contains(&format!(
            "<text x=\"10.0\" y=\"25.8\" fill=\"{DEFAULT_FG}\">a</text>"
        )));
        assert!(out.contains(&format!(
            "<text x=\"20.0\" y=\"25.8\" fill=\"{DEFAULT_FG}\">b</text>"
        )));
    }

    #[test]
    fn the_grid_is_clipped_to_cols_and_rows() {
        let out = rendered("abcd\nefgh\nijkl", 2, 2);
        assert_eq!(out.matches("<text").count(), 4);
    }

    #[test]
    fn text_carries_no_colour_so_every_cell_is_the_default() {
        // Plain text cannot say what the terminal showed. A caller that wants
        // the red back has to hand over an attributed screen.
        let out = rendered("\x1b[31mERROR", 80, 24);
        assert!(!out.contains("#ff7b72"));
        let coloured = attributed_screen_from_ansi_text("\x1b[31mERROR", 80, 24);
        assert!(screen_svg(&coloured, &metrics(80, 24)).contains("#ff7b72"));
    }

    #[test]
    fn png_shares_the_svg_canvas_and_palette() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.png");
        render_png("hi", &path, 80, 24).unwrap();
        let img = image::open(&path).unwrap().to_rgb8();
        let m = metrics(80, 24);
        assert_eq!(
            (img.width(), img.height()),
            (m.width as u32, m.height as u32)
        );
        assert_eq!(img.get_pixel(0, 0).0, rgb(DEFAULT_BG));
        // First cell of the first row is occupied, so its block is painted.
        assert_eq!(
            img.get_pixel(m.padding + 1, m.padding + 1).0,
            rgb(DEFAULT_FG)
        );
    }

    #[test]
    fn render_by_extension_selects_on_the_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let svg = dir.path().join("a.SVG");
        let png = dir.path().join("a.png");
        let bare = dir.path().join("a");
        for path in [&svg, &png, &bare] {
            render_by_extension("hi", path, 10, 3).unwrap();
        }
        assert!(std::fs::read_to_string(&svg).unwrap().starts_with("<svg"));
        assert!(std::fs::read_to_string(&bare).unwrap().starts_with("<svg"));
        assert_eq!(&std::fs::read(&png).unwrap()[1..4], b"PNG");
    }

    #[test]
    fn writing_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/deeper/out.svg");
        render_svg("hi", &path, 10, 3).unwrap();
        assert!(path.exists());
    }
}
