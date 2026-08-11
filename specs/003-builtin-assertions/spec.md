# Feature Specification: The eight built-in assertions

**Feature Branch**: `003-builtin-assertions`

**Created**: 2026-08-11

**Status**: Draft

**Input**: First spec set for the Rust reimplementation — the verdict layer.

---

## Reading this spec

Tags and authorities are as defined in `specs/001-recipe-format/spec.md` §"Reading this spec":
`[BEHAVIOURAL]` / `[BYTE-EXACT]`, with an **Authority** of `SCHEMA`, `DOC`, `OBSERVED`, `TEST`
or `SOURCE`.

Everything marked `OBSERVED` was recorded by executing the Python assertion implementations
under CPython 3.12 with `jsonschema` 4.26.0, against real files on disk. The probe harness and
raw output are in `specs/OBSERVATION-LOG.md`.

Issue #1 (RUST-008, "Implement built-in assertions") lives in this spec.

**Attention item 4 — are diagnostic strings contract? — is decided per assertion in FR-021.**
It is the highest-value decision in this spec set, so it gets its own requirement rather than
being implied by the tags.

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Get a verdict on a run (Priority: P1)

A recipe author lists the conditions a successful run must satisfy. After the steps finish,
each assertion is evaluated in order and produces a pass or fail with a diagnostic. The run's
overall verdict and score follow from the results. Both runtimes produce the same verdicts and
the same diagnostics for the same run.

**Why this priority**: The verdict is the product. Everything else — the PTY, the steps, the
renderers — exists to produce it. The review found 78 of 107 assertion cases diverging and 15
outright pass/fail flips, which means the two runtimes disagreed about whether the software
under test worked.

**Independent Test**: Feed both runtimes an identical `(recipe, screen, raw_output, exit_code,
filesystem)` tuple and diff the `assertions` array, `passed`, and `score` of `result.json`.

**Acceptance Scenarios**:

1. **Given** raw output `RAW output` and `{"type": "output_contains", "value": "RAW"}`,
   **When** evaluated, **Then** the result is `name: "output_contains"`, `passed: true`,
   `detail: "contains 'RAW'"`.
2. **Given** the same output and `{"type": "output_contains", "value": "zzz"}`, **When**
   evaluated, **Then** `passed: false`, `detail: "contains 'zzz'"` — the failure detail states
   the *expectation*, not the outcome, and reads identically to the passing case.
3. **Given** exit code `0` and `{"type": "exit_code", "value": 1}`, **When** evaluated,
   **Then** `passed: false`, `detail: "expected 1, got 0"`.
4. **Given** a recipe with `expect_exit_code: 0` and two explicit assertions, **When** the run
   finishes, **Then** the `assertions` array has three entries: the two explicit ones in
   recipe order, then a synthetic `exit_code` assertion.
5. **Given** two assertions of which one fails, **When** the run finishes, **Then** `score` is
   `0.5` and `passed` is `false`.

---

### User Story 2 - A malformed assertion fails the assertion, not the run (Priority: P1)

A recipe author writes an assertion with a missing key, an unknown type, or a wrong-typed
value. That assertion reports a failure with a diagnostic; every other assertion still runs
and the process does not crash.

**Why this priority**: Equal-first, and for a worse reason than in spec 002 — in the Python
oracle these inputs do not merely crash the *step*, they raise out of the assertion loop, out
of the execution mode, and out to the CLI. The user gets a traceback and **no `result.json`
at all**: every passing assertion in the run is lost along with the failing one.

**Independent Test**: Drive the malformed-assertion corpus in FR-019 through both runtimes and
confirm every case yields a structured `AssertionResult` and a complete `result.json`.

**Acceptance Scenarios**:

1. **Given** `{"type": "output_contains"}` with no `value`, **When** evaluated, **Then** a
   failed `AssertionResult` is produced and the run completes. (The Python oracle raises
   `KeyError: 'value'` and aborts — see FR-020 and OQ-001.)
2. **Given** `{"type": "no_such_type"}`, **When** evaluated, **Then** a failed
   `AssertionResult` is produced and the run completes. (Python raises
   `ValueError: unknown assertion type: no_such_type` and aborts.)
3. **Given** an assertion object with no `type`, **When** evaluated, **Then** the run
   completes. (Python raises `KeyError: 'type'` and aborts.)
