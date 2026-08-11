# Feature Specification: Recipe format v1

**Feature Branch**: `001-recipe-format`

**Created**: 2026-08-11

**Status**: Draft

**Input**: First spec set for the Rust reimplementation — the contract users author against.

---

## Reading this spec

Every functional requirement is tagged and carries an authority:

- **`[BEHAVIOURAL]`** — what the tool must do. A reasonable reimplementation could satisfy
  it differently.
- **`[BYTE-EXACT]`** — the literal bytes are the requirement, because users or downstream
  tools depend on them.

**Authority** names the rung of the constitution's evidence ladder (Principle IV) the
requirement came from:

| Authority | Meaning |
|---|---|
| `SCHEMA` | `docs/recipe-schema-v1.json` in the Python repo — a published JSON Schema |
| `DOC` | `docs/recipe-format-v1.md` |
| `OBSERVED` | A recorded run of the Python implementation; see `specs/OBSERVATION-LOG.md` |
| `TEST` | A test in the Python repo that encodes the intent |
| `SOURCE` | Read from Python source because nothing higher was available — treat with suspicion |

Open questions are collected in [Open Questions](#open-questions) and in
`specs/OBSERVATION-LOG.md`. There are several. Finding them is the point.

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Author a recipe that both runtimes accept (Priority: P1)

A maintainer writes a `.recipe.json` file describing a terminal program to verify: the
command to run, the steps to drive it, and the assertions that must hold. They run it under
the Python implementation today and under the Rust implementation after the port. The recipe
must load identically under both — same fields accepted, same defaults applied, same values
rejected.

**Why this priority**: This is the surface users own. A recipe that works today and breaks on
upgrade is the single worst outcome of the port, and the review found the Rust side already
rejecting recipes Python accepts. Everything downstream (steps, assertions, reporting) reads
the loaded recipe, so a divergence here contaminates every other layer.

**Independent Test**: Load a corpus of recipe documents through both runtimes' loaders and
diff the resulting normalised recipe objects field by field, plus the accept/reject verdict
and the rejection message.

**Acceptance Scenarios**:

1. **Given** the minimal document `{"name": "r", "command": {"argv": ["true"]}}`,
   **When** loaded, **Then** loading succeeds and every optional field takes the default in
   FR-004.
2. **Given** a document with `"recipe_version": 2`, **When** loaded, **Then** loading fails
   with the message `unsupported recipe_version: 2`.
3. **Given** a document with no `recipe_version`, **When** loaded, **Then** loading succeeds
   and `recipe_version` is `1`.
4. **Given** a document with `"timeout_seconds": "45"`, **When** loaded, **Then** loading
   succeeds and `timeout_seconds` is the number `45.0`.
5. **Given** a document with `"cols": "80"`, **When** loaded, **Then** loading succeeds and
   `cols` is the integer `80`.
6. **Given** a document with no `command` key, **When** loaded, **Then** loading fails with a
   bare `KeyError: 'command'` — a leaked exception, not a message. See OQ-009.
7. **Given** `{"cols": 80.9}`, **When** loaded, **Then** loading succeeds and `cols` is `80`.
   The validator rejects the same document (User Story 2, scenario 3).

---

### User Story 2 - Validate a recipe before running it (Priority: P2)

A maintainer runs the validator against a directory of recipes in CI and gets a list of
issues, each with a path and a message, separated into errors and warnings. The validator is
stricter than the loader: it reports problems the loader would tolerate, and it checks things
the loader cannot (whether a named step action or assertion type is actually registered).

**Why this priority**: The validator is what CI gates on, so its verdicts are a contract with
every consumer's pipeline. It is separable from the loader — a runtime that loads correctly
but validates differently still runs every existing recipe.

**Independent Test**: Run both runtimes' validators over the same recipe documents and diff
the issue lists — path, message, and severity — as ordered sequences.

**Acceptance Scenarios**:

1. **Given** a document with no `recipe_version`, **When** validated, **Then** exactly one
   issue is reported: path `recipe_version`, severity `warning`, message
   `missing recipe_version; treating recipe as legacy v0.x`.
2. **Given** a document with `"cols": 0`, **When** validated, **Then** an issue is reported at
   path `cols` with message `must be a positive integer` and severity `error`.
3. **Given** a document with `"cols": 80.0`, **When** validated, **Then** an issue is reported
   at path `cols` with message `must be a positive integer` — an integral float is not an
   integer here, even though JSON Schema draft 2020-12 accepts it.
4. **Given** `{"steps": [{"action": "no_such_action"}]}`, **When** validated with the built-in
   registry, **Then** an issue is reported at path `steps[0].action` with message
   `unknown step action 'no_such_action'`.
5. **Given** `{"assertions": [{"type": "no_such_type"}]}`, **When** validated, **Then** an
   issue is reported at path `assertions[0].type` with message
   `unknown assertion type 'no_such_type'`.
6. **Given** a file that is valid JSON but not a JSON object — `[1,2]`, `"hi"`, or `null` —
   **When** validated, **Then** exactly one issue is reported: path `$`, message
   `recipe must be a JSON object`.
7. **Given** a file containing `not json`, **When** validated, **Then** exactly one issue is
   reported: path `$`, message `invalid JSON: Expecting value`.
8. **Given** `{"command": {"argv": [5]}}`, **When** validated, **Then** an issue is reported at
   path `command.argv[0]` with message `5 is not of type 'string'`.

---

### User Story 3 - Keep legacy v0.x recipes running (Priority: P3)

A maintainer with recipes written before `recipe_version` existed upgrades the tool. Their
recipes keep loading and keep validating, with a warning telling them to migrate. Recipes the
pre-#93 Python validator accepted must keep being accepted.

**Why this priority**: Explicitly promised by `docs/recipe-format-v1.md` ("Recipes without
`recipe_version` are treated as legacy v0.x recipes and remain loadable"). It is the smallest
slice and the easiest to regress, because the tolerances live in a compatibility layer rather
than in the schema.

**Independent Test**: Feed the legacy-tolerated shapes in FR-024 through both validators and
confirm neither reports an error for them.

**Acceptance Scenarios**:

1. **Given** `{"priority": null}` on an otherwise valid recipe, **When** validated, **Then**
   no error is reported for `priority`, even though the schema types it as a string.
2. **Given** `{"description": 42}`, **When** validated, **Then** no error is reported for
   `description`.
3. **Given** `{"steps": [{"action": "sleep", "name": 42}]}`, **When** validated, **Then** no
   error is reported for `steps[0].name`.
4. **Given** `{"command": {"argv": ["true"], "cwd": null}}`, **When** validated, **Then** no
   error is reported for `command.cwd`.

---

### Edge Cases

- **A field the schema does not name.** `additionalProperties: true` at the top level, inside
  `command`, inside each step, and inside each assertion. Unknown keys are carried, not
  rejected. Steps and assertions rely on this: `text`, `pattern`, `seconds`, `value`, `path`
  and `schema` are all "additional properties" as far as the schema is concerned.
- **`expect_exit_code: null`.** Not the same as absent. Absent means `0`; explicit `null`
  suppresses the implicit exit-code assertion entirely (spec 003, FR-002).
- **`timeout_seconds: 0`.** The schema says `exclusiveMinimum: 0`, so the validator reports an
  error — but the loader coerces and accepts it. Loader and validator disagree by design; see
  FR-021.
- **A boolean where a number is expected.** `float(True)` is `1.0` in Python, so
  `"timeout_seconds": true` loads as `1.0`. See OQ-005.
- **`renderers` that is not an object.** The loader raises with a fixed message; the schema
  reports a type error. Two different failure surfaces for one input.
- **`argv: []`.** The schema requires `minItems: 1`; the loader accepts it and the run fails
  later when there is no command to spawn.
- **Duplicate JSON object keys.** Accepted; last wins (Python's `json` module). Not a
  designed behaviour — see OQ-011 in spec 003.

---

## Requirements *(mandatory)*

### Document shape and identity

- **FR-001** `[BYTE-EXACT]` A recipe is a JSON document whose top-level value is an object.
  Recipe files are named with the suffix `.recipe.json`.
  **Authority**: `DOC` (`recipe-format-v1.md` §"Recipe format v1"), `SCHEMA` (`"type":
  "object"`).

- **FR-002** `[BYTE-EXACT]` The set of top-level field names, their JSON types, and their
  nesting is exactly as given in `docs/recipe-schema-v1.json`. The Rust implementation MUST
  read that schema file as its authority rather than transcribing it, so the two cannot drift.
  **Authority**: `SCHEMA`.

- **FR-003** `[BYTE-EXACT]` Required fields are `name` and `command`; `command` requires
  `argv`; each element of `steps` requires `action`; each element of `assertions` requires
  `type`. `name` has `minLength: 1`; `command.argv` has `minItems: 1` with string items.
  **Authority**: `SCHEMA` (`required` keywords).

- **FR-004** `[BYTE-EXACT]` A recipe loaded without a given optional field MUST take exactly
  these defaults:

  | Field | Default |
  |---|---|
  | `recipe_version` | `1` |
  | `description` | `""` |
  | `intent` | `""` |
  | `priority` | `"P2"` |
  | `execution` | `"scripted"` |
  | `determinism` | `"deterministic"` |
  | `ci_paths` | `[]` |
  | `checks` | `[]` |
  | `operator` | `{}` |
  | `renderers` | `{"default": []}` |
  | `command.cwd` | `null` |
  | `command.env` | `{}` |
  | `command.pty` | `true` |
  | `steps` | `[]` |
  | `assertions` | `[]` |
  | `expect_exit_code` | `0` |
  | `timeout_seconds` | `30.0` |
  | `cols` | `100` |
  | `rows` | `30` |
  | `source_path` | `null` |

  These are user-visible: they change what runs and what is asserted. `expect_exit_code`
  defaulting to `0` means every recipe that says nothing about exit codes still asserts a
  clean exit.
  **Authority**: `DOC` (§"Common optional fields") and `SOURCE`
  (`termproof/models.py:19-38`, `recipe_from_mapping`). The doc table and the source agree on
  every value; `source_path` and `renderers`' inner default are source-only.

- **FR-005** `[BEHAVIOURAL]` `source_path` is set to the path the recipe was loaded from when
  loading from a file, and is absent when loading from an in-memory mapping. It is not part
  of the authored document.
  **Authority**: `SOURCE` (`models.py:165-169`).

### Version handling

- **FR-006** `[BYTE-EXACT]` A `recipe_version` that is absent is treated as `1` by the loader.
  Any present value that is not the integer `1` MUST cause loading to fail with the message
  `unsupported recipe_version: <repr>`, where `<repr>` is the Python-`repr` rendering of the
  supplied value (`2`, `'1'`, `True`, `None`, `1.0`).
  **Authority**: `OBSERVED` — `2` → `unsupported recipe_version: 2`, `"1"` →
  `unsupported recipe_version: '1'`, `true` → `unsupported recipe_version: True` — corroborated
  by `DOC` §"Versioning". The `repr` rendering makes this a `[BYTE-EXACT]` requirement on the
  shared formatter mandated by constitution Principle VIII.

- **FR-007** `[BYTE-EXACT]` The boolean `true` MUST NOT satisfy `recipe_version == 1` even
  though Python's `True == 1`. The loader explicitly excludes `bool`.
  **Authority**: `SOURCE` (`models.py:132`, `isinstance(recipe_version, bool)`). This is a
  deliberate carve-out, not an accident: the code tests for it specifically.

- **FR-008** `[BYTE-EXACT]` The validator reports a *warning*, not an error, when
  `recipe_version` is absent, at path `recipe_version` with the message
  `missing recipe_version; treating recipe as legacy v0.x`. Any present non-`1` value is an
  *error* at the same path with the message `must be 1`.
  **Authority**: `SOURCE` (`recipe_schema.py:233-244`), corroborated by `DOC`
  ("`termproof validate` reports that as a warning").

### Loader coercion — how strict the parser is, per field

The loader and the validator are two different levels of strictness, and this is the single
largest source of the divergences the review found. FR-009 through FR-013 define the loader.

- **FR-009** `[BEHAVIOURAL]` The loader is **coercive, not strict**. For each field it applies
  the conversion below to whatever JSON value is present, and fails only if that conversion
  fails. It does not check the JSON type first.

  | Field | Conversion applied |
  |---|---|
  | `name` | none — value used as-is |
  | `description`, `intent`, `priority`, `execution`, `determinism` | none — used as-is |
  | `command.argv` | `list(...)` — any iterable |
  | `command.cwd` | none |
  | `command.env` | `dict(...)` |
  | `command.pty` | `bool(...)` — every JSON value is convertible |
  | `ci_paths`, `checks` | `list(...)` |
  | `operator` | `dict(...)` |
  | `renderers` | see FR-012 |
  | `steps`, `assertions` | `list(...)`; elements untouched |
  | `expect_exit_code` | none — used as-is |
  | `timeout_seconds` | `float(...)` |
  | `cols`, `rows` | `int(...)` |

  **Authority**: `SOURCE` (`models.py:128-162`, `recipe_from_mapping`) and `OBSERVED` (the
  loader accepts `"45"` for `timeout_seconds` and `"80"` for `cols`).

- **FR-010** `[BYTE-EXACT]` `timeout_seconds` MUST accept any value Python's `float()` accepts,
  including the JSON strings `"45"` and `"4.5e1"`, and the booleans `true` (→ `1.0`) and
  `false` (→ `0.0`). It MUST reject values `float()` rejects, and the failure message MUST be
  the one the conversion produces:
  `could not convert string to float: 'abc'` for a non-numeric string, and
  `float() argument must be a string or a real number, not 'NoneType'` for `null`.
  **Authority**: `OBSERVED` at the loader itself (`timeout_seconds: "45"` → `45.0`;
  `"abc"` → the stated `ValueError`), and the same strings again from the step layer's
  `float()` (002-FR-004). **Byte-exact under protest**: these are CPython messages, not
  TermProof's. See OQ-001.

- **FR-011** `[BYTE-EXACT]` `cols` and `rows` MUST accept any value Python's `int()` accepts:
  the string `"80"`, the float `80.9` (truncating toward zero to `80`), and the booleans.
  **Authority**: `OBSERVED` (`cols: "80"` → `80`; `cols: 80.9` → `80`) and `SOURCE`
  (`models.py:160-161`). Truncation of `80.9` to `80` looks accidental rather than designed —
  see OQ-002.

- **FR-012** `[BYTE-EXACT]` `renderers` MUST be `null` (→ `{"default": []}`) or a JSON object,
  whose keys are stringified and whose values are converted with `list(...)`. Any other type
  MUST fail loading with exactly:
  `renderers must be an object mapping renderer names to argv lists`.
  **Authority**: `OBSERVED` (`renderers: 5` → the stated message; `renderers: null` →
  `{"default": []}`). This is the only loader message TermProof authors itself rather than
  inheriting from CPython, which is why it can be held byte-exact without reservation.

- **FR-013** `[BEHAVIOURAL]` `steps` and `assertions` elements are **not** inspected at load
  time. A step with no `action`, an assertion with no `type`, and a step whose `text` is a
  number all load successfully. Their validity is decided when they execute (specs 002, 003).
  **Authority**: `SOURCE` (`models.py:156-157`) and `OBSERVED` (the step probes in
  `OBSERVATION-LOG.md` all reach step execution).

### Validator strictness

- **FR-014** `[BEHAVIOURAL]` The validator produces an ordered list of issues, each with a
  `path`, a `message`, and a `severity` of `error` or `warning`. A recipe is invalid when at
  least one issue has severity `error`. An empty list means valid.
  **Authority**: `SOURCE` (`recipe_schema.py:20-28`).

- **FR-015** `[BYTE-EXACT]` Issue paths are rendered as a dotted path with bracketed integer
  indices: the first string component bare, later string components dot-prefixed, integers as
  `[N]`. Examples: `cols`, `command.argv`, `steps[0].action`, `assertions[2].type`. A
  document-level issue uses `$`.
  **Authority**: `SOURCE` (`recipe_schema.py:51-81`, `_format_path`). CI configurations match
  on these paths, which is why they are byte-exact.

- **FR-016** `[BYTE-EXACT]` A missing required property is reported at the path of the
  *missing property*, not at its containing object: a missing top-level `command` is reported
  at `command`, not at `$`.
  **Authority**: `SOURCE` (`recipe_schema.py:70-81`, `_issue_path`), which states the intent
  explicitly ("The legacy validator reported the missing property itself").

- **FR-017** `[BYTE-EXACT]` Schema-derived messages are the `jsonschema` library's `message`
  field verbatim. A Rust implementation MUST reproduce these strings, not its own validator's
  phrasing. Observed examples:

  | Input | path | message |
  |---|---|---|
  | no `command` | `command` | `'command' is a required property` |
  | `"name": 1` | `name` | `1 is not of type 'string'` |
  | `"name": ""` | `name` | `'' should be non-empty` |
  | `"command": {"argv": []}` | `command.argv` | `[] should be non-empty` |
  | `"command": {"argv": [5]}` | `command.argv[0]` | `5 is not of type 'string'` |
  | `"renderers": 5` | `renderers` | `5 is not of type 'object'` |
  | `"steps": 5` | `steps` | `5 is not of type 'array'` |
  | `steps[0].timeout_seconds: 0` | `steps[0].timeout_seconds` | `0 is less than or equal to the minimum of 0` |
  | `steps[0]` with no `action` | `steps[0].action` | `'action' is a required property` |
  | `assertions[0]` with no `type` | `assertions[0].type` | `'type' is a required property` |

  **Authority**: `OBSERVED`, every row, under `jsonschema` 4.26.0. **This is a hard
  requirement with a real cost** — it makes `jsonschema`'s message vocabulary part of
  TermProof's public contract, including phrasings like `should be non-empty` that no Rust
  validator produces. See OQ-003.

- **FR-018** `[BYTE-EXACT]` `cols` and `rows` MUST be reported as `must be a positive integer`
  when the value is present and is not a non-boolean integer greater than zero. This check
  overrides the JSON Schema verdict: JSON Schema draft 2020-12 accepts `80.0` as an integer,
  and the validator MUST reject it.
  **Authority**: `OBSERVED` — `cols: 0`, `cols: 80.0` and `cols: true` all produce exactly
  this message — and `SOURCE` (`recipe_schema.py:171-191`), which documents the intent as
  preserving the frozen base validator's behaviour exactly.

- **FR-019** `[BYTE-EXACT]` `expect_exit_code` MUST be reported as `must be an integer or null`
  when present and not a non-boolean integer. Same override as FR-018.
  **Authority**: `OBSERVED` (`expect_exit_code: 1.0`) and `SOURCE` (`recipe_schema.py:189-191`).

- **FR-020** `[BYTE-EXACT]` Plugin-name checks report `unknown step action <repr>` at
  `steps[N].action` and `unknown assertion type <repr>` at `assertions[N].type`, where
  `<repr>` is Python-`repr` of the name (single-quoted). These checks run only when the value
  is a string; a non-string `action` is left to the schema.
  **Authority**: `OBSERVED` (`steps[0].action: unknown step action 'no_such_action'`;
  `assertions[0].type: unknown assertion type 'no_such_type'`) and `SOURCE`
  (`recipe_schema.py:247-277`). The set of known names is the runtime registry, not a fixed
  list, so this cannot live in the schema.

- **FR-021** `[BEHAVIOURAL]` The validator MAY reject documents the loader accepts. A runtime
  MUST NOT make the loader reject a document merely because the validator does. Specifically:
  `steps[0].timeout_seconds: 0` is a validator error
  (`0 is less than or equal to the minimum of 0`) and a successful load, and `cols: 80.9` is a
  validator error and a successful load that silently truncates.
  **Authority**: `OBSERVED` on both sides — the validator and loader probes were run over the
  same documents — and `SOURCE` (`models.py:159`). The two disagree, and the
  disagreement is the requirement. This is the trap the Rust port fell into: adding
  validator-grade strictness to the execution path. See spec 002 FR-004.

- **FR-022** `[BYTE-EXACT]` A file that is not valid JSON produces exactly one issue: path
  `$`, message `invalid JSON: <msg>` where `<msg>` is the decoder's message without position
  suffix (e.g. `Expecting value`, `Expecting property name enclosed in double quotes`). A file
  whose top-level value is not an object — an array, a string, or `null` — produces exactly
  one issue: path `$`, message `recipe must be a JSON object`.
  **Authority**: `OBSERVED`, all five cases. The `<msg>` half inherits CPython's JSON
  vocabulary; see OQ-001 and 003-OQ-010.

- **FR-023** `[BEHAVIOURAL]` `recipe_version` errors from the JSON Schema are suppressed; that
  field is reported only by FR-008, so it never produces two issues at one path. The same
  suppression applies to `cols`, `rows`, and `expect_exit_code`, whose verdicts come from
  FR-018/FR-019 alone.
  **Authority**: `SOURCE` (`recipe_schema.py:210-221`).

### Legacy tolerance

- **FR-024** `[BYTE-EXACT]` The validator MUST suppress schema errors for these exact inputs,
  because the pre-#93 validator accepted them:

  | Input | Rule |
  |---|---|
  | A `null` value at any of `priority`, `execution`, `determinism`, `timeout_seconds`, `cols`, `rows`, `checks`, `ci_paths`, `operator`, `renderers`, `description`, `intent` | suppressed |
  | A `null` value at `command.cwd` or `command.pty` | suppressed |
  | A `null` value at `steps[N].timeout_seconds` | suppressed |
  | *Any* value at `description` or `intent` | suppressed — legacy never type-checked them |
  | *Any* value at `steps[N].name` or `assertions[N].name` | suppressed |

  **Authority**: `OBSERVED` for `priority: null`, `description: 42`, `steps[0].name: 42`,
  `command.cwd: null` and `steps[0].timeout_seconds: null` — all clean — and `SOURCE`
  (`recipe_schema.py:84-156`), which documents each carve-out and its reason. `TEST`
  corroborates: `tests/legacy_recipe_validator.py` is retained as the frozen reference and
  `tests/test_differential_compat.py` diffs against it.

- **FR-025** `[BEHAVIOURAL]` The list in FR-024 is closed. A Rust implementation MUST NOT
  extend legacy tolerance to any other path, and MUST NOT drop any entry from it.
  **Authority**: `SOURCE` (`recipe_schema.py:88-90`, "these are the only carve-outs").

### Cross-cutting

- **FR-026** `[BEHAVIOURAL]` Nothing in loading or validating a recipe may panic, abort the
  process, or terminate other recipes running in the same invocation. Every rejection is a
  structured error or issue.
  **Authority**: Constitution Principle VI; issue #2 (RUST-009).

- **FR-027** `[BEHAVIOURAL]` No Rust `Debug` rendering may appear in any loader error message
  or validator issue message. Where a message embeds a value, it uses the shared Python-`repr`
  formatter.
  **Authority**: Constitution Principle VIII.

### Key Entities

- **Recipe** — the loaded, defaulted form of a recipe document. Fields and defaults per
  FR-004. Immutable once loaded.
- **CommandSpec** — the `command` sub-object: `argv`, `cwd`, `env`, `pty`.
- **ValidationIssue** — `path`, `message`, `severity`. The validator's unit of output.
- **Step** — an untyped JSON object with a required `action` and arbitrary extra keys.
  Interpreted by spec 002.
- **Assertion** — an untyped JSON object with a required `type` and arbitrary extra keys.
  Interpreted by spec 003.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A differential harness (issue #3) drives a corpus of at least 120 recipe
  documents through both runtimes' loaders and reports **zero** divergences in the
  accept/reject verdict, and zero in any field of the loaded recipe.
- **SC-002**: The same corpus driven through both validators reports **zero** divergences in
  the ordered issue list — path, message, and severity compared as literal strings.
- **SC-003**: The corpus covers every field in FR-004 with at least: absent, the JSON type the
  schema names, `null`, a numeric string, a boolean, and a wrong-typed value. No field is
  covered only by its happy path.
- **SC-004**: The corpus contains at least one case per row of FR-024, and each is confirmed
  to produce no error under both runtimes.
- **SC-005**: Removing any one of FR-006, FR-007, FR-012, FR-018, FR-019, FR-020 from the
  implementation causes at least one corpus case to fail. Verified by deletion, per
  constitution Principle I.
- **SC-006**: No recipe document in the corpus, however malformed, causes either runtime to
  panic or to exit other than through its normal result path.
- **SC-007**: The Rust implementation reads `docs/recipe-schema-v1.json` as a build- or
  run-time input. A drift check fails if the schema file changes and the Rust behaviour does
  not.

---

## Assumptions

- The oracle environment for every `OBSERVED` claim here is CPython 3.12 with `jsonschema`
  4.26.0. `pyproject.toml` declares `jsonschema>=4.0`, a floor rather than a pin, so FR-017's
  message table can shift under a different resolution. See OQ-003.
- The canonical schema is `docs/recipe-schema-v1.json` in the Python repo at the revision the
  port targets. The Python package also force-includes it at
  `termproof/_resources/recipe-schema-v1.json`; the two are the same file.
- The recipe format is v1 and no `recipe_version: 2` exists. A future major version is
  explicitly reserved by `DOC` §"Versioning" and is out of scope here.
- Recipes are UTF-8 encoded. Every read in the Python implementation passes
  `encoding="utf-8"` explicitly.
- The step and assertion registries are runtime configuration (`VerifierConfig`), so FR-020's
  known-name set is not fixed by this spec. The default registries are the built-ins in specs
  002 and 003.
- YAML is not a recipe format. `docs/recipe-format-v1.md` says recipes are JSON. The review
  refers to "YAML recipes" when discussing quoted scalars — that framing is wrong for the
  format, though the *coercion* concern it raises is real and is FR-009.

---

## Out of Scope

Noted as future spec work, not started here: the evidence pipeline, renderers, video backends,
reporters, the plugin protocol (`docs/plugin-protocols.md`), recipe packs
(`docs/recipe-packs.md`), and the CLI surface including `termproof validate`'s own output
formatting and exit codes.

---

## Open Questions

These are places where the Python implementation is ambiguous, underspecified, or looks
accidental. **They are deliberately not resolved here.** Freezing a guess into the spec is how
the port inherits bugs. Each needs a human decision before the corresponding requirement can
be called settled.

- **OQ-001 — CPython's error vocabulary is currently part of the contract.**
  FR-010 and FR-022 make strings like `could not convert string to float: 'abc'` and
  `Expecting value` byte-exact requirements. These are CPython messages: they are not
  TermProof's, they are not documented anywhere, and they can change between Python versions.
  A Rust implementation must hardcode them. **Decision needed**: freeze them as an explicit
  compatibility table owned by TermProof (and add a Python-side test pinning them), or declare
  them behavioural and accept that this class of diagnostic will differ across runtimes.
  This decision recurs in spec 002 (OQ-002) and spec 003 (OQ-010) and should be made once.

- **OQ-002 — `cols: 80.9` silently truncates to `80`.**
  The loader's `int()` truncates toward zero; the validator rejects the same input. So a
  recipe run without validation gets a silently different terminal width than the author
  wrote. This looks accidental. **Decision needed**: reject non-integral values at load, or
  keep truncation and document it.

- **OQ-003 — `jsonschema`'s message strings as a public contract.**
  FR-017 requires reproducing `'zeta' is a required property` and friends. This binds
  TermProof to one Python library's phrasing, across a library version bump, in a language
  that does not use that library. **Decision needed**: (a) accept the cost and pin the
  `jsonschema` version, (b) define a TermProof message table and change the Python side to
  emit it, or (c) declare validator messages behavioural and require only the path and
  severity to match.

- **OQ-004 — `argv: []` loads but cannot run.**
  The schema says `minItems: 1`; the loader accepts an empty list. The failure surfaces later
  as a spawn error rather than as a recipe error. **Decision needed**: reject at load, or
  specify the runtime failure.

- **OQ-005 — Booleans coerce to numbers everywhere.**
  `timeout_seconds: true` loads as `1.0`; `cols: true` loads as `1`. This falls out of
  Python's `bool`-is-an-`int`, not from a decision — and the loader *does* explicitly exclude
  `bool` for `recipe_version` (FR-007), which shows the author thought about it in one place
  and not the others. **Decision needed**: exclude `bool` consistently, or accept it
  consistently and say so.

- **OQ-006 — The loader has no `null` tolerance but the validator does.**
  FR-024 lets the validator pass `{"priority": null}`, and then the loader stores `None` in a
  field typed `str`. Nothing downstream is specified for that state. **Decision needed**:
  should the loader apply the FR-004 default when a tolerated field is explicitly `null`?

- **OQ-007 — Two different failure surfaces for a bad `renderers`.**
  A non-object `renderers` raises from the loader with a TermProof-authored message (FR-012);
  the validator reports a `jsonschema` type error at the same path. No other field behaves
  this way. **Decision needed**: whether the loader should raise at all, given the validator
  already covers it.

- **OQ-008 — `source_path` is a field of `Recipe` but not of the recipe format.**
  It is in the data model and not in the schema or the docs. A `result.json` consumer could
  reasonably expect it. **Decision needed**: is it internal, or part of the contract?

- **OQ-009 — The loader's own failures are leaked exceptions, not messages.**
  A document with no `command` fails with a bare `KeyError: 'command'`. The validator, given
  the same document, says `'command' is a required property` at path `command`. So the two
  layers disagree not just on strictness (FR-021) but on whether the user gets a diagnostic at
  all — and a user who loads without validating gets the worse one. **Decision needed**:
  give the loader real error messages (and decide whether they should match the validator's),
  or route all loading through the validator first.

- **OQ-010 — `validate_recipe_file` raises on a missing file instead of reporting an issue.**
  Every other failure mode in the validator returns a `ValidationIssue`; a nonexistent path
  raises `FileNotFoundError` out of the function. A CI job validating a glob that matches
  nothing, or a stale path in a recipe pack, crashes the validator rather than reporting.
  **Decision needed**: report it as an issue at path `$` (with what message?), or document
  that the caller is responsible for existence.

---

## Downstream consumer

This spec exists so the cross-runtime conformance gate (issue #3, RUST-026) can be built. The
corpus in SC-001 through SC-004 is the loader/validator half of that gate. Each acceptance
scenario above is written as concrete input plus concrete expected output so it can be encoded
as a corpus case without further interpretation.
