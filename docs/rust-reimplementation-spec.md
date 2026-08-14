# Termproof Rust Reimplementation Specification

Status: Active roadmap  
Owner: termproof maintainers  
Snapshot date: 2026-08-01  
Source baseline: GitHub main at commit 4f2c314  
Original issue baseline: 17 open issues, 0 open pull requests  
GitHub project: https://github.com/users/md-mt/projects/4  
Rust tracking: milestones 5–10 and issues 94–123

> **Provenance.** This document was written in `md-mt/termproof` and moved here
> unmodified when the Rust port was split into its own repository. It is
> recovered verbatim from `md-mt/termproof@9f3b772`, the last commit before that
> repository deleted it; the text below is exactly as it was written on
> 2026-08-01 and has deliberately not been retrofitted, so it still reads as a
> plan for a port living inside the Python repository.
>
> Read it as the design rationale and acceptance criteria for this port — those
> parts stand. Read the following as superseded:
>
> - **Repository layout (§5.2).** The workspace is no longer under `rust/`. The
>   manifest is `Cargo.toml` at this repository's root and crates are under
>   `crates/`. The crate table is no longer accurate either: `termproof-core`,
>   `termproof-terminal` and `termproof-evidence` were merged into one crate,
>   `termproof`, before any of them was published — its root, `terminal` module
>   and `evidence` module hold those three responsibilities respectively.
>   `termproof-cli` and `termproof-plugin-protocol` are unchanged.
> - **CI gates (§6.2), release channels and version sourcing (§8, RUST-021 and
>   RUST-023).** CI here runs fmt, clippy and tests only; the coverage,
>   conformance, drift and dependency-advisory gates listed there have never
>   been wired up on `main`, and the fuller tooling that exists lives on the
>   `archive/*` branches. There is no release workflow that has ever run.
> - **The Python oracle's obligations** — the recipe schema, the compatibility
>   corpus, version/changelog drift checks and the sdist gate stay in
>   `md-mt/termproof` and are not this repository's to satisfy.
> - **Issue and milestone numbers** throughout refer to `md-mt/termproof`.
>
> Most importantly, §11's parity gates are the whole point of this document and
> **none of them pass.** See the README for where the port actually stands.

## 1. Decision

Termproof will be reimplemented in Rust as a compatibility-first migration. The
current Python implementation remains the executable reference until the Rust
implementation passes the same recipes and produces semantically equivalent
results and evidence on Linux and macOS.

This is not a ground-up product redesign. The migration preserves:

- recipe format version 1 and legacy recipe loading;
- command names, flags, configuration behavior, and exit semantics;
- scripted PTY, scripted process, and agent-driven execution;
- built-in steps and assertions;
- cast, screenshot, text, JSON, Markdown, JUnit, video, baseline, diff, and
  aggregate report artifacts;
- Docker, CI, plugin, caching, parallel-run, and changed-path behavior; and
- compatibility for existing Python plugins during a documented transition.

The Rust executable becomes the default only after the parity gates in section
11 pass. The Python implementation is retained for at least one stable release
as the rollback path and compatibility oracle.

## 2. Why Rust

The reimplementation is intended to provide:

- a single predictable execution core with explicit ownership of process, PTY,
  terminal state, timeouts, and artifacts;
- lower startup and steady-state overhead for local and CI use;
- easier distribution through release binaries, Homebrew, containers, and
  platform wheels;
- compiler-enforced interfaces between sessions, execution modes, assertions,
  renderers, reporters, and storage; and
- fewer hidden couplings such as plugins calling private runner functions.

Rust is not itself the acceptance criterion. Behavioral compatibility,
diagnostics, testability, and a reversible rollout are.

## 3. Goals and non-goals

### Goals

1. Preserve the public behavior of the current main branch.
2. Resolve the open core, CI, distribution, documentation, and community issues
   in the same roadmap.
3. Make session and execution-mode APIs public, small, and independently
   testable.
4. Convert all recipe, plugin, process, and infrastructure failures into
   structured results instead of panics or whole-run aborts.
5. Establish one schema source, one result model, and one report pipeline.
6. Support Linux x86-64 and macOS x86-64/arm64 as Tier 1 at cutover.
7. Keep every milestone independently releasable and rollback-safe.