4. **Given** `{"type": "output_contains", "value": 5}`, **When** evaluated, **Then** a failed
   `AssertionResult` is produced and the run completes. (Python raises `TypeError` and
   aborts.)
5. **Given** a run whose third of five assertions is malformed, **When** it finishes, **Then**
   `result.json` exists and contains five assertion results.

---

### User Story 3 - Validate structured output against a JSON Schema (Priority: P2)

A recipe author verifies that a program's stdout is JSON conforming to a schema, supplied
inline or as a file path relative to the recipe's working directory. Failures say what was
wrong and where.

**Why this priority**: `json_schema` is the most complex assertion and has the largest
diagnostic surface — nine distinct failure messages, four of them from libraries. It is also
the most separable: recipes that do not use it are unaffected.

**Independent Test**: Drive the `json_schema` corpus in FR-016 through both runtimes and diff
the detail strings literally.

**Acceptance Scenarios**:

1. **Given** raw output `{"a": 1}` and `{"type": "json_schema", "schema": {"type":
   "object"}}`, **When** evaluated, **Then** `passed: true`,
   `detail: "matches JSON schema"`.
2. **Given** raw output `{"a": 1}` and schema `{"type": "array"}`, **When** evaluated, **Then**
   `passed: false`, `detail: "schema validation failed: {'a': 1} is not of type 'array'"` —
   note the instance is rendered with Python `repr`, so the JSON object appears as a Python
   dict with single quotes.
3. **Given** raw output `{"a": 1}` and schema `{"properties": {"a": {"type": "string"}}}`,
   **When** evaluated, **Then** `detail: "schema validation failed at a: 1 is not of type
   'string'"`.
4. **Given** raw output `not json`, **When** evaluated, **Then**
   `detail: "invalid JSON output: Expecting value"` — the decoder's message with its
   line/column suffix stripped.
5. **Given** `{"type": "json_schema", "schema_path": "nope.json"}`, **When** evaluated,
   **Then** `detail` is `schema file unreadable: [Errno 2] No such file or directory: '<abs
   path>'`, embedding the resolved absolute path.
6. **Given** raw output `NaN`, **When** evaluated against schema `{}`, **Then**
   `passed: true` — Python's JSON decoder accepts `NaN`, `Infinity` and `-Infinity`, which
   strict JSON does not.

---

### Edge Cases

- **A failing contains-assertion's detail is indistinguishable from a passing one.**
  `contains 'zzz'` is emitted whether or not the output contains `zzz`. The `passed` flag
  carries the whole verdict. See OQ-004.
- **`exit_code` compares with Python `==`, across types.** `true` equals `1`; `0.0` equals
  `0`; `"0"` does **not** equal `0`, yet its detail reads `expected 0, got 0`. See OQ-003.
- **An explicit `exit_code` assertion does not suppress the synthetic one.** Both run, and can
  contradict each other. See FR-018.
- **`expect_exit_code: null` suppresses the synthetic assertion entirely** — the only way to
  run a recipe that asserts nothing about the exit code.
- **`file_contains` treats a missing file as the empty string**, so it never reports "no such
  file", and an empty `value` against a missing file *passes*.
- **`file_exists`'s detail is the resolved path and nothing else** — no verb, no verdict.
- **`file_exists` does not normalise `..`**, so `sub/../exists.txt` fails even when
  `exists.txt` is present.
- **`file_exists` with `command.cwd: null` resolves against the *process*'s working
  directory**, and emits a relative path in the detail.
- **`file_exists` stringifies its value**, so `value: 5` becomes the path component `5` rather
  than an error.
- **Duplicate JSON keys in the output are accepted; the last wins.** Python's decoder does not
  reject them.
- **A UTF-8 BOM is a decode failure**, with its own message naming `utf-8-sig`.
- **`detail` overrides work on four of the eight assertions.** `output_contains`,
  `output_not_contains`, `screen_contains` and `screen_not_contains` honour a `detail` key;
  `file_contains` uses the same helper but does not pass it through; `exit_code`,
  `file_exists` and `json_schema` never look at it. See OQ-005.

---

## Requirements *(mandatory)*

### The assertion contract

- **FR-001** `[BEHAVIOURAL]` An assertion is an object with a required `type` naming a
  registered assertion implementation, plus implementation-specific keys. Evaluating one
  produces an **AssertionResult** with three fields: `name`, `passed`, `detail`. Unlike a
  StepResult there is no `screen`.
  **Authority**: `SOURCE` (`models.py`, `AssertionResult`; `builtin_assertions.py`).

