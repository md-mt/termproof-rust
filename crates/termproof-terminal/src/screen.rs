//! Terminal screen emulation (RUST-006).
//!
//! Provides an in-memory view of what a user would see on a terminal, backed
//! by the `vt100` terminal emulator.
//!
//! The screen is a fixed `cols` x `rows` cell grid, not an append-only
//! transcript. Escape sequences are *interpreted*, so erase, cursor
//! addressing, scroll regions and the alternate buffer all behave as they do
//! on a real terminal — in particular, text that is erased or overwritten
//! stops being visible. That non-monotonicity is the whole point: assertions
//! of the form "X is no longer on screen" are only meaningful against an
//! emulator.
//!
//! `contents()` reproduces the Python `screen_text` shape: one entry per
//! visible row, each right-trimmed, joined with `\n`, with trailing blank rows
//! dropped. Rows are read individually rather than via `vt100`'s
//! `Screen::contents`, because the latter splices wrapped rows back into one
//! logical line while `pyte.Screen.display` — the behavioural oracle — keeps
//! one entry per physical row.
//!
//! Resize is deterministic: the parser is recreated at the new dimensions and
//! the buffered raw output is replayed, so narrowing and then widening again
//! restores the original contents rather than truncating cells.

use vt100::Parser;

/// In-memory terminal screen backed by `vt100`.
pub struct TerminalScreen {
    cols: u16,
    rows: u16,
    raw: Vec<u8>,
    parser: Parser,
}

impl std::fmt::Debug for TerminalScreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalScreen")
            .field("cols", &self.cols)
            .field("rows", &self.rows)
            .field("raw_len", &self.raw.len())
            .field("contents", &self.contents())
            .finish()
    }
}

impl TerminalScreen {
    /// Create a screen with the given dimensions.
    pub fn new(cols: u16, rows: u16) -> Self {
        assert!(cols > 0 && rows > 0, "cols and rows must be > 0");
        Self {
            cols,
            rows,
            raw: Vec::new(),
            // vt100 takes (rows, cols); no scrollback — TermProof asserts on
            // what is visible, and scrollback would resurrect erased text.
            parser: Parser::new(rows, cols, 0),
        }
    }

    /// Feed raw terminal bytes.
    ///
    /// Bytes are handed to the parser as-is, so a multi-byte character split
    /// across two chunks is reassembled rather than replaced with U+FFFD.
    pub fn feed_bytes(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.raw.extend_from_slice(data);
        self.parser.process(data);
    }

    /// Feed a UTF-8 string.
    pub fn feed_str(&mut self, data: &str) {
        self.feed_bytes(data.as_bytes());
    }

    /// Current plain-text contents, normalized like Python `screen_text`.
    pub fn contents(&self) -> String {
        let mut lines = self.visible_rows();
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }

    /// Raw display without normalization: every visible row, blanks included.
    pub fn raw_contents(&self) -> String {
        self.visible_rows().join("\n")
    }

