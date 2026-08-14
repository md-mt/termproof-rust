//! A terminal screen with per-cell attributes, and SVG rendering.
//!
//! [`TerminalScreen`](crate::screen::TerminalScreen) answers "what text is on
//! screen". This answers "what does the screen *look* like": every cell with
//! its foreground, background, bold, italic, underline, reverse and display
//! width, plus a renderer that turns that into SVG.
//!
//! # Why attributes are worth carrying
//!
//! Evidence is the point of this crate, and a monochrome transcript is weak
//! evidence. A recipe asserting that an error rendered in red, or that the
//! selected row was highlighted, has nothing to point at without this. It is
//! also what lets two screens with identical text but a different highlight be
//! recognised as *different* screens — see
//! [`AttributedScreen::render_fingerprint`].
//!
//! # Sources
//!
//! - [`from_vt100`] — a live [`vt100::Screen`], the usual path.
//! - [`attributed_screen_from_ansi_text`] — a captured string that still has
//!   its escape sequences, e.g. `tmux capture-pane -e`.
//! - [`attributed_screen_from_text`] — plain text, rendered in default colours.
//!
//! # Two fidelity gaps, stated plainly
//!
//! `vt100` models neither dim nor strikethrough, so [`AttributedCell::dim`] and
//! [`AttributedCell::strikethrough`] are always `false` on the [`from_vt100`]
//! path. The ANSI-text path parses SGR 2 and SGR 9 and does set them. The
//! fields are kept rather than removed because the SVG renderer honours them
//! and the ANSI path produces them.

use std::sync::OnceLock;

use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use unicode_normalization::char::canonical_combining_class;
use unicode_normalization::UnicodeNormalization;
use unicode_width::UnicodeWidthChar;

/// Default grid width, in columns.
pub const DEFAULT_COLUMNS: usize = 120;
/// Default grid height, in rows.
pub const DEFAULT_ROWS: usize = 40;
/// Cell width in SVG units. With [`DEFAULT_CELL_H`] this gives a ~1:2.2 cell,
/// which is the aspect a real terminal font has.
pub const DEFAULT_CELL_W: f64 = 10.0;
/// Cell height in SVG units. See [`DEFAULT_CELL_W`].
pub const DEFAULT_CELL_H: f64 = 22.0;
/// Font size in SVG units.
pub const DEFAULT_FONT_PX: u32 = 16;
/// Margin around the grid, in SVG units.
pub const DEFAULT_PADDING: u32 = 10;
/// Linux-first monospace stack; the boxes render on a CI host that has
/// neither Menlo nor Consolas, so naming them first just wastes a lookup.
pub const FONT_STACK: &str = "Noto Sans Mono, Liberation Mono, monospace";
/// Foreground used for a cell with no explicit colour.
pub const DEFAULT_FG: &str = "#e6edf3";
/// Background used for a cell with no explicit colour, and for the page.
pub const DEFAULT_BG: &str = "#0b0f14";

/// Named palette entries, tuned to read against [`DEFAULT_BG`].
const COLORS: [(&str, &str); 16] = [
    ("black", "#0b0f14"),
    ("red", "#ff7b72"),
    ("green", "#7ee787"),
    ("brown", "#d29922"),
    ("blue", "#79c0ff"),
    ("magenta", "#d2a8ff"),
    ("cyan", "#56d4dd"),
    ("white", "#e6edf3"),
    ("brightblack", "#6e7681"),
    ("brightred", "#ffa198"),
    ("brightgreen", "#aff5b4"),
    ("brightbrown", "#f2cc60"),
    ("brightblue", "#a5d6ff"),
    ("brightmagenta", "#d2a8ff"),
    ("brightcyan", "#79c0ff"),
    ("brightwhite", "#ffffff"),
];

/// SGR code -> palette name, for the 8 standard and 8 bright foregrounds.
fn ansi_fg(code: u16) -> Option<&'static str> {
    let name = match code {
        30 => "black",
        31 => "red",
        32 => "green",
        33 => "brown",
        34 => "blue",
        35 => "magenta",
        36 => "cyan",
        37 => "white",
        90 => "brightblack",
        91 => "brightred",
        92 => "brightgreen",
        93 => "brightbrown",
        94 => "brightblue",
        95 => "brightmagenta",
        96 => "brightcyan",
        97 => "brightwhite",
        _ => return None,
    };
    Some(name)
}

/// SGR code -> palette name, for the 8 standard and 8 bright backgrounds.
fn ansi_bg(code: u16) -> Option<&'static str> {
    match code {
        40..=47 => ansi_fg(code - 10),
        100..=107 => ansi_fg(code - 10),
        _ => None,
    }
}