- **FR-002** `[BYTE-EXACT]` An assertion's `name` is the value of its `name` key if present,
  otherwise the assertion's **type name** — `output_contains`, `exit_code`, and so on. Unlike
  steps (002-FR-002) there is no index prefix, so several assertions of the same type in one
  recipe all carry the same `name`.
  **Authority**: `OBSERVED` and `SOURCE` (`assertion.get("name", self.name)` in every
  assertion).

- **FR-003** `[BEHAVIOURAL]` Every assertion is evaluated against the same five inputs: the
  loaded recipe, the assertion object, the final rendered `screen`, the accumulated
  `raw_output`, and the process `exit_code` (which may be absent). Assertions do not run
  during the steps and cannot see intermediate state.
  **Authority**: `SOURCE` (the `evaluate` signature shared by all eight).

### The contains family

- **FR-004** `[BYTE-EXACT]` Four assertions share one implementation, differing only in the
  haystack and the polarity:

  | type | haystack | passes when |
  |---|---|---|
  | `output_contains` | `raw_output` | needle is present |
  | `output_not_contains` | `raw_output` | needle is absent |
  | `screen_contains` | `screen` | needle is present |
  | `screen_not_contains` | `screen` | needle is absent |

  The needle is the assertion's `value` (**required**). The default detail is
  `contains <repr(value)>` for the positive forms and `does not contain <repr(value)>` for the
  negative — **in both the passing and the failing case**. An empty needle is always present,
  so `output_contains` with `value: ""` passes and `output_not_contains` with `value: ""`
  fails.
  **Authority**: `OBSERVED`, all four types, both polarities, and `SOURCE`
  (`builtin_assertions.py:13-24`, `_contains`).

- **FR-005** `[BYTE-EXACT]` A `detail` key on any of the four replaces the generated detail
  entirely, for both outcomes. An empty-string `detail` does **not** override — the
  implementation uses `custom_detail or <generated>`, so `""` falls through to the generated
  form.
  **Authority**: `OBSERVED` (`{"detail": "my detail"}` → `my detail`) and `SOURCE`
  (`builtin_assertions.py:23`). The falsy-empty-string carve-out is `SOURCE`-only and looks
  accidental; see OQ-005.

- **FR-006** `[BYTE-EXACT]` The needle is rendered with Python `repr`, which means:
  `'zzz'`; `"it's"` (flips to double quotes); `'say "hi"'`; `'it\'s "x"'` (both quote
  characters present → single quotes with the apostrophe escaped); `'a\nb'`; `'a\tb'`;
  `'café'` (non-ASCII is **not** escaped); `'\x1b[0m'` (control characters are).
  **Authority**: `OBSERVED`, every listed case. This is the same shared formatter constitution
  Principle VIII requires, and the same one 002-FR-020 depends on.

- **FR-007** `[BYTE-EXACT]` `raw_output` is searched by the `output_*` forms and `screen` by
  the `screen_*` forms. Neither searches the other, and neither searches a concatenation.
  **Authority**: `OBSERVED` (`screen_contains 'RAW'` fails when `RAW` is only in raw output).

### `exit_code`

- **FR-008** `[BYTE-EXACT]` `exit_code` reads `value` (**required**) and passes when the
  process's exit code equals it under Python's `==`, which compares across numeric types:

  | `value` | actual | `passed` | `detail` |
  |---|---|---|---|
  | `0` | `0` | `true` | `expected 0, got 0` |
  | `1` | `0` | `false` | `expected 1, got 0` |
  | `true` | `1` | `true` | `expected True, got 1` |
  | `false` | `0` | `true` | `expected False, got 0` |
  | `0.0` | `0` | `true` | `expected 0.0, got 0` |
  | `"0"` | `0` | `false` | `expected 0, got 0` |
  | `0` | absent | `false` | `expected 0, got None` |
  | `null` | absent | `true` | `expected None, got None` |

  **Authority**: `OBSERVED`, every row.

- **FR-009** `[BYTE-EXACT]` The detail renders both values with Python `str`, not `repr`: a
  boolean appears as `True`/`False`, an absent exit code as `None`, and a **string** `"0"`
  appears as bare `0`. The `"0"` row above therefore fails while printing a detail that reads
  as a match. This is type-blind and actively misleading, and it is nonetheless the observed
  contract. See OQ-003.
  **Authority**: `OBSERVED` and `SOURCE` (`f"expected {value}, got {exit_code}"`).

