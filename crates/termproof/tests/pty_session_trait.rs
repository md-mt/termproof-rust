//! `PtySession` must be usable everywhere the in-memory double is.
//!
//! Every step engine and execution mode reaches the terminal through
//! `&mut dyn Session` (RUST-016). Until `PtySession` implements that trait the
//! real pseudo-terminal is unreachable from the execution path, so these tests
//! drive a real child entirely through the trait object.

use std::path::{Path, PathBuf};
use std::time::Duration;

use termproof::terminal::{PtyConfig, PtySession, Session, SessionError};

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

/// Run `pwd -P` on a pty and return what the child printed.
///
/// `-P` rather than a bare `pwd`: the shell prefers an inherited `$PWD` when it
/// is still valid, and on macOS a temporary directory reaches us through a
/// symlink. Both sides of these comparisons have to be the physical path or
/// they compare two spellings of one directory.
fn printed_pwd(session: &mut dyn Session) -> PathBuf {
    session.wait_for_exit(Duration::from_secs(5)).expect("exit");
    let line = session
        .raw_output()
        .lines()
        .next()
        .expect("the child printed nothing")
        .trim()
        .to_string();
    PathBuf::from(line)
}

fn physical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).expect("directory should exist")
}

#[test]
fn cwd_names_the_directory_the_child_started_in() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = PtyConfig::new(vec!["sh".into(), "-c".into(), "pwd -P".into()])
        .with_cwd(dir.path().to_path_buf());
    let mut owned = PtySession::new(config, 80, 24).expect("construct");
    owned.spawn().expect("spawn");
    let session: &mut dyn Session = &mut owned;

    let printed = printed_pwd(session);
    let reported = session.cwd().expect("a spawned child has a directory");
    assert_eq!(reported, dir.path(), "reported as configured");
    assert_eq!(
        physical(reported),
        printed,
        "cwd must name the directory the child itself printed"
    );
    session.close().expect("close");
}

#[test]
fn a_session_with_no_configured_directory_reports_where_the_child_really_went() {
    // A pty child does not inherit our directory. `portable-pty` starts it in
    // its home directory instead, and `cwd` has to report that rather than the
    // comfortable-sounding wrong answer — this test exists to say which of the
    // two it is, by asking the child.
    let mut owned = spawned(&["sh", "-c", "pwd -P"]);
    let session: &mut dyn Session = &mut owned;

    let printed = printed_pwd(session);
    let reported = session.cwd().expect("HOME is set, so this is knowable");
    assert_eq!(
        physical(reported),
        printed,
        "cwd must name the directory the child itself printed"
    );
    assert_ne!(
        printed,
        physical(&std::env::current_dir().expect("current dir")),
        "if this ever starts inheriting, the doc comment is now wrong"
    );
    session.close().expect("close");
}

#[test]
fn a_directory_that_does_not_exist_is_not_reported_as_if_it_did() {
    // `portable-pty` does not refuse a `cwd` that is not a directory — it
    // silently starts the child in its home directory instead. Echoing the
    // request back would hand the caller a path the child was never in.
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("no-such-directory");
    let config =
        PtyConfig::new(vec!["sh".into(), "-c".into(), "pwd -P".into()]).with_cwd(missing.clone());
    let mut owned = PtySession::new(config, 80, 24).expect("construct");
    owned.spawn().expect("spawn");
    let session: &mut dyn Session = &mut owned;

    let printed = printed_pwd(session);
    assert_ne!(
        printed, missing,
        "the child cannot be in a directory that does not exist"
    );
    let reported = session.cwd().expect("HOME is set, so this is knowable");
    assert_eq!(
        physical(reported),
        printed,
        "report where the child went, or nothing — never the request"
    );
    session.close().expect("close");
}

#[test]
fn an_unspawned_session_has_nowhere_to_report() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = PtyConfig::new(vec!["sh".into(), "-c".into(), "pwd -P".into()])
        .with_cwd(dir.path().to_path_buf());
    let session = PtySession::new(config, 80, 24).expect("construct");
    assert_eq!(
        Session::cwd(&session),
        None,
        "nothing has launched, so there is no launch directory"
    );
}

#[test]
fn cwd_does_not_follow_a_child_that_changes_directory() {
    // The caveat in the doc comment, demonstrated. `cwd` is where the child
    // started; a `chdir` moves the child and leaves the answer behind, and
    // nothing here can detect that it has happened.
    let dir = tempfile::tempdir().expect("tempdir");
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).expect("create subdirectory");

    let config = PtyConfig::new(vec![
        "sh".into(),
        "-c".into(),
        "cd elsewhere && pwd -P".into(),
    ])
    .with_cwd(dir.path().to_path_buf());
    let mut owned = PtySession::new(config, 80, 24).expect("construct");
    owned.spawn().expect("spawn");
    let session: &mut dyn Session = &mut owned;

    let printed = printed_pwd(session);
    assert_eq!(
        printed,
        physical(&elsewhere),
        "the child should have moved; it printed {printed:?}"
    );

    let reported = session.cwd().expect("a spawned child has a directory");
    assert_eq!(reported, dir.path(), "still the launch directory");
    assert_ne!(
        physical(reported),
        printed,
        "cwd reported where the child is now, which it cannot know"
    );
    session.close().expect("close");
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