const XTERM_256_BASE: [&str; 16] = [
    "000000", "cd0000", "00cd00", "cdcd00", "0000ee", "cd00cd", "00cdcd", "e5e5e5", "7f7f7f",
    "ff0000", "00ff00", "ffff00", "5c5cff", "ff00ff", "00ffff", "ffffff",
];

fn xterm_256_colors() -> &'static [String] {
    static COLORS_256: OnceLock<Vec<String>> = OnceLock::new();
    COLORS_256.get_or_init(|| {
        let mut colors: Vec<String> = XTERM_256_BASE.iter().map(|s| (*s).to_string()).collect();
        const LEVELS: [u32; 6] = [0x00, 0x5F, 0x87, 0xAF, 0xD7, 0xFF];
        for red in LEVELS {
            for green in LEVELS {
                for blue in LEVELS {
                    colors.push(format!("{:02x}{:02x}{:02x}", red, green, blue));
                }
            }
        }
        for index in 0..24 {
            let value = 8 + index * 10;
            colors.push(format!("{:02x}{:02x}{:02x}", value, value, value));
        }
        colors
    })
}

/// Mutable SGR state, carried across a `capture-pane` line boundary because a
/// colour opened on one row stays open on the next.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AnsiAttrs {
    fg: String,
    bg: String,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
    reverse: bool,
}

impl Default for AnsiAttrs {
    fn default() -> Self {
        AnsiAttrs {
            fg: "default".to_string(),
            bg: "default".to_string(),
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            strikethrough: false,
            reverse: false,
        }
    }
}

impl AnsiAttrs {
    fn reset(&mut self) {
        *self = AnsiAttrs::default();
    }

    fn cell(&self, text: &str, width: u8) -> AttributedCell {
        AttributedCell {
            text: text.to_string(),
            fg: self.fg.clone(),
            bg: self.bg.clone(),
            bold: self.bold,
            dim: self.dim,
            italic: self.italic,
            underline: self.underline,
            strikethrough: self.strikethrough,
            reverse: self.reverse,
            width,
        }
    }
}

/// One terminal cell with its display attributes.
///
/// `fg`/`bg` hold the *unresolved* colour: `"default"`, a palette name such as
/// `"brightred"`, or a bare 6-digit hex string. [`cell_colors`] resolves them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributedCell {
    /// The glyph in this cell; a single space when the cell is blank, and
    /// empty for the filler cell that follows a wide glyph.
    pub text: String,
    /// Unresolved foreground colour. See the type-level docs.
    pub fg: String,
    /// Unresolved background colour. See the type-level docs.
    pub bg: String,
    /// SGR 1.
    pub bold: bool,
    /// SGR 2. Always `false` on the [`from_vt100`] path; see the module docs.
    pub dim: bool,
    /// SGR 3.
    pub italic: bool,
    /// SGR 4.
    pub underline: bool,
    /// SGR 9. Always `false` on the [`from_vt100`] path; see the module docs.
    pub strikethrough: bool,
    /// SGR 7 — foreground and background swap when rendered.
    pub reverse: bool,
    /// Display columns: 2 for a wide glyph, 0 for the filler cell that follows
    /// it, 1 otherwise.
    pub width: u8,
}

impl Default for AttributedCell {
    fn default() -> Self {
        AttributedCell {
            text: " ".to_string(),
            fg: "default".to_string(),
            bg: "default".to_string(),
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            strikethrough: false,
            reverse: false,
            width: 1,
        }
    }
}

impl AttributedCell {
    /// A plain cell holding `text`, with default attributes.
    pub fn plain(text: &str) -> Self {
        AttributedCell {
            text: text.to_string(),
            ..Default::default()
        }
    }
}

/// Serializes as a JSON array, matching the Python fingerprint payload.
#[derive(Serialize)]
struct CellFingerprint<'a>(
    &'a str,
    &'a str,
    &'a str,
    bool,
    bool,
    bool,
    bool,
    bool,
    bool,
    u8,
);

/// A rectangular terminal grid plus cursor metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributedScreen {
    /// Cells in row-major order. Rows are not required to be equal length;
    /// [`column_count`](Self::column_count) reports the widest.
    pub rows: Vec<Vec<AttributedCell>>,
    /// Zero-based cursor row.
    pub cursor_row: usize,
    /// Zero-based cursor column.
    pub cursor_column: usize,
    /// Whether the cursor is hidden (DECTCEM reset).
    pub cursor_hidden: bool,
}

impl AttributedScreen {
    /// Number of rows in the grid.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Width of the widest row, in cells.
    pub fn column_count(&self) -> usize {
        self.rows.iter().map(|r| r.len()).max().unwrap_or(0)
    }