### File assertions

- **FR-010** `[BEHAVIOURAL]` Both file assertions resolve a path the same way: an absolute
  path is used as given; a relative path is joined to `command.cwd`, or to `.` when `cwd` is
  `null`. Resolution is lexical joining plus the path library's own normalisation — it does
  **not** canonicalise, resolve symlinks, or collapse `..`.
  **Authority**: `OBSERVED` and `SOURCE` (`builtin_assertions.py:27-32`, `_recipe_path`).

- **FR-011** `[BYTE-EXACT]` Path normalisation is exactly what Python's `pathlib` does when
  joining: a leading `./` is dropped, duplicate separators collapse, and a trailing separator
  on `cwd` is dropped — but `..` segments are **preserved**. Concretely, with
  `cwd = /tmp/fx`:

  | `value` | resolved path | exists? |
  |---|---|---|
  | `/tmp/fx/exists.txt` | `/tmp/fx/exists.txt` | yes |
  | `exists.txt` | `/tmp/fx/exists.txt` | yes |
  | `./exists.txt` | `/tmp/fx/exists.txt` | yes |
  | `sub/../exists.txt` | `/tmp/fx/sub/../exists.txt` | **no** |
  | `""` | `/tmp/fx` | yes (the directory) |
  | `5` | `/tmp/fx/5` | no |

  With `cwd = /tmp/fx//`, `exists.txt` still resolves to `/tmp/fx/exists.txt`.
  With `cwd = null`, `exists.txt` resolves to the relative path `exists.txt`.
  **Authority**: `OBSERVED`, every row. See OQ-006 on `..`.

- **FR-012** `[BYTE-EXACT]` `file_exists` reads `value` (**required**), stringifies it, and
  passes when the resolved path exists. Its `detail` is **the resolved path and nothing
  else** — no verb, no verdict, no quoting — for both outcomes. With `cwd: null` the detail is
  a relative path.
  **Authority**: `OBSERVED` and `SOURCE` (`builtin_assertions.py:162-178`).

- **FR-013** `[BYTE-EXACT]` `file_contains` reads `path` (**required**) and `value`
  (**required**). It reads the resolved file as UTF-8, substituting the **empty string** if
  the path does not exist, then applies the positive contains check of FR-004. Detail is
  therefore `contains <repr(value)>`, with no mention of the path and no distinct
  file-missing diagnostic. An empty `value` against a missing file passes.
  **Authority**: `OBSERVED` and `SOURCE` (`builtin_assertions.py:181-199`). See OQ-007.

- **FR-014** `[BEHAVIOURAL]` `file_contains` does **not** honour a `detail` override, even
  though it calls the same helper — the override argument is simply not passed.
  **Authority**: `SOURCE` (`builtin_assertions.py:194-199`). Source-only, and an inconsistency
  rather than a decision; see OQ-005.

### `json_schema`

- **FR-015** `[BEHAVIOURAL]` `json_schema` parses `raw_output` — whitespace-stripped — as JSON
  and validates it against a schema. The schema is resolved in this order, first match wins:
  1. `schema_path`, if present and not `null`: stringified and resolved per FR-010, read as
     UTF-8, parsed as JSON.
  2. `schema`, if it is an object: used directly.
  3. `schema`, if it is a string: resolved as a path exactly as in (1).
  4. otherwise: a fixed error.

  A `schema_path` of `null` falls through to `schema`.
  **Authority**: `OBSERVED` (all four branches, plus the `null` fallthrough) and `SOURCE`
  (`builtin_assertions.py:35-59`, `_json_schema`).

