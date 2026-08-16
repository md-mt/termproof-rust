# TermProof Rust Repository Governance

Status: Active
Owner: termproof maintainers (`@md-mt`)
Scope: the `md-mt/termproof-rust` GitHub repository
Baseline date: 2026-08-16 (main at `e6efcb5`)

This document records the repository's GitHub governance: metadata,
vulnerability settings, merge policy, the required CI checks by their stable
names, the intended `main` ruleset, the emergency bypass, and the audit
procedure. It is the review trail for the settings change that follows it —
the external GitHub settings are deliberately **not** changed by this
document's PR. When the settings are applied, the exact before/after state is
recorded there, per the audit note in the open-source polish plan.

This document governs the GitHub repository surface. Code policy — formatting,
lints, errors, tracing, dependencies, features, unsafe code — is
[`docs/engineering-baseline.md`](engineering-baseline.md).

## 1. Baseline — current live state (verified 2026-08-16)

Verified against the live repository with the GitHub API/GraphQL, not assumed:

| Setting | Live value |
|---|---|
| Visibility / default branch | public, `main` |
| Description | "Rust implementation of TermProof — evidence-first verification for terminal and TUI apps. Ported from the Python reference implementation in md-mt/termproof." |
| Homepage | unset |
| Topics | none |
| Issues / Wiki / Projects / Discussions | enabled / enabled / enabled / disabled |
| Merge methods | all three allowed (squash, merge commit, rebase); delete branch on merge **off** |
| Rulesets | none configured |
| Branch protection | none (legacy endpoint absent) |
| Security policy file | `SECURITY.md` present and detected by GitHub |
| Private vulnerability reporting | enabled — the *Security → Report a vulnerability* button is live, and reporters are directed there by `SECURITY.md` |
| Vulnerability alerts | not enabled (alerts endpoints are admin-gated; the Dependabot alerts API reports "Dependabot alerts are disabled for this repository") |
| Automated security fixes | not enabled |
| Dependabot version updates | enabled — `.github/dependabot.yml`, weekly, grouped |
| Code owners | `* @md-mt` (`CODEOWNERS`) |
| Community health files | present — `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, `SUPPORT.md`, `CHANGELOG.md`, issue forms, PR template |
| CI on `main` | green at baseline: 10/10 check runs on `e6efcb5` |

Because no ruleset exists, `main` currently accepts direct pushes and merges
without review or checks. The gate is social, not structural: every change so
far has gone through a reviewed PR, but nothing in GitHub enforces that today.

## 2. Intended repository metadata

Applied by the settings-change task after this document merges and is reviewed:

- **Homepage:** `https://docs.rs/termproof` — the library's API docs, the most
  useful landing page for consumers (the README already links there).
- **Topics:** `rust`, `terminal`, `tui`, `testing`, `verification`,
  `evidence`, `pty`, `cli`, `snapshot-testing`, `developer-tools`.

## 3. Intended vulnerability settings

- Enable **vulnerability alerts** and **Dependabot alerts**, so the advisory
  posture is visible in the UI as well as in CI (`cargo deny` already runs the
  advisory check on schedule).
- Keep **private vulnerability reporting** enabled — `SECURITY.md` documents it
  as the reporting path.
- Enable **automated security fixes** (Dependabot security updates) only if the
  maintainers will review the resulting PRs; otherwise leave them off. Least
  privilege applies: do not enable a surface nobody is committed to tending.
- The baseline for these settings is "not enabled" (section 1). The
  settings-change task records the exact before/after JSON.

## 4. Merge policy