    /// The grid as one string per row, dropping attributes.
    ///
    /// `trim_right` strips trailing whitespace from each line, which is what
    /// you want when comparing against expected text: a terminal pads every
    /// row out to the full width.
    pub fn text_lines(&self, trim_right: bool) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| {
                let line: String = row
                    .iter()
                    .filter(|c| c.width > 0)
                    .map(|c| c.text.as_str())
                    .collect();
                if trim_right {
                    line.trim_end().to_string()
                } else {
                    line
                }
            })
            .collect()
    }

    /// [`text_lines`](Self::text_lines) joined with newlines.
    pub fn to_text(&self, trim_right: bool) -> String {
        self.text_lines(trim_right).join("\n")
    }

    /// A digest of every cell *including its attributes*.
    ///
    /// Evidence uses this to skip re-rendering a step whose screen is identical
    /// to the previous one. Attributes are part of the payload because two
    /// screens with the same text but a different highlight are different
    /// screenshots.
    pub fn render_fingerprint(&self) -> String {
        let payload: Vec<Vec<CellFingerprint<'_>>> = self
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|c| {
                        CellFingerprint(
                            &c.text,
                            &c.fg,
                            &c.bg,
                            c.bold,
                            c.dim,
                            c.italic,
                            c.underline,
                            c.strikethrough,
                            c.reverse,
                            c.width,
                        )
                    })
                    .collect()
            })
            .collect();
        let encoded = serde_json::to_string(&payload).unwrap_or_default();
        format!("{:x}", Sha256::digest(encoded.as_bytes()))
    }
}

/// A screen from plain text lines, with default attributes throughout.
pub fn attributed_screen_from_lines(
    lines: &[&str],
    columns: usize,
    rows: usize,
) -> AttributedScreen {
    let screen_rows = lines
        .iter()
        .take(rows)
        .map(|line| {
            line.chars()
                .take(columns)
                .map(|ch| AttributedCell::plain(&ch.to_string()))
                .collect()
        })
        .collect();
    AttributedScreen {
        rows: screen_rows,
        ..Default::default()
    }
}

/// A screen from a plain-text blob, with default attributes throughout.
pub fn attributed_screen_from_text(
    screen_text: &str,
    columns: usize,
    rows: usize,
) -> AttributedScreen {
    let lines: Vec<&str> = screen_text.lines().collect();
    attributed_screen_from_lines(&lines, columns, rows)
}

/// A screen from `tmux capture-pane -e` output: laid-out text plus SGR escapes.
pub fn attributed_screen_from_ansi_text(
    ansi_text: &str,
    columns: usize,
    rows: usize,
) -> AttributedScreen {
    let mut attrs = AnsiAttrs::default();
    let mut lines: Vec<&str> = ansi_text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let screen_rows = lines
        .iter()
        .take(rows)
        .map(|line| cells_from_ansi_line(line.trim_end_matches('\r'), &mut attrs, columns))
        .collect();
    AttributedScreen {
        rows: screen_rows,
        ..Default::default()
    }
}

/// Build an attributed screen from a live `vt100` screen.
///
/// The usual path: a session feeds bytes to `vt100`, and this reads the grid
/// back out with attributes intact.
pub fn from_vt100(screen: &vt100::Screen) -> AttributedScreen {
    let (rows, cols) = screen.size();
    let grid = (0..rows)
        .map(|row| {
            (0..cols)
                .map(|col| match screen.cell(row, col) {
                    Some(cell) => cell_from_vt100(cell),
                    None => AttributedCell::default(),
                })
                .collect()
        })
        .collect();
    let (cursor_row, cursor_column) = screen.cursor_position();
    AttributedScreen {
        rows: grid,
        cursor_row: cursor_row as usize,
        cursor_column: cursor_column as usize,
        cursor_hidden: screen.hide_cursor(),
    }
}

fn cell_from_vt100(cell: &vt100::Cell) -> AttributedCell {
    // A wide glyph occupies two columns; the second is a continuation that
    // carries no text of its own, matching what the SGR parser produces.
    let width: u8 = if cell.is_wide_continuation() {
        0
    } else if cell.is_wide() {
        2
    } else {
        1
    };
    AttributedCell {
        text: if width == 0 {
            String::new()
        } else if cell.has_contents() {
            cell.contents().to_string()
        } else {
            " ".to_string()
        },
        fg: vt100_color(cell.fgcolor()),
        bg: vt100_color(cell.bgcolor()),
        bold: cell.bold(),
        // `vt100` models neither dim nor strikethrough; see the module docs.
        dim: false,
        italic: cell.italic(),
        underline: cell.underline(),
        strikethrough: false,
        reverse: cell.inverse(),
        width,
    }
}