- **FR-016** `[BYTE-EXACT]` The nine possible details:

  | Situation | `passed` | `detail` |
  |---|---|---|
  | instance conforms | `true` | `matches JSON schema` |
  | instance violates the schema at the root | `false` | `schema validation failed: <library message>` |
  | instance violates it at a path | `false` | `schema validation failed at <path>: <library message>` |
  | output is not valid JSON | `false` | `invalid JSON output: <decoder message>` |
  | schema file cannot be read | `false` | `schema file unreadable: <OS error>` |
  | schema file is not valid JSON | `false` | `invalid schema JSON: <decoder message>` |
  | schema is itself invalid | `false` | `invalid schema: <library message>` |
  | no usable schema supplied | `false` | `json_schema requires an object schema or schema path` |

  Only the last is TermProof's own wording. Worked examples, all observed:

  - `schema validation failed: {'a': 1} is not of type 'array'`
  - `schema validation failed: 'zeta' is a required property`
  - `schema validation failed at a: 1 is not of type 'string'`
  - `schema validation failed at 11: 2 is not of type 'string'`
  - `invalid JSON output: Expecting value`
  - `invalid JSON output: Expecting property name enclosed in double quotes`
  - `invalid JSON output: Unexpected UTF-8 BOM (decode using utf-8-sig)`
  - `schema file unreadable: [Errno 2] No such file or directory: '/tmp/fx/nope.json'`
  - `invalid schema JSON: Expecting property name enclosed in double quotes`
  - `invalid schema: 'nope' is not valid under any of the given schemas`

  **Authority**: `OBSERVED`, every line. **Byte-exact under protest** for everything but the
  last row — see OQ-010.

- **FR-017** `[BYTE-EXACT]` The details in FR-016 have four properties a reimplementation must
  reproduce deliberately:
  1. **The instance is rendered with Python `repr`**, so a JSON object appears as a Python
     dict: `{'a': 1}`, not `{"a": 1}`.
  2. **The decoder message is `msg` alone**, with the ` : line 1 column 1 (char 0)` suffix
     stripped.
  3. **The error path is joined with `.` and includes array indices as bare integers**, so an
     error at element 11 of an array renders `at 11`, not `at [11]` — which is *not* the path
     syntax the recipe validator uses (001-FR-015).
  4. **When several schema errors apply, exactly one is reported**, chosen by `jsonschema`'s
     `best_match` heuristic — not the first, not the lexicographically smallest path. With
     schema `{"required": ["zeta", "alpha"]}` against `{}`, the reported error is about
     `'zeta'`.

  **Authority**: `OBSERVED` for (1)–(3); `SOURCE` for (4) (`jsonschema.validate` calls
  `best_match(iter_errors(...))` internally), corroborated by the observed `'zeta'` result.
  Property (4) is the hardest to reproduce and the easiest to get subtly wrong.

- **FR-018** `[BYTE-EXACT]` The instance is parsed with Python's JSON decoder, which is more
  permissive than the JSON specification: `NaN`, `Infinity` and `-Infinity` are accepted as
  numbers, both bare and nested; duplicate object keys are accepted with the last occurrence
  winning; leading and trailing whitespace is stripped before parsing. It is stricter in one
  respect: a UTF-8 BOM is rejected.
  **Authority**: `OBSERVED`, every case.

### Evaluation order, the implicit assertion, and scoring

- **FR-019** `[BYTE-EXACT]` The assertion list evaluated for a run is the recipe's
  `assertions` array in order, followed by a synthetic `{"type": "exit_code", "value":
  <expect_exit_code>}` appended **if and only if** `expect_exit_code` is not `null`. Since
  `expect_exit_code` defaults to `0` (001-FR-004), a recipe that says nothing gets the
  synthetic assertion.

  | recipe | evaluated list |
  |---|---|
  | `expect_exit_code: 0`, two explicit | the two, then `{"type":"exit_code","value":0}` |
  | `expect_exit_code: null`, one explicit `exit_code` | just the explicit one |
  | `expect_exit_code: 0`, one explicit `exit_code` with `value: 3` | the explicit `value: 3`, then the synthetic `value: 0` |

  An explicit `exit_code` assertion does **not** suppress the synthetic one; the last row
  produces two `exit_code` results that necessarily contradict each other, both named
  `exit_code`.
  **Authority**: `OBSERVED` (the ordering probe) and `SOURCE` (`runner.py`,
  `evaluate_assertions`). See OQ-002.