### Non-goals for the parity release

- Recipe format version 2 or incompatible recipe cleanup.
- A stable Rust dynamic-library ABI.
- Native Windows Tier 1 support.
- Replacing Docker, ffmpeg, or the external agent command with internal
  implementations.
- Changing artifact layout merely to make the Rust implementation simpler.
- Deleting the Python implementation before the rollback window closes.
- Treating outreach, adopter case studies, account creation, or star counts as
  work that software can complete automatically.

## 4. Compatibility contract

### 4.1 Recipe and configuration

- Recipe version 1 remains the canonical format.
- An omitted recipe version retains the current legacy interpretation.
- Unknown object fields remain loadable where the current schema permits
  additional properties. Rust models use flattened extension maps rather than
  silently discarding data.
- JSON and YAML inputs are supported.
- The checked-in JSON Schema remains Draft 2020-12.
- CLI values, discovered configuration, recipe values, and built-in defaults
  follow a single documented precedence table. The current behavior is frozen
  in fixtures before implementation, and issue #79 is resolved explicitly
  rather than copied as accidental behavior.
- Invalid recipes produce deterministic, path-specific diagnostics and never
  start the command under test.

### 4.2 CLI

The Rust binary preserves these command surfaces:

- termproof run
- termproof list
- termproof validate
- termproof plugins list
- termproof plugins search
- termproof plugins install
- termproof init
- termproof demo

All existing run flags are part of the parity inventory, including output,
video, FPS, priority, recipe filtering, parallelism, renderer, operator command,
configuration, reporter, XML path, screen renderer, video backend, visual diff,
baseline updates, changed-path skipping, and cache directory.

Help text may improve, but scripts must continue to parse command output, exit
codes, and result files without migration work. The exact exit-code table is
captured from the Python oracle in M0.

### 4.3 Execution

The implementation preserves:

- scripted PTY execution;
- scripted non-PTY process execution;
- external-command agent-driven execution;
- working directory and environment handling;
- timeout and expected-exit-code behavior;
- deterministic recipe ordering with bounded parallel execution; and
- Docker-backed sessions.

Built-in steps:

- wait_for_text
- wait_for_idle
- send_text
- send_line
- press
- sleep
- wait_for_regex

Built-in assertions:

- output_contains
- output_not_contains
- screen_contains
- screen_not_contains
- exit_code
- file_exists
- file_contains
- json_schema

Step input errors, send failures, timeout failures, and plugin exceptions become
failed step results. A single step failure must not crash the full invocation.
Infrastructure failures still fail the affected run, with a structured error
and preserved partial evidence.

### 4.4 Evidence and reports

The Rust implementation preserves the current artifact names, relative paths,
and semantic fields for:

- asciinema v2 cast recordings;
- final and per-step screenshots in SVG or PNG;
- final text snapshots;
- optional MP4 video;
- per-run result.json;
- per-run Markdown reports;
- aggregate latest reports;
- JUnit XML;
- exact baseline comparison and visual-diff outputs;
- CI receipts and before/after reports; and
- published screenshots or evidence links.

Byte-for-byte identity is required for stable JSON serialization, normalized
text, and deterministic render fixtures where practical. For timestamps,
durations, platform paths, terminal font rendering, and encoded video, tests
compare normalized semantics rather than unstable bytes.

### 4.5 Plugin compatibility

The currently exposed plugin roles remain supported:

- StepAction
- AssertionType
- ExecutionMode
- Reporter
- ScreenRenderer
- VideoBackend
- AgentRunner
- SessionBackend

Rust dynamic libraries are not the plugin boundary. Their ABI is not stable
enough for a long-lived third-party protocol.

New plugins use a versioned, newline-delimited JSON process protocol over
stdin/stdout. The handshake declares protocol version and capabilities. Requests
carry typed context and bounded extension data; responses contain typed results
and diagnostics. Plugin stderr is diagnostic output. Timeouts, cancellation,
maximum message size, lifecycle, and protocol errors are specified and tested.
Plugins remain trusted local programs; process isolation is an interoperability
boundary, not a security sandbox.

During migration, a small Python host process loads existing Python entry points
and legacy import references, then bridges them to the process protocol. Built-in
plugins are native Rust. The Python bridge can be removed only after a separately
announced deprecation window and an ecosystem audit.