Intended, and consistent with the pull-request template ("PRs are
squash-merged; the merge commit message will be the PR title"):

- **Squash merge only.** Merge commits and rebase merges are disabled, so every
  `main` commit reads as one conventional-commit message and `main` history
  stays linear. The auto-release workflow derives bumps from commit messages,
  so the PR title is the release input.
- **Delete branch on merge.** Merged branches are removed automatically.
- **Pull requests required for `main`.** No direct pushes to `main`.

## 5. Required checks (stable names)

The ruleset requires status checks by exact name, so the names below are the
stable job names from the merged workflows. Renaming a job silently un-gates
the ruleset; `security.yml` already documents that its job names are stable for
this reason, and this list is the contract.

Every pull request runs eight gate checks across three workflows. The five
`Rust`/`Security` checks also run on `main` (verified at `e6efcb5`); the three
`Publish (crates.io)` jobs run only on pull requests — `publish-crates.yml`
triggers on `pull_request`, `workflow_dispatch`, and `release`, never on a
push to `main` — so those three are verified on PR heads, not on `main`
(audit commands in section 8):

| Check name (exact) | Workflow | Job |
|---|---|---|
| `fmt, clippy, test (Rust ubuntu-latest)` | `Rust` | `rust` (matrix leg) |
| `fmt, clippy, test (Rust macos-latest)` | `Rust` | `rust` (matrix leg) |
| `cargo deny (advisories, licenses, bans, sources)` | `Security` | `deny` |
| `cargo semver-checks (public API vs latest published termproof)` | `Security` | `semver` |
| `cargo package (termproof tarball + contents)` | `Security` | `package` |
| `Plan` | `Publish (crates.io)` | `plan` |
| `Gate` | `Publish (crates.io)` | `gate` |
| `Dry run` | `Publish (crates.io)` | `publish` (PR path) |

The `Publish (crates.io)` jobs are dry runs on pull requests by design
(`docs/publishing.md`): nothing uploads until a release is published, so they
are safe to require on every PR. The `Dependabot` and `.github/dependabot.yml`
checks on `main` are not PR gate checks and are not required.

Any change to a workflow that renames a job must update this table in the same
PR, or the ruleset will silently stop enforcing that check.

## 6. Intended `main` ruleset behavior

One ruleset, name **`main`**, enforcement **active**, targeting branch `main`:

- **Require pull requests** — direct pushes rejected.
- **Required approving reviews: 0 — the enforceable floor.** GitHub counts a
  required review only from a reviewer with write permission (or a designated
  code owner) who is not the PR author. Today the only write-capable account
  is the maintainer `@md-mt`, which authors every PR, and no second eligible
  reviewer exists (verified 2026-08-16: the review/audit account `@mw-ding`
  has no push access). A count of one would make every routine
  `@md-mt`-authored PR unmergeable without the emergency bypass, which
  section 7 reserves for incidents. The count is therefore **zero**: the
  ruleset still requires a PR, all eight checks, and up-to-date branches, and
  blocks force pushes; review stays social (the PR template asks for review)
  until a second eligible reviewer is established. Raising this count is
  gated on the promotion criteria below.
- **Code-owner review: not required** — `require_code_owner_review` stays
  off. With `CODEOWNERS = * @md-mt` and the sole code owner authoring the
  PRs, requiring code-owner review would reproduce the same lockout.
- **Dismiss stale approvals** when new commits are pushed.
- **Require conversation resolution** — a resolved thread must stay resolved.
- **Require status checks**: the eight names in section 5.
- **Require branches to be up to date before merging** — the PR head must
  include the latest `main` before the merge button works. CI latency for the
  eight checks is acceptable (minutes), so the strict option stays on.
- **Block force pushes** and **block branch deletion** on `main`.
- **Bypass: explicit, not blanket.** The sole bypass actor is the maintainer
  account `@md-mt` (the code owner). Do **not** enable the "bypass all
  administrators" toggle, so a future admin added to the repo does not inherit
  an unchecked path around the ruleset.
- **Least privilege**: nothing else is exempted. `CODEOWNERS` remains
  `* @md-mt` as the ownership signal; because the sole code owner authors the
  PRs, the ruleset enforces the gate structurally (PR + checks + up-to-date)
  and keeps code-owner review off, so no routine path depends on a bypass.

**Promotion criteria.** The review count is raised from zero to **one** only
when a second eligible reviewer exists — a collaborator granted write
permission (or designated code owner) who is not the PR author, or a
non-`@md-mt` authoring path (a bot/CI account opening PRs) that lets
`@md-mt` approve as the code owner. Until one of those is real, a count of
one stays out of the ruleset: it would lock every routine `@md-mt`-authored
PR onto the emergency bypass, which section 7 forbids. The settings task
records which reviewer was added and re-runs the audit when it promotes.

## 7. Emergency bypass

The ruleset exists to keep normal work on the reviewed PR path; it must not be
able to lock the maintainer out of the repository.

- **When it is used:** incident recovery only — e.g. a ruleset misconfiguration
  that blocks an urgent security fix, a botched CI workflow that gates every
  PR, or GitHub-side drift that makes the required checks un-runnable.
- **Who:** `@md-mt`, the explicit bypass actor. It is a per-incident decision,
  not a standing permission to push to `main`.
- **Procedure:**
  1. Record the incident and the reason in the repository (a comment on the
     relevant issue/PR, or a maintenance note) before acting.
  2. Use the bypass to land the fix.
  3. Re-run the normal gate as soon as the incident clears; if the bypass was
     a ruleset edit, restore the documented ruleset and re-verify it with the
     audit commands in section 8.
  4. Note the bypass in the next audit (section 8), so it cannot become the
     habitual path.
- **Non-uses:** never for routine changes, version bumps, or "small fix,
   quick push" convenience. If a change is worth making to `main`, it is worth
   the reviewed PR path.

## 8. Audit procedure

Governance is a live surface; the audit re-verifies that the repository still
matches this document. Run it:

- after any settings change (before/after JSON recorded with the change);
- after any workflow rename that touches a required check;
- quarterly, or whenever the governance document is edited.

Commands (all read-only):

```sh
# Metadata
gh repo view md-mt/termproof-rust --json homepageUrl,repositoryTopics,hasIssuesEnabled,hasWikiEnabled,hasProjectsEnabled,hasDiscussionsEnabled,mergeCommitAllowed,rebaseMergeAllowed,squashMergeAllowed,deleteBranchOnMerge

# Ruleset and branch protection
gh api repos/md-mt/termproof-rust/rulesets
gh api repos/md-mt/termproof-rust/branches/main/protection

# Vulnerability settings (admin token; record 2xx vs 4xx per endpoint)
gh api -i repos/md-mt/termproof-rust/vulnerability-alerts
gh api -i repos/md-mt/termproof-rust/automated-security-fixes
gh api repos/md-mt/termproof-rust/dependabot/alerts

# Baseline CI health on the current head of main — the five Rust/Security
# gate checks (plus Dependabot config checks). The Publish jobs never run on
# main, so this command alone cannot verify the full required-check contract.
gh api repos/md-mt/termproof-rust/commits/main/check-runs --jq '.check_runs[].name'

# Required PR gate checks — audit from a PR head, where all eight run.
# Pick any open or recent PR (44 in the baseline, or the newest merged one):
gh pr checks 44 --repo md-mt/termproof-rust --json name,state,workflow
# or directly against a PR head commit:
gh api repos/md-mt/termproof-rust/commits/<pr-head-sha>/check-runs --jq '.check_runs[].name'

# After the ruleset exists, verify the eight configured contexts from the
# ruleset JSON itself (the settings-change task records the ruleset id):
gh api repos/md-mt/termproof-rust/rulesets --jq '.[] | select(.name == "main") | .rules[] | select(.type == "required_status_checks") | .parameters.required_status_checks[].context'

# Community health
gh api repos/md-mt/termproof-rust/community/profile --jq .health_percentage
```

Expected after the settings change: homepage `https://docs.rs/termproof`;
topics present; vulnerability/Dependabot alerts enabled; private reporting
enabled; squash-only + delete-branch-on-merge; one active `main` ruleset with
the eight required checks and a required-review count of zero (until a second
eligible reviewer exists — see promotion criteria in section 6); `main`
refusing direct pushes, force pushes and deletion; and an explicit `@md-mt`
bypass. Any deviation from section 1–6 is a finding to fix or a deliberate
change to record here first.