- **FR-020** `[BEHAVIOURAL]` **No assertion, and no assertion input, may terminate the run.**
  Every failure produces an `AssertionResult` with `passed: false` and a `detail`, and
  evaluation proceeds to the next assertion.
  **Authority**: Constitution Principle VI; issue #1. **This requirement supersedes the
  oracle** under constitution Principle III. In Python, every one of these aborts the entire
  run — the assertion loop has no handler, and neither does the execution mode above it, so
  the exception reaches the CLI and **no `result.json` is written at all**:

  | input | Python raises |
  |---|---|
  | assertion with no `type` | `KeyError: 'type'` |
  | `{"type": "no_such_type"}` | `ValueError: unknown assertion type: no_such_type` |
  | `{"type": null}` | `ValueError: unknown assertion type: None` |
  | `{"type": "output_contains"}` with no `value` | `KeyError: 'value'` |
  | `{"type": "exit_code"}` with no `value` | `KeyError: 'value'` |
  | `{"type": "file_contains"}` with no `path` | `KeyError: 'path'` |
  | `{"type": "output_contains", "value": 5}` | `TypeError: 'in <string>' requires string as left operand, not int` |
  | `{"type": "output_contains", "value": null}` | `TypeError: 'in <string>' requires string as left operand, not NoneType` |

  All eight are defects. The Rust behaviour — a structured failure — is correct, and the
  Python side is what needs to change. **The replacement diagnostics are not yet decided**;
  see OQ-001. Note the contrast with steps, where a bad *value* is already contained and only
  a missing `action` escapes: assertions contain nothing at all.

- **FR-021** `[BEHAVIOURAL]` **Attention item 4 — which diagnostics are contract.** Per
  assertion:

  | assertion | detail is | rationale |
  |---|---|---|
  | `output_contains`, `output_not_contains`, `screen_contains`, `screen_not_contains` | **`[BYTE-EXACT]`** | It embeds a user-supplied value via `repr` and is user-overridable, so consumers do match on it. FR-004/FR-006. |
  | `exit_code` | **`[BYTE-EXACT]`** | Short, fully determined by two values, no foreign strings. FR-008/FR-009. |
  | `file_exists` | **`[BYTE-EXACT]`**, with the resolved path itself `[BEHAVIOURAL]` | The *shape* (path only) is contract; the absolute path obviously varies by machine, so a differential harness compares it after substituting the fixture root. FR-012. |
  | `file_contains` | **`[BYTE-EXACT]`** | Same helper, same reasoning as the contains family. FR-013. |
  | `json_schema`, the `matches JSON schema` and `json_schema requires an object schema or schema path` details | **`[BYTE-EXACT]`** | TermProof's own strings. FR-016. |
  | `json_schema`, the six details embedding a library or OS message | **`[BYTE-EXACT]` under protest** | They are `jsonschema`, CPython and libc strings. Held byte-exact today because a conformance gate must diff *something*, but this is exactly what OQ-010 asks a human to decide. |

  The *prefixes* are unreservedly contract in every row: `contains `, `does not contain `,
  `expected …, got `, `matches JSON schema`, `schema validation failed`, `invalid JSON
  output: `, `schema file unreadable: `, `invalid schema JSON: `, `invalid schema: `. Only the
  interpolated foreign text is in question.
  **Authority**: This requirement is a decision, not an observation. It is grounded in
  `OBSERVED` behaviour throughout and in constitution Principle V.

- **FR-022** `[BYTE-EXACT]` A run's `score` is `1.0` when the evaluated assertion list is
  empty; otherwise `1.0` when all assertions passed, and `passed_count / total_count`
  otherwise. `total_count` includes the synthetic exit-code assertion.
  **Authority**: `SOURCE` (`models.py:172`, `score_from_assertions`). Note the first branch:
  a recipe with `expect_exit_code: null` and no assertions scores `1.0` while asserting
  nothing.

- **FR-023** `[BYTE-EXACT]` A run's overall `passed` is true only when **every** step passed
  **and** every assertion passed. A failing step fails the run even if all assertions pass.
  **Authority**: `SOURCE` (`runner.py:198`).

- **FR-024** `[BEHAVIOURAL]` The assertion registry is extensible: a plugin may register a
  type name, and dispatch MUST consult the registry rather than a closed set.
  **Authority**: `SOURCE` (`runner.py`, `_build_assertion_registry` from `VerifierConfig`).
  Same constraint as 002-FR-027, same open question (002-OQ-009).

- **FR-025** `[BEHAVIOURAL]` No Rust `Debug` rendering appears in any `detail`; embedded
  values use the shared Python-`repr` formatter.
  **Authority**: Constitution Principle VIII.

### Key Entities

- **Assertion** — an object with `type` plus implementation-specific keys, loaded by spec 001
  without inspection.
- **AssertionResult** — `name`, `passed`, `detail`. Serialised into `result.json`'s
  `assertions` array.
