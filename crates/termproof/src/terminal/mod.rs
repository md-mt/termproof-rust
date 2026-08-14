//! TermProof terminal: PTY/process ownership, terminal screen, cast recording, idle, and session backends (RUST-005/006 + RUST-012 + RUST-016).

// RUST-005/006 PTY and process implementations
pub mod attributed;
pub mod cast;
pub mod idle;
pub mod proc;
pub mod process;
pub mod pty;
pub mod pty_backend;
pub mod screen;

// RUST-016 public session interfaces (ExecutionContext boundary)
pub mod backend;
pub mod custom;
pub mod docker;
pub mod error;
pub mod inmemory;
pub mod keys;
pub mod session;
pub mod tmux;

pub use cast::{replay_cast, ActivityClock, CastHeader, CastRecorder};
pub use idle::{wait_for_idle, IdleTracker};
pub use process::{ProcessConfig, ProcessError, ProcessOutput, ProcessSession, ProcessWaitResult};
pub use pty::{PtyConfig, PtyError, PtySession};
pub use pty_backend::PtySessionBackend;
pub use screen::TerminalScreen;

pub use backend::SessionBackend;
pub use custom::PluginSessionBackend;
pub use docker::{DockerBackendConfig, DockerSessionBackend};
pub use error::SessionError;
pub use inmemory::InMemorySession;
pub use keys::{normalize_key, press_sequence, PressAction, PressError, KEYS};
pub use session::Session;