fn vt100_color(color: vt100::Color) -> String {
    match color {
        vt100::Color::Default => "default".to_string(),
        vt100::Color::Idx(index) => palette_color(index as u16),
        vt100::Color::Rgb(r, g, b) => rgb_color(r, g, b),
    }
}

/// Name a palette index: the first 16 are named colours, the rest are hex.
///
/// Public so a caller emulating with something other than `vt100` can build
/// [`AttributedCell`]s without this crate taking a dependency on their
/// emulator. [`crate::attributed`] stays `vt100`-only for that reason.
pub fn palette_color(index: u16) -> String {
    if (index as usize) < COLORS.len() {
        COLORS[index as usize].0.to_string()
    } else {
        xterm_256_color(index)
    }
}

/// Format a true-colour triple the way [`AttributedCell::fg`] expects.
pub fn rgb_color(r: u8, g: u8, b: u8) -> String {
    format!("{r:02x}{g:02x}{b:02x}")
}

/// Resolve a cell's `(foreground, background)` to CSS colours.
pub fn cell_colors(cell: &AttributedCell) -> (String, String) {
    let fg = css_color(&cell.fg, DEFAULT_FG);
    let bg = css_color(&cell.bg, DEFAULT_BG);
    if cell.reverse {
        (bg, fg)
    } else {
        (fg, bg)
    }
}

/// Escape `&`, `<`, `>` for XML text content (mirrors `xml.sax.saxutils.escape`).
pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Canvas geometry for [`screen_svg`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgMetrics {
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
    /// Canvas width, in SVG units.
    ///
    /// Cached, not computed on read: mutating `columns`, `cell_w` or `padding`
    /// leaves this stale until [`recompute`](Self::recompute) is called. It is
    /// a field rather than a method so a caller can override the canvas —
    /// video encoders want a fixed frame size regardless of the grid.
    pub width: usize,
    /// Canvas height, in SVG units. Cached; see [`width`](Self::width).
    pub height: usize,
}

impl Default for SvgMetrics {
    fn default() -> Self {
        let mut metrics = SvgMetrics {
            columns: DEFAULT_COLUMNS,
            rows: DEFAULT_ROWS,
            cell_w: DEFAULT_CELL_W,
            cell_h: DEFAULT_CELL_H,
            font_px: DEFAULT_FONT_PX,
            padding: DEFAULT_PADDING,
            width: 0,
            height: 0,
        };
        metrics.recompute();
        metrics
    }
}

impl SvgMetrics {
    /// Recompute the cached [`width`](Self::width) and [`height`](Self::height)
    /// from the grid and cell metrics.
    ///
    /// Call this after changing `columns`, `rows`, `cell_w`, `cell_h` or
    /// `padding` — including after a `..Default::default()` struct update,
    /// which copies the *default* canvas rather than deriving a new one.
    pub fn recompute(&mut self) {
        self.width = self.derived_width();
        self.height = self.derived_height();
    }

    /// Canvas width implied by the grid and cell metrics.
    pub fn derived_width(&self) -> usize {
        (self.columns as f64 * self.cell_w) as usize + 2 * self.padding as usize
    }

    /// Canvas height implied by the grid and cell metrics.
    pub fn derived_height(&self) -> usize {
        (self.rows as f64 * self.cell_h) as usize + 2 * self.padding as usize
    }
}

/// Render `screen` as an SVG document.
///
/// One `<text>` per cell at `x = column * cell_w`, so a glyph's column is
/// structural rather than dependent on whatever font fontconfig resolves. The
/// previous whole-line `<tspan>` approach let a single substituted glyph shift
/// the rest of the row, which is exactly the fidelity a reviewer is looking at.
pub fn screen_svg(screen: &AttributedScreen, metrics: &SvgMetrics) -> String {
    let mut backgrounds = String::new();
    let mut glyphs = String::new();
    let baseline = metrics.cell_h * 0.72;

    for (r, row) in screen.rows.iter().take(metrics.rows).enumerate() {
        for (c, cell) in row.iter().take(metrics.columns).enumerate() {
            // The filler behind a wide glyph paints nothing of its own.
            if cell.width == 0 {
                continue;
            }
            let (fg, bg) = cell_colors(cell);
            let x = metrics.padding as f64 + c as f64 * metrics.cell_w;
            let y = metrics.padding as f64 + r as f64 * metrics.cell_h;
            // The page already has the default background painted under it.
            if bg != DEFAULT_BG {
                backgrounds.push_str(&format!(
                    "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{}\"/>",
                    x,
                    y,
                    metrics.cell_w * cell.width.max(1) as f64,
                    metrics.cell_h,
                    bg,
                ));
            }
            let text = printable(&cell.text);
            if text == " " || text.is_empty() {
                continue;
            }
            glyphs.push_str(&glyph_svg(cell, &text, x, y + baseline, &fg));
        }
    }

    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" \
viewBox=\"0 0 {w} {h}\"><rect width=\"100%\" height=\"100%\" fill=\"{page_bg}\"/>\
{backgrounds}<g font-family=\"{font}\" font-size=\"{font_px}\" \
xml:space=\"preserve\">{glyphs}</g></svg>",
        w = metrics.width,
        h = metrics.height,
        page_bg = DEFAULT_BG,
        font = FONT_STACK,
        font_px = metrics.font_px,
    )
}