- **Evaluated assertion list** — the recipe's array plus the synthetic exit-code assertion
  (FR-019). This, not the recipe's array, is what scoring and reporting see.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A differential harness drives at least 107 assertion cases through both runtimes
  and reports **zero** divergences in `passed`, and zero in `detail` compared as literal
  strings after substituting the fixture root into any absolute path. The review's baseline
  was 29 agreements out of 107, with 15 pass/fail flips.
- **SC-002**: The corpus covers every row of every table in FR-004, FR-008, FR-011, FR-016 and
  FR-020, and every worked example in FR-016 — at least one case per row.
- **SC-003**: All eight assertion types are exercised by at least one passing and at least one
  failing case. Per constitution Principle I, deleting any single dispatch arm makes at least
  one case fail; verified by deletion.
- **SC-004**: **Zero** aborted runs across the corpus in either runtime: every case produces a
  complete `result.json` containing one result per assertion in the evaluated list. Since the
  Python oracle currently fails this for the eight rows of FR-020, the Python side is fixed as
  part of satisfying it.
- **SC-005**: The ordering corpus covers all three rows of FR-019 and asserts the exact length
  and order of the `assertions` array, not just its contents.
- **SC-006**: A scoring corpus covers `0` assertions, `1` passing, `1` failing, `2` of which
  `1` fails, and `3` of which `2` fail, and diffs `score` and `passed` exactly.
- **SC-007**: A `best_match` conformance suite covers at least 10 schemas that produce
  multiple simultaneous errors and confirms both runtimes select the same one.
- **SC-008**: The `repr` conformance suite required by 002-SC-006 also covers dicts and lists,
  since FR-017 renders the JSON instance with `repr`.
- **SC-009**: No `detail` produced anywhere in the corpus contains a newline character.

---

## Assumptions

- The oracle environment is CPython 3.12 with `jsonschema` 4.26.0. `pyproject.toml` declares
  `jsonschema>=4.0`, a floor rather than a pin, so FR-016's library messages and FR-017's
  `best_match` selection can shift under a different resolution. Pinning is proposed in
  OQ-010.
- Assertions were probed by calling the implementation classes directly against real temporary
  files, with the runner's dispatch and its (absent) error handling reproduced faithfully in
  the harness. Assertions are pure with respect to the session, so no child process is needed
  — unlike steps, where a stub session hid a real failure.
- `raw_output` and `screen` are already-decoded text by the time assertions see them. Their
  encoding is the session layer's problem, out of scope here.
- Filesystem case-sensitivity affects `file_exists` and `file_contains` and is not specified.
  The macOS default is case-insensitive; a Linux CI runner is not. The corpus must avoid
  case-varying fixtures or the gate will be flaky.

---

## Out of Scope

Noted as future spec work, not started here: the evidence pipeline and `result.json`'s
top-level shape, renderers, video backends, reporters, the plugin protocol, and the CLI
surface including the process exit code a failed run produces.

---

## Open Questions

- **OQ-001 — Eight malformed-assertion inputs destroy the entire run, and the replacement
  diagnostics are undecided.**
  FR-020 supersedes the oracle on behaviour: these must become failed assertions. It cannot
  supersede it on *text*, because the oracle produces none — it produces a traceback. This is
  worse than the equivalent step defect (002-OQ-008) because it discards a complete run's
  results, including every assertion that already passed, and because there are eight ways in
  rather than one. **Decision needed**: the exact `name` and `detail` for each row of FR-020's
  table, plus the matching fix on the Python side. Suggested shapes, needing sign-off:
  `output_contains requires a 'value'`, `unknown assertion type 'no_such_type'`,
  `assertion requires a 'type'`, `output_contains value must be a string, got int` — following
  the pattern `wait_for_regex` and `json_schema` already set.

- **OQ-002 — An explicit `exit_code` assertion does not suppress the synthetic one.**
  A recipe that writes `{"type": "exit_code", "value": 3}` and forgets to set
  `expect_exit_code: 3` gets two contradictory `exit_code` results, both named `exit_code`,
  one of which must fail — so the run can never pass. The author's intent is unmistakable and
  the tool does the opposite. **Decision needed**: suppress the synthetic assertion when an
  explicit `exit_code` assertion is present, or keep the current behaviour and make the
  validator warn.

