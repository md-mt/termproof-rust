## Summary

<!-- What does this PR do, in one or two sentences? -->

## Motivation and context

<!-- Why is this change needed? Link the issue it closes, e.g. "Closes #42".
     If there is no issue, say what problem this solves. -->

## Test plan

<!-- How did you verify it? At minimum, the three workspace gates:
     - cargo fmt --check --all
     - cargo clippy --workspace --all-targets --all-features -- -D warnings
     - cargo test --workspace
     If you changed a step or an assertion, what does the differential harness
     report (cargo test -p termproof --test differential_steps -- --nocapture)? -->

## Checklist

- [ ] `cargo fmt --check --all` passes
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] If behaviour changed, the README's [maturity section](https://github.com/md-mt/termproof-rust#maturity--read-this-before-using-it) still describes the port accurately — this port is experimental and not at parity with the Python implementation
- [ ] If a step or assertion changed, the differential harness counts in `harness/README.md` were updated to match, and the recorded oracle expectations were **not** regenerated
- [ ] If the change is user-facing, `CHANGELOG.md` gained an entry under `[Unreleased]`
- [ ] If a manifest changed, `docs/engineering-baseline.md` carries the documented reason

<!-- PRs are squash-merged; the merge commit message will be the PR title,
     so make the title a Conventional Commit (feat:, fix:, refactor:, docs:,
     chore:, ...). -->