fn glyph_svg(cell: &AttributedCell, text: &str, x: f64, y: f64, fg: &str) -> String {
    let mut attrs = format!("x=\"{:.1}\" y=\"{:.1}\" fill=\"{}\"", x, y, fg);
    if cell.bold {
        attrs.push_str(" font-weight=\"700\"");
    }
    if cell.italic {
        attrs.push_str(" font-style=\"italic\"");
    }
    let mut decorations: Vec<&str> = Vec::new();
    if cell.underline {
        decorations.push("underline");
    }
    if cell.strikethrough {
        decorations.push("line-through");
    }
    if !decorations.is_empty() {
        attrs.push_str(&format!(" text-decoration=\"{}\"", decorations.join(" ")));
    }
    if cell.dim {
        attrs.push_str(" opacity=\"0.65\"");
    }
    format!("<text {}>{}</text>", attrs, xml_escape(text))
}

/// Cell text with everything an XML document cannot carry removed.
///
/// A control character is not a valid XML character at all, and one of them
/// makes `rsvg-convert` reject the whole file — which surfaces as a zero-byte
/// PNG rather than as an error, so a run produces a directory of empty
/// screenshots and says nothing about it.
///
/// A cell should never hold one: `vt100` consumes control bytes rather than
/// storing them. But [`attributed_screen_from_text`] builds a grid straight
/// from a string, escapes and all, and a caller emulating with something other
/// than `vt100` builds [`AttributedCell`]s by hand. The guarantee is therefore
/// enforced here, at the document boundary, where nothing can route around it.
fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

// -- Internals ---------------------------------------------------------------

fn cells_from_ansi_line(line: &str, attrs: &mut AnsiAttrs, columns: usize) -> Vec<AttributedCell> {
    // Indexed by character, not byte: the escape grammar is ASCII but the text
    // around it is not.
    let chars: Vec<char> = line.chars().collect();
    let mut cells: Vec<AttributedCell> = Vec::new();
    let mut index = 0;
    while index < chars.len() && cells.len() < columns {
        if chars[index] == '\x1b' {
            index = consume_ansi_sequence(&chars, index, attrs);
            continue;
        }
        let ch = chars[index];
        if ch == '\t' {
            append_tab(&mut cells, attrs, columns);
        } else if canonical_combining_class(ch) != 0 && !cells.is_empty() {
            let previous = cells.last_mut().expect("checked non-empty");
            let combined: String = format!("{}{}", previous.text, ch).nfc().collect();
            previous.text = combined;
        } else {
            let width = cell_width(ch);
            cells.push(attrs.cell(&ch.to_string(), width));
            if width == 2 && cells.len() < columns {
                // Filler so the grid stays rectangular; skipped when reading text.
                cells.push(attrs.cell("", 0));
            }
        }
        index += 1;
    }
    cells
}

/// Advance past the escape sequence at `index`, applying it if it is an SGR.
fn consume_ansi_sequence(chars: &[char], index: usize, attrs: &mut AnsiAttrs) -> usize {
    if index + 2 >= chars.len() || chars[index + 1] != '[' {
        return index + 1;
    }
    let mut end = index + 2;
    while end < chars.len() && !chars[end].is_alphabetic() {
        end += 1;
    }
    if end >= chars.len() {
        return index + 1;
    }
    if chars[end] == 'm' {
        let params: String = chars[index + 2..end].iter().collect();
        apply_sgr(&parse_sgr_params(&params), attrs);
    }
    end + 1
}

fn parse_sgr_params(params: &str) -> Vec<u16> {
    if params.is_empty() {
        return vec![0];
    }
    let parsed: Vec<u16> = params
        .split([';', ':'])
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse::<u16>().ok())
        .collect();
    if parsed.is_empty() {
        vec![0]
    } else {
        parsed
    }
}

fn apply_sgr(params: &[u16], attrs: &mut AnsiAttrs) {
    let mut index = 0;
    while index < params.len() {
        let code = params[index];
        if let Some(name) = ansi_fg(code) {
            attrs.fg = name.to_string();
        } else if let Some(name) = ansi_bg(code) {
            attrs.bg = name.to_string();
        } else if code == 38 || code == 48 {
            index = apply_extended_color(params, index, attrs);
        } else {
            apply_basic_sgr(code, attrs);
        }
        index += 1;
    }
}

