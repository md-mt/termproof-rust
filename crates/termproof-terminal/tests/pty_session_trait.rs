//! `PtySession` must be usable everywhere the in-memory double is.
//!
//! Every step engine and execution mode reaches the terminal through
//! `&mut dyn Session` (RUST-016). Until `PtySession` implements that trait the
//! real pseudo-terminal is unreachable from the execution path, so these tests
//! drive a real child entirely through the trait object.

use std::path::PathBuf;
use std::time::Duration;

use termproof_terminal::{PtyConfig, PtySession, Session, SessionError};

fn spawned(argv: &[&str]) -> PtySession {
    let config = PtyConfig::new(argv.iter().map(|s| (*s).to_string()).collect());
    let mut session = PtySession::new(config, 80, 24).expect("construct pty session");
    session.spawn().expect("spawn pty child");
    session
}

#[test]
fn real_child_is_drivable_through_the_trait_object() {
    let mut owned = spawned(&["sh", "-c", "echo hello-from-pty"]);
    let session: &mut dyn Session = &mut owned;

    assert!(
        session
            .wait_for_text("hello-from-pty", Duration::from_secs(5))
            .expect("wait_for_text"),
        "text never appeared; screen={:?}",
        session.screen()
    );
    assert!(session.screen().contains("hello-from-pty"));
    assert!(session.raw_output().contains("hello-from-pty"));
    assert_eq!(session.cols(), 80);
    assert_eq!(session.rows(), 24);
    assert_eq!(session.argv(), ["sh", "-c", "echo hello-from-pty"]);

    assert_eq!(
        session.wait_for_exit(Duration::from_secs(5)).expect("exit"),
        Some(0)
    );
    session.close().expect("close");
}

#[test]
fn boxed_pty_session_satisfies_the_backend_return_type() {
    let mut session: Box<dyn Session> = Box::new(spawned(&["sh", "-c", "printf boxed"]));
    assert!(session
        .wait_for_text("boxed", Duration::from_secs(5))
        .expect("wait_for_text"));
    session.close().expect("close");
}

#[test]
fn send_line_reaches_the_child_and_the_snapshot_updates() {
    let mut owned = spawned(&["cat"]);
    let session: &mut dyn Session = &mut owned;
    session
        .read_available(Duration::from_millis(100))
        .expect("settle");
    session.send_line("round-trip").expect("send_line");
    assert!(session
        .wait_for_text("round-trip", Duration::from_secs(5))
        .expect("wait_for_text"));
    assert!(
        session.screen().contains("round-trip"),
        "screen() must reflect output observed by the preceding call; got {:?}",
        session.screen()
    );
    session.close().expect("close");
}

#[test]
fn press_reports_unknown_keys_as_a_config_error() {
    let mut owned = spawned(&["sh", "-c", "sleep 0.2"]);
    let session: &mut dyn Session = &mut owned;
    let err = session.press("f13").expect_err("f13 is not a known key");
    assert!(
        matches!(&err, SessionError::Config(msg) if msg == "unknown key: f13"),
        "expected the same wording as the in-memory double, got {err}"
    );
    session.close().expect("close");
}

#[test]
fn press_enter_is_accepted_by_a_real_child() {
    let mut owned = spawned(&["cat"]);
    let session: &mut dyn Session = &mut owned;
    session
        .read_available(Duration::from_millis(100))
        .expect("settle");
    session.send_text("pressed").expect("send_text");
    session.press("enter").expect("press enter");
    assert!(session
        .wait_for_text("pressed", Duration::from_secs(5))
        .expect("wait_for_text"));
    session.close().expect("close");
}

#[test]
fn cast_path_is_reported_for_a_recording_session() {
    let dir = std::env::temp_dir().join("termproof-pty-session-trait");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let cast: PathBuf = dir.join("session.cast");
    let config = PtyConfig::new(vec!["sh".into(), "-c".into(), "printf cast".into()])
        .with_cast_path(cast.clone());
    let mut owned = PtySession::new(config, 80, 24).expect("construct");
    owned.spawn().expect("spawn");
    let session: &mut dyn Session = &mut owned;
    assert_eq!(session.cast_path(), cast.as_path());
    session.close().expect("close");
    let _ = std::fs::remove_file(&cast);
}

#[test]
fn is_alive_and_exit_code_track_the_child() {
    let mut owned = spawned(&["sh", "-c", "exit 3"]);
    let session: &mut dyn Session = &mut owned;
    assert_eq!(
        session.wait_for_exit(Duration::from_secs(5)).expect("exit"),
        Some(3)
    );
    assert!(!session.is_alive());
    assert_eq!(session.exit_code(), Some(3));
    session.close().expect("close");
}
