//! The PTY backend is how an execution mode obtains a real terminal.
//!
//! `ExecutionContext::create_session` returns `Box<dyn Session>`; without a
//! `SessionBackend` that produces a spawned `PtySession` there is nothing for
//! it to return but a double.

use std::collections::HashMap;
use std::time::Duration;

use termproof_terminal::{PtySessionBackend, SessionBackend, SessionError};

#[test]
fn backend_returns_a_spawned_child() {
    let backend = PtySessionBackend::new();
    assert_eq!(backend.name(), "pty");
    let mut session = backend
        .create_session(
            vec!["sh".into(), "-c".into(), "echo from-backend".into()],
            std::path::PathBuf::new(),
            None,
            HashMap::new(),
            80,
            24,
        )
        .expect("create_session");

    assert!(
        session
            .wait_for_text("from-backend", Duration::from_secs(5))
            .expect("wait_for_text"),
        "backend must hand back an already-spawned child; screen={:?}",
        session.screen()
    );
    assert_eq!(
        session.wait_for_exit(Duration::from_secs(5)).expect("exit"),
        Some(0)
    );
    session.close().expect("close");
}

#[test]
fn backend_honours_cwd_and_env() {
    let dir = std::env::temp_dir();
    let mut env = HashMap::new();
    env.insert("TERMPROOF_BACKEND_PROBE".to_string(), "probe-value".into());

    let backend = PtySessionBackend::new();
    let mut session = backend
        .create_session(
            vec![
                "sh".into(),
                "-c".into(),
                "printf '%s|%s' \"$TERMPROOF_BACKEND_PROBE\" \"$(pwd)\"".into(),
            ],
            std::path::PathBuf::new(),
            Some(dir.to_string_lossy().to_string()),
            env,
            120,
            24,
        )
        .expect("create_session");

    assert!(session
        .wait_for_text("probe-value|", Duration::from_secs(5))
        .expect("wait_for_text"));
    session.close().expect("close");
}

#[test]
fn empty_argv_is_a_config_error_not_a_panic() {
    let backend = PtySessionBackend::new();
    let err = backend
        .create_session(
            Vec::new(),
            std::path::PathBuf::new(),
            None,
            HashMap::new(),
            80,
            24,
        )
        .err()
        .expect("empty argv cannot spawn");
    assert!(matches!(err, SessionError::Config(_)), "got {err}");
}

#[test]
fn zero_dimensions_are_a_config_error_not_a_panic() {
    let backend = PtySessionBackend::new();
    let err = backend
        .create_session(
            vec!["sh".into()],
            std::path::PathBuf::new(),
            None,
            HashMap::new(),
            0,
            0,
        )
        .err()
        .expect("a zero-sized terminal cannot be allocated");
    assert!(matches!(err, SessionError::Config(_)), "got {err}");
}

#[test]
fn a_cast_path_is_recorded_and_reported() {
    let dir = std::env::temp_dir().join("termproof-pty-backend");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let cast = dir.join("backend.cast");
    let _ = std::fs::remove_file(&cast);

    let backend = PtySessionBackend::new();
    let mut session = backend
        .create_session(
            vec!["sh".into(), "-c".into(), "echo recorded".into()],
            cast.clone(),
            None,
            HashMap::new(),
            80,
            24,
        )
        .expect("create_session");
    assert_eq!(session.cast_path(), cast.as_path());
    assert!(session
        .wait_for_text("recorded", Duration::from_secs(5))
        .expect("wait_for_text"));
    session.close().expect("close");

    let written = std::fs::read_to_string(&cast).expect("cast file written");
    assert!(
        written.contains("\"version\""),
        "cast header missing: {written:?}"
    );
    let _ = std::fs::remove_file(&cast);
}