## 5. Architecture

### 5.1 Data flow

1. CLI parses only command-line intent.
2. Config loader resolves configuration and records the origin of each value.
3. Recipe loader deserializes, validates, and produces typed recipes plus
   extension fields.
4. Planner expands filters, changed-path decisions, cache decisions, and run
   ordering.
5. Orchestrator selects an execution mode and a session backend.
6. Session owns the child process, input writer, output reader, raw byte log,
   terminal parser, activity clock, and cast recorder.
7. Step engine operates through a public SessionContext interface.
8. Assertion engine evaluates immutable run snapshots.
9. Evidence pipeline renders and stores artifacts.
10. Reporter pipeline serializes the canonical RunResult model.

No execution-mode plugin calls private orchestrator functions. The orchestrator
passes an explicit ExecutionContext containing only supported operations.

### 5.2 Cargo workspace

The Rust workspace lives in a dedicated `rust/` directory at the repository
root — workspace manifest at `rust/Cargo.toml`, crates under `rust/crates/`.
The Python implementation, packaging, and docs stay at the top level. This
isolates the Rust toolchain, avoids Cargo metadata colliding with the Python
package, and gives CI and rollback one obvious boundary.

Keep the initial workspace small:

| Crate | Responsibility |
| --- | --- |
| termproof-cli | Binary, command parsing, composition, human diagnostics |
| termproof-core | Models, config, schema, registries, planning, orchestration |
| termproof-terminal | PTY/process sessions, terminal screen, cast recording |
| termproof-evidence | Rendering, reports, video, baselines, diff, cache |
| termproof-plugin-protocol | Versioned process messages and client/host support |

Create another crate only when it has a real independent boundary. The Python
compatibility host remains a small Python package during the transition.

### 5.3 Implementation choices

- Stable Rust with a pinned toolchain and an explicit minimum supported Rust
  version; no nightly-only production features.
- serde and serde_json for typed data and stable JSON serialization.
- A maintained Serde YAML implementation for recipes and config.
- schemars as the source-to-schema generator and jsonschema for Draft 2020-12
  validation.
- portable-pty for cross-platform PTY creation.
- vt100 for the in-memory terminal screen.
- Standard threads and channels for the initial blocking PTY implementation.
  An async runtime is added only if profiling demonstrates a need.
- Instant-based deadlines and a condition variable or channel driven by output
  events. wait_for_idle has no unrelated hard three-second ceiling.
- thiserror-style typed internal errors, converted at the boundary into stable
  public diagnostics.
- A compatibility corpus chooses the regex engine. The selected engine must
  cover existing named captures and documented Python-regex behavior; unsupported
  constructs fail validation rather than changing meaning silently.
- Existing external ffmpeg/agg and Docker behavior stays behind typed adapters.

Technology evidence:

- portable-pty exposes a cross-platform PTY API:
  https://docs.rs/portable-pty/latest/portable_pty/
- vt100 parses terminal bytes into an in-memory screen:
  https://docs.rs/vt100/latest/vt100/
- jsonschema supports JSON Schema Draft 2020-12:
  https://docs.rs/jsonschema/latest/jsonschema/draft202012/
- schemars generates Draft 2020-12 schemas from Rust types:
  https://docs.rs/schemars/latest/schemars/
- quick-junit can serialize JUnit/XUnit data and clean invalid XML characters:
  https://docs.rs/quick-junit/

Dependencies are pinned in Cargo.lock after the relevant spike. The specification
does not pin crate versions that will be stale before implementation begins.

## 6. Quality strategy

### 6.1 Test layers

- Unit tests for config precedence, recipe loading, schema generation, JSON
  extraction, terminal event handling, key mapping, step state machines,
  assertions, cache keys, path normalization, and report serialization.
- Property tests for terminal input chunks, malformed plugin messages, and
  result serialization round trips.
- Golden tests for recipes, CLI output, terminal text, SVG/PNG, Markdown, JUnit,
  and normalized result JSON.
- Integration tests with small deterministic TUI fixtures covering PTY,
  non-PTY, timeout, signal, failed command, partial output, resize, Unicode,
  Docker, and agent-driven modes.
- Cross-runtime conformance tests that execute the same corpus with Python and
  Rust and produce a machine-readable difference report.
