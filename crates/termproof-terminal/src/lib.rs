//! TermProof terminal sessions: PTY/process ownership, terminal screen state,
//! and asciinema cast recording.
//!
//! RUST-002 baseline: crate skeleton only. Implementation lands in RUST-005
//! (non-PTY process sessions) and RUST-006 (PTY sessions and terminal state).
//!
//! RUST-007: adds the public `Session` trait, `MockSession` for deterministic
//! unit tests, key mapping, and wait helpers that the seven built-in steps
//! depend on. The real PTY/process backends (portable-pty + vt100) remain
//! behind `RUST-005`/`RUST-006` but steps are wired against this trait so they
//! can ship with mock-backed corpus parity now.

pub mod keys;
pub mod session;

pub use keys::{normalize_key, press_sequence, KEYS};
pub use session::{MockSession, Session};
