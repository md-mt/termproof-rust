//! Not re-rendering a screen that has not changed.
//!
//! A recipe captures evidence either side of an action, and plenty of actions
//! turn out not to change the screen — a key that was already pressed, a wait
//! that had nothing to wait for, a step that failed to do anything. Rendering
//! and uploading a byte-identical PNG for each of those is pure cost, and a
//! reviewer scrolling twelve identical images is worse off than one reading
//! four distinct ones.
//!
//! # Attributes count
//!
//! Deduplication keys on
//! [`AttributedScreen::render_fingerprint`](crate::terminal::attributed::AttributedScreen::render_fingerprint),
//! not on text. Two screens with the same characters but a different highlight
//! are *different screenshots*, and a text-keyed cache would silently collapse
//! them — losing exactly the frame where a selection moved.
//!
//! # It only ever looks backwards one step
//!
//! A run of identical screens all point at the first of them, but a screen that
//! matches one from earlier — after something else in between — renders again.
//! That is deliberate: evidence is read in order, and a caption saying "same as
//! step 2" nine steps later costs the reader more than the image saves.
//!
//! ```
//! use termproof::evidence::dedup::Deduper;
//! # use termproof::terminal::attributed::attributed_screen_from_text;
//! let mut deduper = Deduper::default();
//!
//! let menu = attributed_screen_from_text("menu open", 20, 2);
//! let same = attributed_screen_from_text("menu open", 20, 2);
//! let gone = attributed_screen_from_text("menu closed", 20, 2);
//!
//! assert_eq!(deduper.check("opened", &menu), None);
//! assert_eq!(deduper.check("still-open", &same), Some("opened"));
//! assert_eq!(deduper.check("closed", &gone), None);
//! ```

use crate::terminal::attributed::AttributedScreen;

/// Decides whether a screen needs rendering, or matches the previous one.
#[derive(Debug, Default, Clone)]
pub struct Deduper {
    previous: Option<Rendered>,
}

#[derive(Debug, Clone)]
struct Rendered {
    label: String,
    fingerprint: String,
}

impl Deduper {
    /// Whether `screen` duplicates the previously rendered one.
    ///
    /// Returns `Some(label)` of the step to reuse, or `None` when this screen
    /// needs rendering. A `None` answer records the screen as the new previous,
    /// so a run of identical screens all point at the first.
    ///
    /// The caller renders on `None` and, if that render *fails*, should call
    /// [`Deduper::forget`] — otherwise the next identical screen would be told
    /// to reuse an image that does not exist.
    pub fn check(&mut self, label: &str, screen: &AttributedScreen) -> Option<&str> {
        let fingerprint = screen.render_fingerprint();
        let matched = self
            .previous
            .as_ref()
            .is_some_and(|p| p.fingerprint == fingerprint);
        if matched {
            // Deliberately not updating `previous`: every screen in a run of
            // identical ones points at the first, not at its neighbour.
            return self.previous.as_ref().map(|p| p.label.as_str());
        }
        self.previous = Some(Rendered {
            label: label.to_string(),
            fingerprint,
        });
        None
    }

    /// Drop the remembered screen, so the next one renders afresh.
    ///
    /// For the case where the caller was told to render and could not. Without
    /// this, a failed render leaves a fingerprint pointing at an image that was
    /// never produced, and the next identical screen reuses nothing.
    pub fn forget(&mut self) {
        self.previous = None;
    }

    /// The label of the last screen that needed rendering, if any.
    pub fn last_rendered(&self) -> Option<&str> {
        self.previous.as_ref().map(|p| p.label.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::attributed::attributed_screen_from_ansi_text;
    use crate::terminal::attributed::attributed_screen_from_text;

    fn text(s: &str) -> AttributedScreen {
        attributed_screen_from_text(s, 20, 2)
    }

    fn ansi(s: &str) -> AttributedScreen {
        attributed_screen_from_ansi_text(s, 20, 2)
    }

    #[test]
    fn the_first_screen_always_renders() {
        let mut d = Deduper::default();
        assert_eq!(d.check("first", &text("hello")), None);
    }

    #[test]
    fn an_unchanged_screen_reuses_the_previous() {
        let mut d = Deduper::default();
        d.check("first", &text("hello"));
        assert_eq!(d.check("second", &text("hello")), Some("first"));
    }

    #[test]
    fn a_changed_screen_renders() {
        let mut d = Deduper::default();
        d.check("first", &text("hello"));
        assert_eq!(d.check("second", &text("goodbye")), None);
    }

    #[test]
    fn a_run_of_identical_screens_all_point_at_the_first() {
        // Not at their immediate neighbour — a chain of "same as the one
        // before" is useless to a reader following it back.
        let mut d = Deduper::default();
        d.check("first", &text("hello"));
        assert_eq!(d.check("second", &text("hello")), Some("first"));
        assert_eq!(d.check("third", &text("hello")), Some("first"));
    }

    #[test]
    fn same_text_different_colour_is_a_different_screenshot() {
        // The reason this keys on attributes. A text-keyed cache collapses
        // these two and loses the frame where the selection moved.
        let mut d = Deduper::default();
        d.check("red", &ansi("\x1b[31mhi"));
        assert_eq!(d.check("green", &ansi("\x1b[32mhi")), None);
    }

    #[test]
    fn identical_colour_and_text_still_dedupes() {
        let mut d = Deduper::default();
        d.check("red", &ansi("\x1b[31mhi"));
        assert_eq!(d.check("red-again", &ansi("\x1b[31mhi")), Some("red"));
    }

    #[test]
    fn only_the_immediately_previous_screen_counts() {
        // A screen matching one from earlier, with something else in between,
        // renders again: evidence is read in order, and "same as step 2" nine
        // steps later costs the reader more than the image saves.
        let mut d = Deduper::default();
        d.check("a", &text("one"));
        d.check("b", &text("two"));
        assert_eq!(d.check("c", &text("one")), None);
    }

    #[test]
    fn forgetting_makes_the_next_identical_screen_render() {
        // The failed-render path. Without `forget`, the next identical screen
        // is told to reuse an image that was never produced.
        let mut d = Deduper::default();
        d.check("first", &text("hello"));
        d.forget();
        assert_eq!(d.check("second", &text("hello")), None);
    }

    #[test]
    fn last_rendered_tracks_the_reusable_label() {
        let mut d = Deduper::default();
        assert_eq!(d.last_rendered(), None);
        d.check("first", &text("hello"));
        assert_eq!(d.last_rendered(), Some("first"));
        d.check("second", &text("hello"));
        // Unchanged: the run still points at the first.
        assert_eq!(d.last_rendered(), Some("first"));
        d.check("third", &text("other"));
        assert_eq!(d.last_rendered(), Some("third"));
    }
}