- Packaging smoke tests for release archives, Homebrew, container images,
  GitHub Actions, and supported PyPI wheels.

### 6.2 CI gates

Every Rust pull request runs:

- cargo fmt --check;
- clippy with warnings denied for workspace code;
- unit, integration, documentation, and conformance tests;
- a coverage report with a non-regression gate established in M0;
- Linux and macOS Tier 1 jobs;
- schema and golden-file drift checks; and
- dependency/license/advisory checks with documented exceptions.

Terminal and video tests use deterministic fixtures and normalized comparisons.
They may not be made green by broad retries. A quarantined test must have an
owner, issue, and expiry.

### 6.3 Tidy-first rule

Compatibility fixtures and small boundary refactors land before behavior
changes. Typical pull requests should change one seam at a time and remain
reviewable. The migration does not mix unrelated Python cleanup, Rust feature
work, and release automation in one change.

## 7. Proposed implementation milestones

These are additive Rust milestones. They do not silently repurpose the existing
public-launch milestones. Dates assume work starts 2026-08-03 and should be
rebaselined if staffing changes.

| Milestone | Target | Outcome | Issues |
| --- | --- | --- | --- |
| Rust M0 — Contract and skeleton | 2026-08-14 | Frozen compatibility corpus, workspace, CI | RUST-001–003 |
| Rust M1 — Core execution alpha | 2026-08-28 | Recipes, config, PTY/process, steps, assertions | RUST-004–009 |
| Rust M2 — CLI and evidence parity | 2026-09-11 | CLI, evidence, reports, video, cache, diff, parallelism | RUST-010–015 |
| Rust M3 — Extensibility parity | 2026-09-25 | Public execution API, agent, plugins, Python bridge, Docker | RUST-016–020 |
| Rust M4 — Distribution candidate | 2026-10-16 | Release channels, CI integrations, docs, durable evidence | RUST-021–025 |
| Rust M5 — v1 default cutover | 2026-11-13 | Conformance, canary, rollback-ready launch | RUST-026–030 |

M0 through M4 may release prerelease binaries. M5 is a gate, not a promise to
ship incompatible code on a date.

## 8. Proposed implementation issues

Each row is ready to become a GitHub issue. Dependencies refer to other rows.

