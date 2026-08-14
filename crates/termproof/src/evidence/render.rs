//! Evidence rendering: plain screen text to an image file.
//!
//! # Which renderer do I want?
//!
//! | I have | I want | Use | Draws through [`screen_svg`] |
//! | --- | --- | --- | --- |
//! | plain text | an SVG file | [`render_svg`] | yes |
//! | an [`AttributedScreen`] | a PNG file | [`ScreenshotRenderer`](crate::evidence::screenshot::ScreenshotRenderer) | yes |
//! | an [`AttributedScreen`] | an SVG document, in memory | [`screen_svg`] | — |
//! | a cast | an MP4 | [`CastVideoConverter`](crate::evidence::cast_video::CastVideoConverter) | yes |
//! | plain text | a PNG file, with no external tool | [`render_png`] | **no** |
//! | plain text | either, chosen by file suffix | [`render_by_extension`] | for `.svg` |
//!
//! Everything in the `yes` rows is the same visual language — same fonts,
//! palette, cell metrics and document structure — so those entry points differ
//! in what they take, not in how they look. Three caveats.
//!
//! [`render_png`] is a block renderer, not a glyph renderer — it bundles no
//! font, so it paints a rectangle per occupied cell. It agrees with
//! [`render_svg`] on canvas size, grid and palette, and that is all it agrees
//! on; the shapes are not letters. It exists because it needs no external
//! binary, where a real PNG has to go out to `rsvg-convert`. If you want a PNG
//! that looks like the SVG, rasterise one — that is
//! [`ScreenshotRenderer`](crate::evidence::screenshot::ScreenshotRenderer).
//!
//! The MP4 row holds for `CastVideoConverter` and not for the default video
//! backend, which is still `agg_ffmpeg` and brings its own fonts and palette.
//! See [`evidence`](crate::evidence).
//!
//! Even `CastVideoConverter` matches a still in appearance rather than in
//! column layout: cast playback goes through `avt`, which measures widths with
//! `unicode-width` 0.1 where the rest of the crate uses 0.2, so a character the
//! two tables classify differently sits in a different column in a frame than
//! in a still. [`cast_video`](crate::evidence::cast_video) explains why that
//! trade is the right one.
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
/// [`screen_svg`], so on the same grid the document is what
/// [`ScreenshotRenderer`](crate::evidence::screenshot::ScreenshotRenderer)
/// hands `rsvg-convert`, byte for byte but for the trailing newline this adds.
/// `svg_is_what_the_screenshot_renderer_rasterises` pins that. See the module
/// docs for which renderer to reach for.
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
/// A block per occupied cell rather than glyphs, on the canvas, grid and
/// palette [`render_svg`] uses — and nothing beyond those three, which is what
/// `png_shares_the_svg_canvas_grid_and_palette_and_nothing_else` pins. This
/// does not draw through [`screen_svg`]; it is the PNG you can have without
/// `rsvg-convert`. See the module docs for which renderer to reach for.
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

/// Unified entry point: render `text` to `path` using extension to select
/// renderer.
///
/// A `.png` suffix picks [`render_png`], which is a different renderer rather
/// than the same picture in another container — see the module docs. Anything
/// else picks [`render_svg`].
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
    use std::sync::Arc;
    use std::sync::Mutex;

    use super::*;
    use crate::terminal::attributed::attributed_screen_from_ansi_text;
    use crate::terminal::attributed::DEFAULT_COLUMNS;
    use crate::terminal::attributed::DEFAULT_FG;
    use crate::terminal::attributed::DEFAULT_ROWS;

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
        // The whole point of #19: one renderer behind both SVG entry points.
        let text = "$ ls\nREADME.md";
        let expected = screen_svg(&attributed_screen_from_text(text, 80, 24), &metrics(80, 24));
        assert_eq!(rendered(text, 80, 24), expected + "\n");
    }

    #[test]
    fn svg_is_what_the_screenshot_renderer_rasterises() {
        // The claim on `render_svg` is about `ScreenshotRenderer`, so compare
        // against what that actually hands the rasteriser rather than against
        // `screen_svg` directly — otherwise the doc outruns the test.
        let text = "$ ls\nREADME.md 你x";
        let captured = Arc::new(Mutex::new(String::new()));
        let sink = captured.clone();
        let shot = crate::evidence::screenshot::ScreenshotRenderer::with_runner(Box::new(
            move |_, args: &[String], _| {
                *sink.lock().expect("poisoned") =
                    std::fs::read_to_string(&args[2]).expect("svg written");
                Ok(())
            },
        ));
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("a.png").to_string_lossy().to_string();
        shot.render(text, &png, None).unwrap();

        // `ScreenshotRenderer`'s default grid, so the two are comparable.
        let ours = rendered(text, DEFAULT_COLUMNS as u16, DEFAULT_ROWS as u16);
        let theirs = captured.lock().expect("poisoned").clone();
        assert_eq!(ours, theirs.clone() + "\n");
        assert_ne!(theirs, "", "the runner never saw an SVG");
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
    fn png_shares_the_svg_canvas_grid_and_palette_and_nothing_else() {
        // This is the whole contract, so it is worth stating in full: what the
        // PNG shares is the canvas, the cell grid and the palette. It is a
        // block renderer — the shapes are not letters — and a caller who needs
        // the glyphs wants `ScreenshotRenderer`, not this.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.png");
        render_png("hi 你", &path, 80, 24).unwrap();
        let img = image::open(&path).unwrap().to_rgb8();
        let m = metrics(80, 24);
        let at = |col: usize, row: usize| {
            let x = m.padding + (col as f64 * m.cell_w) as u32 + 1;
            let y = m.padding + (row as f64 * m.cell_h) as u32 + 1;
            img.get_pixel(x, y).0
        };

        // Canvas: the same one `render_svg` declares.
        assert_eq!(
            (img.width(), img.height()),
            (m.width as u32, m.height as u32)
        );
        assert!(rendered("hi 你", 80, 24)
            .contains(&format!("width=\"{}\" height=\"{}\"", m.width, m.height)));
        // Palette: default background outside the grid, default foreground on
        // an occupied cell.
        assert_eq!(img.get_pixel(0, 0).0, rgb(DEFAULT_BG));
        assert_eq!(at(0, 0), rgb(DEFAULT_FG));
        // Grid: cells are placed at `cell_w` intervals, a blank cell stays
        // background, and a wide glyph covers both of its columns.
        assert_eq!(at(1, 0), rgb(DEFAULT_FG));
        assert_eq!(at(2, 0), rgb(DEFAULT_BG), "the space should be blank");
        assert_eq!(at(3, 0), rgb(DEFAULT_FG));
        assert_eq!(at(4, 0), rgb(DEFAULT_FG), "wide glyph lost its second cell");
        assert_eq!(at(5, 0), rgb(DEFAULT_BG));
        assert_eq!(at(0, 1), rgb(DEFAULT_BG), "row 1 is empty");
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