fn apply_basic_sgr(code: u16, attrs: &mut AnsiAttrs) {
    match code {
        0 => attrs.reset(),
        1 => attrs.bold = true,
        2 => attrs.dim = true,
        3 => attrs.italic = true,
        4 => attrs.underline = true,
        7 => attrs.reverse = true,
        9 => attrs.strikethrough = true,
        22 => {
            attrs.bold = false;
            attrs.dim = false;
        }
        23 => attrs.italic = false,
        24 => attrs.underline = false,
        27 => attrs.reverse = false,
        29 => attrs.strikethrough = false,
        39 => attrs.fg = "default".to_string(),
        49 => attrs.bg = "default".to_string(),
        _ => {}
    }
}

/// Apply a `38;…`/`48;…` extended colour and return the index of its last
/// consumed parameter (the caller advances one past).
fn apply_extended_color(params: &[u16], index: usize, attrs: &mut AnsiAttrs) -> usize {
    if index + 1 >= params.len() {
        return index;
    }
    let is_fg = params[index] == 38;
    let mode = params[index + 1];
    if mode == 5 && index + 2 < params.len() {
        let color = xterm_256_color(params[index + 2]);
        if is_fg {
            attrs.fg = color
        } else {
            attrs.bg = color
        }
        return index + 2;
    }
    if mode == 2 && index + 4 < params.len() {
        let color = format!(
            "{:02x}{:02x}{:02x}",
            params[index + 2].min(255),
            params[index + 3].min(255),
            params[index + 4].min(255),
        );
        if is_fg {
            attrs.fg = color
        } else {
            attrs.bg = color
        }
        return index + 4;
    }
    // Truncated sequence: not enough sub-parameters for the declared mode.
    // Consume the rest so the mode indicator is not reinterpreted as a fresh
    // top-level SGR code by the caller's loop.
    params.len() - 1
}

fn append_tab(cells: &mut Vec<AttributedCell>, attrs: &AnsiAttrs, columns: usize) {
    let target = (((cells.len() / 8) + 1) * 8).min(columns);
    while cells.len() < target {
        cells.push(attrs.cell(" ", 1));
    }
}

fn xterm_256_color(index: u16) -> String {
    xterm_256_colors()
        .get(index as usize)
        .cloned()
        .unwrap_or_else(|| "default".to_string())
}

fn cell_width(ch: char) -> u8 {
    match ch.width() {
        Some(2) => 2,
        _ => 1,
    }
}