| ID | Milestone | Issue and acceptance criteria | Depends on |
| --- | --- | --- | --- |
| RUST-001 | M0 | **Freeze the Python behavior contract.** Build representative v1 and legacy recipes; capture CLI help, exit codes, normalized results, artifacts, failure modes, config precedence, and every built-in step/assertion. Commit fixtures and a documented normalization policy. | — |
| RUST-002 | M0 | **Create the Rust workspace and engineering baseline.** Add the five crates, pinned stable toolchain, formatting/lint policy, error conventions, tracing policy, dependency policy, and a hello-world binary without replacing Python. | — |
| RUST-003 | M0 | **Add Rust CI quality gates.** Run formatting, clippy, tests, coverage, schema drift, dependency checks, and Linux/macOS builds. Also add equivalent lint/type/coverage gates needed to close #73 for the Python oracle. | RUST-002 |
| RUST-004 | M1 | **Implement typed recipes, config, and schema.** Load JSON/YAML, preserve extension fields, generate/check Draft 2020-12 schema, validate before execution, and prove the intended config-default cascade. Resolve #79 with regression fixtures. | RUST-001, RUST-002 |
| RUST-005 | M1 | **Implement non-PTY process sessions.** Support argv, cwd, env, stdin, output capture, timeout, expected exit, termination, and partial evidence with no leaked child processes. | RUST-004 |
| RUST-006 | M1 | **Implement PTY sessions and terminal state.** Support Linux/macOS PTYs, streaming reads, key/input writes, resize, raw output, VT screen state, cast events, activity timestamps, graceful termination, and deterministic fixture tests. | RUST-004 |
| RUST-007 | M1 | **Implement built-in steps.** Match all seven built-ins, timeout behavior, key mapping, regex captures, screen/raw search behavior, validation errors, and step evidence. | RUST-005, RUST-006 |
| RUST-008 | M1 | **Implement built-in assertions.** Match all eight built-ins, path resolution, JSON Schema validation, diagnostics, ordering, and pass/fail serialization. | RUST-004–007 |
| RUST-009 | M1 | **Contain step and execution failures.** Convert invalid step data, exceptions, process I/O failure, and PTY send failure into structured results; continue or stop according to the recipe contract; preserve partial artifacts. Add the regression that closes #74. | RUST-007, RUST-008 |
| RUST-010 | M2 | **Define canonical result and artifact storage.** Preserve paths, filenames, JSON fields, atomic writes, partial-run handling, latest-report semantics, and safe concurrent output. | RUST-009 |
| RUST-011 | M2 | **Implement text, SVG, and PNG evidence.** Render final/per-step snapshots at compatible dimensions and styling, preserve empty/error states, and pass normalized goldens. | RUST-006, RUST-010 |
| RUST-012 | M2 | **Implement video and idle semantics.** Detect a missing requested video as an explicit failure or warning per frozen contract, render MP4 through the backend, remove the hidden idle cap, and close #77 with tests. | RUST-006, RUST-010 |
| RUST-013 | M2 | **Unify serialization, validation, and reports.** Generate Markdown, JUnit, aggregate reports, and CLI summaries from one RunResult model; remove duplicate logic identified in #80 without output drift. | RUST-008, RUST-010 |
| RUST-014 | M2 | **Implement parallel runs, cache, changed paths, baselines, and visual diff.** Make ordering and output race-safe; preserve cache-key inputs, exact comparisons, update mode, and CI receipt behavior. | RUST-010–013 |
| RUST-015 | M2 | **Complete CLI parity.** Implement all commands and flags, completions/help if currently shipped, recipe filtering, init/demo/templates, plugin command routing, and conformance snapshots. | RUST-004–014 |
| RUST-016 | M3 | **Publish ExecutionContext and session interfaces.** Give execution modes supported operations instead of private runner calls, add contract tests and API docs, migrate built-ins, and close #78. | RUST-009, RUST-010 |
| RUST-017 | M3 | **Implement agent-driven execution.** Run the configured agent command, generate prompts/artifacts, parse bounded JSON robustly, handle malformed output and cancellation, and add regression tests covering #81's JSON loader concern. | RUST-015, RUST-016 |
| RUST-018 | M3 | **Specify and implement plugin protocol v1.** Define handshake, capability discovery, messages, lifecycle, timeout, cancellation, size bounds, version negotiation, diagnostics, and a conformance kit. Support all eight plugin roles. | RUST-016 |
| RUST-019 | M3 | **Build the legacy Python plugin host.** Load existing entry points and legacy import names, map every stable protocol through plugin protocol v1, document limitations, and test third-party fixture plugins. | RUST-018 |
| RUST-020 | M3 | **Implement Docker and custom session backends.** Match image, environment, mount, working-directory, interactive, cleanup, error, and artifact behavior through the public session boundary. | RUST-006, RUST-018 |
| RUST-021 | M4 | **Build signed release artifacts and install channels.** Produce Linux/macOS archives, checksums, provenance, Homebrew artifacts, container images, and PyPI-compatible platform wheels or a bundled launcher. Smoke-test every channel. | RUST-015, RUST-020 |
| RUST-022 | M4 | **Port CI integrations.** Exercise the GitHub Action, GitLab CI, CircleCI, Docker image, changed-path mode, receipts, and evidence publishing against prerelease Rust binaries. | RUST-014, RUST-021 |
| RUST-023 | M4 | **Establish one version and changelog source.** Generate/check crate, CLI, package, docs, action, and container versions; correct nonexistent built-ins; publish the missing 0.2.1 history; close #75 and #76. | RUST-015, RUST-021 |
| RUST-024 | M4 | **Complete docs and repository health.** Document Rust install/migration/plugin protocol, add community templates, deploy the VitePress site, then set homepage and topics. This covers #72, #82, and #83. | RUST-018, RUST-021, RUST-023 |
| RUST-025 | M4 | **Provide durable evidence hosting.** Select and document a retention model outside ephemeral Actions artifacts, publish from CI with least privilege, surface stable links, and close #69. | RUST-022, RUST-024 |
| RUST-026 | M5 | **Pass the cross-runtime conformance gate.** Run the complete corpus on all Tier 1 targets; allow only reviewed normalizations; publish a zero-unexplained-difference report and performance baseline. | RUST-001, RUST-003–025 |
| RUST-027 | M5 | **Run a canary release.** Publish prerelease channels, dogfood real repositories, collect opt-in diagnostics, repair regressions, and document known differences. No existing install changes engine silently. | RUST-026 |
| RUST-028 | M5 | **Cut over with rollback.** Make Rust the default, retain an explicit Python fallback for one stable release, document rollback, test downgrade and artifact compatibility, and define removal criteria. | RUST-027 |
| RUST-029 | M5 | **Publish the first PyPI release.** Complete the human-owned trusted-publisher setup, verify ownership/workflow/environment, publish and install-test the Rust-backed package, and close #7. | RUST-021, RUST-028 |
| RUST-030 | M5 | **Execute launch and adoption work.** Perform maintainer outreach, create official social accounts/posts, and publish at least three real consenting adopter case studies. Close #37, #38, and #35 only on external evidence. | RUST-024, RUST-027 |