- **OQ-003 — `exit_code`'s detail is type-blind and can be actively misleading.**
  `value: "0"` against exit code `0` fails while printing `expected 0, got 0`. A user reading
  the report sees a contradiction and has no way to diagnose it. Separately, `value: true`
  matching exit code `1` is Python's `bool`-is-an-`int` leaking into a user-facing comparison.
  **Decision needed**: render with `repr` so `'0'` is visibly a string, and/or reject
  non-integer `value` at validation time (the schema does not currently constrain an
  assertion's `value` at all). This is the same `bool` question as 001-OQ-005.

- **OQ-004 — A contains-assertion's detail does not say what happened.**
  `contains 'zzz'` is emitted identically whether the assertion passed or failed. The detail
  restates the expectation, so a report reader must consult `passed` for every row, and a
  failure gives no information at all about what the output *did* contain. **Decision
  needed**: is the detail meant to be an expectation label or a diagnosis? If the latter, a
  failing case should say something like `does not contain 'zzz'` with the polarity flipped by
  outcome — which is a breaking change to a string many consumers likely match on.

- **OQ-005 — The `detail` override is honoured by four of eight assertions, and an empty
  override silently does nothing.**
  `file_contains` calls the same helper but does not forward the override; `exit_code`,
  `file_exists` and `json_schema` ignore it entirely. And because the implementation uses
  `custom_detail or <generated>`, a `detail` of `""` falls through to the generated string
  rather than producing an empty detail. Neither looks deliberate. **Decision needed**: make
  the override universal (or explicitly document it as contains-family-only), and decide
  whether `""` is a valid override.

- **OQ-006 — `file_exists` does not collapse `..`.**
  `sub/../exists.txt` fails even when `exists.txt` exists, because the path is joined
  lexically and never normalised. A recipe author writing a path relative to a subdirectory
  gets a false negative and a detail showing the un-collapsed path. **Decision needed**:
  normalise lexically (collapse `..`), fully canonicalise (which also resolves symlinks and
  changes behaviour for existing recipes), or freeze the current behaviour and document it.

- **OQ-007 — `file_contains` cannot report a missing file.**
  A missing file is read as `""`, so the assertion reports `contains 'x'` / `false` — exactly
  what an existing file lacking `x` reports. Worse, `value: ""` against a missing file
  *passes*. A recipe that verifies an output artefact was written cannot distinguish "the file
  is wrong" from "the file was never created". **Decision needed**: add a distinct
  `file not found: <path>` detail (and decide whether `value: ""` should still pass), or
  document the conflation.

- **OQ-008 — Python's JSON decoder is not JSON.**
  FR-018 makes `NaN`, `Infinity`, `-Infinity` and duplicate keys part of the contract for
  `json_schema`'s instance parsing. A recipe asserting that a program emits valid JSON will
  pass on output that no other JSON parser accepts, which is close to the opposite of the
  assertion's purpose. Rust's `serde_json` rejects all of them by default. **Decision
  needed**: match Python's permissiveness (requiring deliberate configuration in Rust), or
  tighten Python to strict JSON — a behaviour change that could flip existing recipes from
  pass to fail.

- **OQ-009 — `json_schema` never states which JSON Schema draft it uses.**
  `jsonschema.validate` selects a validator from the schema's `$schema` keyword and falls back
  to the library's latest supported draft when absent. So a schema with no `$schema` is
  validated against whichever draft the installed `jsonschema` happens to prefer, and a
  library upgrade can change a recipe's verdict without the recipe changing. Note the recipe
  schema itself (001) *does* declare draft 2020-12 — the inconsistency is only here.
  **Decision needed**: specify a default draft explicitly and pass it, rather than inheriting
  the library's default.

- **OQ-010 — `jsonschema`, CPython and libc strings are currently the contract.**
  Six of `json_schema`'s nine details embed a foreign message: `is not of type 'array'`,
  `'zeta' is a required property`, `Expecting value`,
  `Unexpected UTF-8 BOM (decode using utf-8-sig)`, `[Errno 2] No such file or directory`,
  `'nope' is not valid under any of the given schemas`. FR-017's `best_match` selection is
  likewise a library heuristic, not a documented rule. A Rust implementation must reproduce
  another project's validator messages *and* its error-ranking heuristic. This is the largest
  single cost in the spec set. **Decision needed**: (a) pin `jsonschema` and freeze the
  strings in a TermProof-owned table with Python-side tests, (b) define TermProof's own
  message vocabulary and change Python to emit it, or (c) declare these details behavioural
  and require only `passed` plus the prefix to match. Same decision as 001-OQ-001 and
  002-OQ-002 — make it once for all three.