fn css_color(value: &str, default: &str) -> String {
    if value == "default" {
        return default.to_string();
    }
    let lowered = value.to_lowercase();
    if let Some((_, hex)) = COLORS.iter().find(|(name, _)| *name == lowered) {
        return (*hex).to_string();
    }
    if value.len() == 6 && value.chars().all(|c| c.is_ascii_hexdigit()) {
        return format!("#{}", lowered);
    }
    default.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(ansi: &str) -> AttributedScreen {
        attributed_screen_from_ansi_text(ansi, DEFAULT_COLUMNS, DEFAULT_ROWS)
    }

    #[test]
    fn plain_text_keeps_default_attributes() {
        let s = screen("hi\n");
        assert_eq!(s.to_text(true), "hi");
        assert_eq!(s.rows[0][0], AttributedCell::plain("h"));
    }

    #[test]
    fn sgr_sets_named_foreground() {
        let s = screen("\x1b[31mred\x1b[0m!");
        assert_eq!(s.rows[0][0].fg, "red");
        assert_eq!(s.rows[0][3].fg, "default");
        assert_eq!(s.to_text(true), "red!");
    }

    #[test]
    fn sgr_sets_bright_background() {
        let s = screen("\x1b[103mx");
        assert_eq!(s.rows[0][0].bg, "brightbrown");
    }

    #[test]
    fn attributes_carry_across_lines() {
        // tmux closes a colour on the row it ends, not at the line break.
        let s = screen("\x1b[32mgreen\nstill");
        assert_eq!(s.rows[1][0].fg, "green");
    }

    #[test]
    fn style_codes_toggle() {
        let s = screen("\x1b[1;3;4;9;7mx\x1b[22;23;24;27;29my");
        let styled = &s.rows[0][0];
        assert!(styled.bold && styled.italic && styled.underline);
        assert!(styled.strikethrough && styled.reverse);
        let plain = &s.rows[0][1];
        assert!(!plain.bold && !plain.italic && !plain.underline);
        assert!(!plain.strikethrough && !plain.reverse);
        // 22 clears dim along with bold, and neither survives.
        assert!(!plain.dim);
    }

    #[test]
    fn xterm_256_indexed_color() {
        // 196 is the pure red of the 6x6x6 cube.
        let s = screen("\x1b[38;5;196mx");
        assert_eq!(s.rows[0][0].fg, "ff0000");
        assert_eq!(cell_colors(&s.rows[0][0]).0, "#ff0000");
    }

    #[test]
    fn xterm_256_grayscale_ramp_is_last() {
        assert_eq!(xterm_256_colors().len(), 256);
        assert_eq!(xterm_256_colors()[255], "eeeeee");
    }

    #[test]
    fn truecolor_rgb() {
        let s = screen("\x1b[48;2;1;2;3mx");
        assert_eq!(s.rows[0][0].bg, "010203");
    }

    #[test]
    fn truncated_extended_color_does_not_leak_into_next_code() {
        // "38;5" with no index: the 5 must not be read as a fresh SGR code, and
        // the 1 that follows must not turn the cell bold.
        let s = screen("\x1b[38;5mx");
        assert_eq!(s.rows[0][0].fg, "default");
        let s = screen("\x1b[48;2;9mx");
        assert_eq!(s.rows[0][0].bg, "default");
    }

    #[test]
    fn empty_sgr_params_reset() {
        let s = screen("\x1b[31ma\x1b[mb");
        assert_eq!(s.rows[0][0].fg, "red");
        assert_eq!(s.rows[0][1].fg, "default");
    }

    #[test]
    fn non_sgr_escapes_are_skipped_without_emitting_cells() {
        let s = screen("\x1b[2Kab");
        assert_eq!(s.to_text(true), "ab");
    }

    #[test]
    fn wide_char_gets_a_zero_width_filler() {
        let s = screen("\u{4f60}x");
        assert_eq!(s.rows[0][0].width, 2);
        assert_eq!(s.rows[0][1].width, 0);
        assert_eq!(s.rows[0][2].text, "x");
        // The filler is dropped when reading text back.
        assert_eq!(s.to_text(true), "\u{4f60}x");
    }

    #[test]
    fn combining_mark_merges_into_the_previous_cell() {
        let s = screen("e\u{0301}");
        assert_eq!(s.rows[0].len(), 1);
        assert_eq!(s.rows[0][0].text, "\u{00e9}");
    }

    #[test]
    fn tab_advances_to_the_next_eight_column_stop() {
        let s = screen("ab\tc");
        assert_eq!(s.rows[0].len(), 9);
        assert_eq!(s.rows[0][8].text, "c");
    }

    #[test]
    fn columns_and_rows_are_clipped() {
        let s = attributed_screen_from_ansi_text("abcdef\nghi\njkl", 3, 2);
        assert_eq!(s.row_count(), 2);
        assert_eq!(s.column_count(), 3);
        assert_eq!(s.to_text(false), "abc\nghi");
    }

    #[test]
    fn trailing_newline_does_not_add_a_row() {
        assert_eq!(screen("a\nb\n").row_count(), 2);
    }

    #[test]
    fn from_lines_has_no_attributes() {
        let s = attributed_screen_from_lines(&["ab", "c"], 10, 10);
        assert_eq!(s.to_text(false), "ab\nc");
        assert!(s.rows[0].iter().all(|c| c.fg == "default"));
    }

    #[test]
    fn from_text_splits_lines() {
        assert_eq!(
            attributed_screen_from_text("a\nb", 10, 10).to_text(false),
            "a\nb"
        );
    }

    #[test]
    fn trim_right_strips_padding() {
        let s = screen("hi   ");
        assert_eq!(s.to_text(false), "hi   ");
        assert_eq!(s.to_text(true), "hi");
    }

    #[test]
    fn fingerprint_is_stable_and_attribute_sensitive() {
        let a = screen("\x1b[31mhi");
        let b = screen("\x1b[31mhi");
        let c = screen("\x1b[32mhi");
        let d = screen("ho");
        assert_eq!(a.render_fingerprint(), b.render_fingerprint());
        // Same text, different colour — a different screenshot.
        assert_ne!(a.render_fingerprint(), c.render_fingerprint());
        assert_ne!(a.render_fingerprint(), d.render_fingerprint());
    }

    #[test]
    fn reverse_swaps_resolved_colors() {
        let s = screen("\x1b[7mx");
        let (fg, bg) = cell_colors(&s.rows[0][0]);
        assert_eq!(fg, DEFAULT_BG);
        assert_eq!(bg, DEFAULT_FG);
    }

    #[test]
    fn unknown_color_name_falls_back_to_the_default() {
        assert_eq!(css_color("chartreuse", DEFAULT_FG), DEFAULT_FG);
        assert_eq!(css_color("abcdef", DEFAULT_FG), "#abcdef");
        assert_eq!(css_color("abcde", DEFAULT_FG), DEFAULT_FG);
    }

    // -- SVG rendering ------------------------------------------------------

    fn svg(ansi: &str) -> String {
        let mut metrics = SvgMetrics {
            columns: 10,
            rows: 3,
            ..Default::default()
        };
        metrics.recompute();
        screen_svg(&attributed_screen_from_ansi_text(ansi, 10, 3), &metrics)
    }

    #[test]
    fn escapes_xml_specials() {
        assert_eq!(xml_escape("a<b>&c"), "a&lt;b&gt;&amp;c");
        assert!(svg("a<b").contains("&lt;"));
    }

    #[test]
    fn control_characters_never_reach_the_document() {
        // One of these makes `rsvg-convert` reject the file, and a rejected
        // file surfaces as a zero-byte PNG rather than as an error. The
        // plain-text constructor takes its characters verbatim, so an
        // unparsed escape in the text a caller hands over lands in a cell.
        let screen = attributed_screen_from_text("a\x1b[31mb\x07c", 10, 3);
        let out = screen_svg(&screen, &SvgMetrics::default());
        assert!(
            !out.chars().any(char::is_control),
            "control character survived into the SVG"
        );
        // The printable characters around it are still drawn.
        for glyph in ["a", "b", "c", "3", "1", "m"] {
            assert!(out.contains(&format!(">{glyph}</text>")), "{glyph} missing");
        }
    }

    #[test]
    fn metrics_derive_the_canvas_from_the_grid() {
        let m = SvgMetrics::default();
        assert_eq!(m.width, 120 * 10 + 20);
        assert_eq!(m.height, (40.0 * 22.0) as usize + 20);
    }

    #[test]
    fn each_glyph_is_positioned_by_column() {
        let out = svg("ab");
        // padding 10, cell_w 10 -> columns land on 10.0 and 20.0.
        assert!(out.contains("<text x=\"10.0\" y=\"25.8\" fill=\"#e6edf3\">a</text>"));
        assert!(out.contains("<text x=\"20.0\" y=\"25.8\" fill=\"#e6edf3\">b</text>"));
    }

    #[test]
    fn rows_advance_by_cell_height() {
        let out = svg("a\nb");
        // Both rows sit in column 0; only the baseline moves, by cell_h.
        assert!(out.contains("<text x=\"10.0\" y=\"25.8\" fill=\"#e6edf3\">a</text>"));
        assert!(out.contains("<text x=\"10.0\" y=\"47.8\" fill=\"#e6edf3\">b</text>"));
    }

    #[test]
    fn spaces_emit_no_glyph() {
        let out = svg("a b");
        assert_eq!(out.matches("<text").count(), 2);
        assert!(!out.contains("> </text>"));
    }

    #[test]
    fn default_background_paints_no_per_cell_rect() {
        // Only the full-page background rect.
        assert_eq!(svg("abc").matches("<rect").count(), 1);
    }

    #[test]
    fn non_default_background_paints_a_cell_rect() {
        let out = svg("\x1b[41mx");
        assert_eq!(out.matches("<rect").count(), 2);
        assert!(out.contains("width=\"10.0\" height=\"22.0\" fill=\"#ff7b72\""));
    }

    #[test]
    fn wide_glyph_background_spans_both_columns() {
        let out = svg("\x1b[41m\u{4f60}");
        assert!(out.contains("width=\"20.0\""));
        // The filler cell contributes neither a rect nor a glyph.
        assert_eq!(out.matches("<rect").count(), 2);
        assert_eq!(out.matches("<text").count(), 1);
    }

    #[test]
    fn text_attributes_reach_the_glyph() {
        let out = svg("\x1b[1;2;3;4;9mx");
        assert!(out.contains("font-weight=\"700\""));
        assert!(out.contains("font-style=\"italic\""));
        assert!(out.contains("text-decoration=\"underline line-through\""));
        assert!(out.contains("opacity=\"0.65\""));
    }

    #[test]
    fn reverse_video_swaps_the_painted_colors() {
        let out = svg("\x1b[7mx");
        // Background becomes the default foreground, and the glyph inverts.
        assert!(out.contains(&format!("height=\"22.0\" fill=\"{}\"", DEFAULT_FG)));
        assert!(out.contains(&format!("fill=\"{}\">x</text>", DEFAULT_BG)));
    }

    #[test]
    fn grid_is_clipped_to_the_metrics() {
        let mut metrics = SvgMetrics {
            columns: 2,
            rows: 1,
            ..Default::default()
        };
        metrics.recompute();
        let screen = attributed_screen_from_ansi_text("abcd\nefgh", 10, 5);
        let out = screen_svg(&screen, &metrics);
        assert_eq!(out.matches("<text").count(), 2);
    }
}