## 9. Disposition of every currently open GitHub issue

No open issue is dropped. Closed Python-era feature issues are not reopened; their
delivered behavior is covered by RUST-001 and RUST-026.

| GitHub issue | Existing milestone | Rust disposition |
| --- | --- | --- |
| [#83 Deploy a live docs site](https://github.com/md-mt/termproof/issues/83) | Unmilestoned | RUST-024, M4 |
| [#82 Set repo topics and homepage URL](https://github.com/md-mt/termproof/issues/82) | Unmilestoned | RUST-024 after the site is live, M4 |
| [#81 Add unit tests for session.py and agent_driven._load_json](https://github.com/md-mt/termproof/issues/81) | Unmilestoned | Add Python oracle regression tests in M0/M1; Rust equivalents in RUST-006 and RUST-017 |
| [#80 Consolidate duplicate report, serialization, and validation logic](https://github.com/md-mt/termproof/issues/80) | Unmilestoned | RUST-013, M2 |
| [#79 Config defaults are never read](https://github.com/md-mt/termproof/issues/79) | Unmilestoned | Freeze intended precedence then fix in RUST-004, M1 |
| [#78 ExecutionMode plugins depend on private runner internals](https://github.com/md-mt/termproof/issues/78) | Unmilestoned | RUST-016, M3 |
| [#77 Missing requested video is silent and idle wait is capped](https://github.com/md-mt/termproof/issues/77) | Unmilestoned | RUST-006 and RUST-012, M1–M2 |
| [#76 Reconcile version drift and add 0.2.1 changelog](https://github.com/md-mt/termproof/issues/76) | Unmilestoned | RUST-023, M4; fix current release metadata before Rust GA |
| [#75 CHANGELOG documents built-ins that do not exist](https://github.com/md-mt/termproof/issues/75) | Unmilestoned | RUST-023, M4; also prevents incorrect parity scope |
| [#74 PTY step exceptions abort the whole run](https://github.com/md-mt/termproof/issues/74) | Unmilestoned | RUST-009, M1, plus Python-oracle regression |
| [#73 Add lint, type-check, and coverage CI gates](https://github.com/md-mt/termproof/issues/73) | Unmilestoned | RUST-003, M0, for both oracle and Rust code |
| [#72 Add community health files and GitHub templates](https://github.com/md-mt/termproof/issues/72) | Unmilestoned | RUST-024, M4 |
| [#69 Host video evidence outside Actions artifacts](https://github.com/md-mt/termproof/issues/69) | Unmilestoned | RUST-025, M4 |
| [#35 Publish three or more adopter case studies](https://github.com/md-mt/termproof/issues/35) | v1.0 — Stable API | RUST-030, M5; human/adopter gated |
| [#38 Create social presence](https://github.com/md-mt/termproof/issues/38) | v0.2 — Public Launch | RUST-030, M5; human account ownership required |
| [#37 Direct outreach to TUI maintainers](https://github.com/md-mt/termproof/issues/37) | v0.2 — Public Launch | RUST-030, M5; human communication required |
| [#7 Claim PyPI and publish first release](https://github.com/md-mt/termproof/issues/7) | v0.2 — Public Launch | RUST-029, M5; trusted-publisher administration required |

## 10. Existing milestone reconciliation

The live repository currently has four open milestones:

| Existing milestone | Current due date and state | Decision |
| --- | --- | --- |
| v0.2 — Public Launch | 2026-08-15; 3 open, 13 closed | Keep as the Python-era launch milestone or rebaseline it explicitly. Its remaining account/outreach work can proceed independently, but a Rust cutover cannot responsibly fit this date. |
| v0.3 — Ecosystem Foundation | 2026-09-12; 0 open, 8 closed | Treat Homebrew, Docker backend, PNG rendering, plugin CLI, and framework guides as Rust parity requirements in M2–M4. Do not reopen closed issues solely for tracking. |
| v0.4 — CI Everywhere | 2026-10-10; 0 open, 6 closed | Treat GitLab, CircleCI, Docker image, parallel execution, and visual diff as Rust parity requirements in RUST-014 and RUST-022. |
| v1.0 — Stable API | 2026-11-13; 1 open, 4 closed | Align with Rust M5 only if conformance and rollback gates pass. Keep #35 open until real adopter evidence exists. A star count is a success metric, not a software correctness gate. |

Recommended GitHub setup:

1. Create the six Rust milestones from section 7.
2. Create RUST-001 through RUST-030 and apply the dependencies from section 8.
3. Link, rather than duplicate, the 17 existing issues.
4. Add a rust-migration label and preserve current area/priority labels.
5. Close an existing issue only against its original acceptance criteria or an
   explicit maintainer-approved supersession note.

## 11. Cutover gates

Rust may become the default only when all of the following are true:

1. All M0–M4 technical issues are complete.
2. Every public command and flag is represented in the conformance corpus.
3. Every built-in step, assertion, execution mode, renderer, reporter, session
   backend, video backend, cache/diff mode, and CI integration has a passing
   Tier 1 integration test.
4. The cross-runtime report has zero unexplained semantic differences.
5. No child-process leak, panic, hang, or lost partial-evidence defect is open at
   high or critical priority.
6. Existing Python plugins pass through the compatibility host, or each known
   incompatibility is documented and accepted before release.
7. Install, upgrade, downgrade, and rollback are tested for every release
   channel.
8. Documentation and durable evidence links are live.
9. The Python fallback and rollback procedure have been exercised in CI.

Human-gated launch work in #35, #37, and #38 may continue after the technical
candidate is ready. Issue #7 remains a release-channel blocker because the PyPI
publisher configuration cannot be completed by code alone.

## 12. Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Terminal behavior differs by platform or read chunk | Keep one session owner, test chunk boundaries and Unicode, normalize only proven platform noise, run Linux/macOS conformance |
| Rust regex behavior differs from Python | Build a corpus first, select the engine from evidence, validate unsupported expressions, never silently reinterpret |
| Plugin rewrite strands users | Versioned process protocol plus Python compatibility host and a published deprecation window |
| Two implementations drift | Python is frozen as oracle; every Rust feature lands with cross-runtime fixtures; minimize dual feature development |
| Report and artifact changes break CI consumers | One canonical result model, golden fixtures, atomic storage, normalized semantic comparisons |
| Video dependencies undermine single-binary expectations | Keep ffmpeg/agg as declared optional external backends and make missing requested video visible |
| A big-bang PR becomes unreviewable | Land milestones through small dependency-ordered seams with tests before movement |
| Release channels disagree on version | One version source and automated drift checks in RUST-023 |
| Dates encourage premature cutover | Treat M5 as a quality gate; ship prereleases and rebaseline rather than waive parity |
| Outreach work is mistaken for implementation | Keep human-owned issues explicit and require real public evidence |

## 13. Definition of done

The Rust implementation is complete when:

- Rust is the default engine in supported install channels;
- recipe, CLI, plugin, execution, and artifact compatibility gates pass;
- all 17 currently open issues are either closed against their acceptance
  criteria or explicitly carried forward with an owner and reason;
- release, upgrade, downgrade, and rollback procedures are documented and
  tested;
- no critical/high correctness issue blocks Tier 1 use;
- docs and evidence hosting are live; and
- the Python fallback has completed its announced support window before removal.

The project is not complete merely because the Rust binary compiles or passes
unit tests. Completion is measured at the user-visible and ecosystem boundaries.
