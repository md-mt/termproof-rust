//! Every path 0.3.1 published for the JUnit writer still resolves.
//!
//! #34 split `quick-junit` out of `evidence` into its own `junit` feature and
//! moved `generate_junit` to a root `junit` module, because it reads a
//! `RunResult` and nothing else. `default` still turns both features on, so
//! that move must be invisible: `evidence::report::generate_junit` and
//! `evidence::generate_junit` are kept as re-exports.
//!
//! A re-export is easy to delete by accident — rustdoc renders it as a link to
//! the canonical item rather than a page of its own, so it leaves no trace in
//! the generated documentation to notice missing. Naming all three paths here
//! is what makes dropping one a test failure rather than a silent break for a
//! consumer that never asked for anything to change.

#![cfg(all(feature = "evidence", feature = "junit"))]

use termproof::RunResult;

type Writer = fn(&[RunResult]) -> String;

#[test]
fn the_junit_writer_answers_to_every_name_0_3_1_gave_it() {
    let moved: Writer = termproof::junit::generate_junit;
    let via_evidence: Writer = termproof::evidence::generate_junit;
    let via_report: Writer = termproof::evidence::report::generate_junit;

    let expected = moved(&[]);
    assert!(expected.contains("<?xml"));
    assert_eq!(via_evidence(&[]), expected);
    assert_eq!(via_report(&[]), expected);
}