    /// Resize the screen, replaying buffered raw output.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        assert!(cols > 0 && rows > 0, "cols and rows must be > 0");
        self.cols = cols;
        self.rows = rows;
        // `Parser::set_size` truncates cells past the new width, which would
        // make a narrow-then-widen round trip lossy. Replay instead.
        let mut parser = Parser::new(rows, cols, 0);
        parser.process(&self.raw);
        self.parser = parser;
    }

    /// Current columns.
    pub fn cols(&self) -> u16 {
        self.cols
    }

    /// Current rows.
    pub fn rows(&self) -> u16 {
        self.rows
    }

    /// Whether the screen is empty.
    pub fn is_empty(&self) -> bool {
        self.contents().is_empty()
    }

    /// Clear the screen and replay buffer.
    pub fn clear(&mut self) {
        self.raw.clear();
        self.parser = Parser::new(self.rows, self.cols, 0);
    }

    /// Every visible row, right-trimmed, in top-to-bottom order.
    fn visible_rows(&self) -> Vec<String> {
        self.parser
            .screen()
            .rows(0, self.cols)
            .map(|row| row.trim_end().to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_text_appears() {
        let mut s = TerminalScreen::new(80, 24);
        s.feed_str("hello");
        assert_eq!(s.contents(), "hello");
    }

    #[test]
    fn trailing_blank_lines_are_stripped() {
        let mut s = TerminalScreen::new(20, 5);
        s.feed_str("hi\r\n");
        assert_eq!(s.contents(), "hi");
    }

    #[test]
    fn ansi_escape_is_interpreted_not_emitted() {
        let mut s = TerminalScreen::new(80, 24);
        s.feed_str("\x1b[2J\x1b[H\x1b[31mred\x1b[0m");
        assert!(s.contents().contains("red"));
        assert!(!s.contents().contains("\x1b"));
    }

    #[test]
    fn unicode_is_preserved() {
        let mut s = TerminalScreen::new(80, 24);
        s.feed_str("héllo 🌍");
        let c = s.contents();
        assert!(c.contains("héllo"), "missing héllo in {c:?}");
        assert!(c.contains("🌍"), "missing emoji in {c:?}");
    }

    #[test]
    fn double_width_unicode() {
        let mut s = TerminalScreen::new(20, 5);
        s.feed_str("中文");
        let c = s.contents();
        assert!(c.contains("中文"));
    }

    #[test]
    fn resize_is_deterministic() {
        let mut s = TerminalScreen::new(40, 10);
        s.feed_str("hello world this is a long line that will wrap");
        let before = s.contents();
        // Narrowing rewraps: the same text, laid out over more rows.
        s.resize(20, 10);
        let after = s.contents();
        assert!(after.contains("hello"), "text lost on narrow: {after:?}");
        assert!(
            after.lines().count() > before.lines().count(),
            "narrowing did not rewrap: {before:?} -> {after:?}"
        );
        // Widening back is lossless because the raw stream is replayed.
        s.resize(40, 10);
        assert_eq!(s.contents(), before, "resize round trip was lossy");
    }

    #[test]
    fn chunked_feed_matches_single_feed() {
        let mut s1 = TerminalScreen::new(80, 24);
        s1.feed_str("hello world");
        let c1 = s1.contents();

        let mut s2 = TerminalScreen::new(80, 24);
        for chunk in ["hel", "lo ", "wor", "ld"] {
            s2.feed_str(chunk);
        }
        let c2 = s2.contents();
        assert_eq!(c1, c2);
    }

    #[test]
    fn clear_empties_screen() {
        let mut s = TerminalScreen::new(80, 24);
        s.feed_str("hello");
        s.clear();
        assert!(s.is_empty());
        assert_eq!(s.contents(), "");
    }

    // --- Non-monotonicity: text must be able to disappear. ---
    //
    // A screen built by stripping escapes can only ever grow, so every
    // "did X go away?" assertion fails against it. These tests are the
    // regression guard for that class of defect.

    #[test]
    fn erase_display_removes_earlier_text() {
        let mut s = TerminalScreen::new(80, 24);
        s.feed_str("FIRST TEXT\r\n");
        assert!(s.contents().contains("FIRST TEXT"));

        s.feed_str("\x1b[H\x1b[2J");
        s.feed_str("SECOND TEXT\r\n");

        let c = s.contents();
        assert!(
            !c.contains("FIRST TEXT"),
            "erase did not remove text: {c:?}"
        );
        assert!(c.contains("SECOND TEXT"), "second text missing: {c:?}");
    }

    #[test]
    fn cursor_addressing_overwrites_in_place() {
        let mut s = TerminalScreen::new(20, 5);
        s.feed_str("OPEN\r\n");
        // Home, then write over the same cell run.
        s.feed_str("\x1b[1;1HSHUT");

        let c = s.contents();
        assert!(!c.contains("OPEN"), "overwrite left stale text: {c:?}");
        assert!(c.contains("SHUT"), "overwrite text missing: {c:?}");
    }

    #[test]
    fn erase_in_line_removes_rest_of_line() {
        let mut s = TerminalScreen::new(40, 5);
        s.feed_str("keep|DROPTHIS");
        // Move back to just after "keep|" (column 6) and erase to end of line.
        s.feed_str("\x1b[1;6H\x1b[K");

        let c = s.contents();
        assert!(!c.contains("DROPTHIS"), "EL did not erase: {c:?}");
        assert!(c.contains("keep|"), "EL erased too much: {c:?}");
    }

    #[test]
    fn alternate_screen_hides_and_restores_primary() {
        let mut s = TerminalScreen::new(40, 6);
        s.feed_str("PRIMARY CONTENT\r\n");

        // Enter the alternate buffer and draw something else.
        s.feed_str("\x1b[?1049h");
        s.feed_str("\x1b[H\x1b[2JOVERLAY CONTENT\r\n");
        let alt = s.contents();
        assert!(
            !alt.contains("PRIMARY CONTENT"),
            "primary visible while on alternate buffer: {alt:?}"
        );
        assert!(alt.contains("OVERLAY CONTENT"), "overlay missing: {alt:?}");

        // Leave it; the primary buffer must come back.
        s.feed_str("\x1b[?1049l");
        let restored = s.contents();
        assert!(
            restored.contains("PRIMARY CONTENT"),
            "primary not restored: {restored:?}"
        );
        assert!(
            !restored.contains("OVERLAY CONTENT"),
            "overlay leaked into primary: {restored:?}"
        );
    }

    #[test]
    fn backspace_overwrite_removes_character() {
        let mut s = TerminalScreen::new(20, 3);
        s.feed_str("typo");
        s.feed_str("\x08\x08\x08\x08    ");

        let c = s.contents();
        assert!(!c.contains("typo"), "backspace-erased text remains: {c:?}");
    }
}
