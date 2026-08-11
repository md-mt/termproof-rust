# Feature Specification: The seven built-in steps

**Feature Branch**: `002-builtin-steps`

**Created**: 2026-08-11

**Status**: Draft

**Input**: First spec set for the Rust reimplementation — the step semantics a recipe drives.

---

## Reading this spec

Tags and authorities are as defined in `specs/001-recipe-format/spec.md` §"Reading this spec":
`[BEHAVIOURAL]` / `[BYTE-EXACT]`, with an **Authority** of `SCHEMA`, `DOC`, `OBSERVED`, `TEST`
or `SOURCE`.

Everything marked `OBSERVED` was recorded by executing the Python step implementations under
CPython 3.12 with `pexpect` 4.9.0 and `ptyprocess` 0.7.0. The probe harnesses, the raw output,
and what each probe could and could not see are in `specs/OBSERVATION-LOG.md`.

Issue #2 (RUST-009, "Contain step and execution failures") lives in this spec.

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Drive a terminal program to a known state (Priority: P1)

A recipe author writes a sequence of steps that types into a program, waits for it to respond,
and moves on. Each step reports pass or fail with a human-readable detail, and the sequence
continues according to the failure policy. The same recipe run under either runtime produces
the same step results in the same order.

**Why this priority**: This is what the tool is for. Steps are also where the review found the
most divergence — 84 of 110 cases — because several step details embed a formatted value and
the formatting was wrong.

**Independent Test**: Run a fixed recipe against a fixed program under both runtimes and diff
the `steps` array of `result.json` field by field.

**Acceptance Scenarios**:

1. **Given** a session whose output contains `hello world` and the step
   `{"action": "wait_for_text", "text": "hello"}`, **When** executed, **Then** the result is
   `passed: true`, `name: "1:wait_for_text"`, `detail: "found 'hello'"`.
2. **Given** a session whose output does not contain `zzz` and the step
   `{"action": "wait_for_text", "text": "zzz", "timeout_seconds": 0.05}`, **When** executed,
   **Then** the result is `passed: false`, `detail: "timed out waiting for 'zzz'"` — with no
   timeout value in the string.
3. **Given** the step `{"action": "send_line", "text": "ls"}`, **When** executed, **Then** the
   bytes `ls\r` are written to the child and the result is `passed: true`,
   `detail: "sent line"` — a constant, with no echo of the text.
4. **Given** the step `{"action": "press", "key": "Enter"}`, **When** executed, **Then** the
   byte `\r` is written and the result is `passed: true`, `detail: "pressed Enter"` — the key
   as authored, unquoted, not lowercased.
5. **Given** any step with `{"name": "my step"}`, **When** executed, **Then** the result's
   `name` is `my step` and not the positional default.

---

### User Story 2 - A malformed step fails the step, not the run (Priority: P1)

A recipe author makes a mistake — a missing key, a wrong type, a nonsensical timeout. The step
reports a failure with a diagnostic, the remaining steps run according to policy, and the
process does not crash. This is the whole of issue #2.

**Why this priority**: Equal-first with User Story 1, because the review found six
process-killing panics reachable from a recipe file, and because a crash loses every result
that came before it, not just the bad step. A tool that its own input can crash cannot be
trusted to report anything.

**Independent Test**: Drive the malformed-input corpus in FR-022 through both runtimes and
confirm every case produces a structured `StepResult` and a zero-crash process.

**Acceptance Scenarios**:

1. **Given** `{"action": "wait_for_text", "text": "zzz", "timeout_seconds": 1e300}`, **When**
   executed, **Then** the process does not panic and a `StepResult` is produced.
2. **Given** `{"action": "wait_for_text", "text": "zzz", "timeout_seconds": "abc"}`, **When**
   executed, **Then** the result is `passed: false`,
   `detail: "could not convert string to float: 'abc'"`, and execution continues to the next
   step.
3. **Given** `{"action": "no_such_action"}`, **When** executed, **Then** the result is
   `name: "1:no_such_action"`, `passed: false`,
   `detail: "unknown step action: no_such_action"`.
4. **Given** a step object with no `action` key, **When** executed, **Then** the run does not
   crash. (The Python oracle *does* crash here — see FR-025 and OQ-008.)
5. **Given** `{"action": "wait_for_regex", "pattern": "[bad"}`, **When** executed, **Then**
   the result is `passed: false`,
   `detail: "invalid regex '[bad': unterminated character set at position 0"`, containing no
   newline character.

---

### User Story 3 - Wait for a pattern and see what it captured (Priority: P2)

An author waits for a regular expression rather than a literal, and the pass detail tells them
what matched, including named and positional capture groups, so a failing recipe can be
debugged from the report alone.

**Why this priority**: `wait_for_regex` is the most expressive step and carries by far the
most elaborate diagnostic, but it is used less than `wait_for_text`. It is separable: a
runtime with the other six steps correct is already useful.

**Independent Test**: Drive the pattern/haystack corpus in FR-019 through both runtimes and
diff the detail strings literally.

**Acceptance Scenarios**:

1. **Given** haystack `alice 42` and pattern `(\w+) (\d+)`, **When** matched, **Then**
   `detail` is exactly ``matched '(\\w+) (\\d+)' -> groups=('alice', '42') (full: 'alice 42')``.
2. **Given** haystack `x 42` and pattern `(?P<n>\d+)`, **When** matched, **Then** `detail` is
   exactly ``matched '(?P<n>\\d+)' -> n='42'; groups=('42',) (full: '42')`` — note the trailing
   comma in the one-element tuple.
3. **Given** haystack `b` and pattern `(?P<x>a)?(?P<y>b)`, **When** matched, **Then** `detail`
   is exactly `matched '(?P<x>a)?(?P<y>b)' -> x=None, y='b'; groups=(None, 'b') (full: 'b')`.
4. **Given** haystack `it's here` and pattern `(it's)`, **When** matched, **Then** `detail` is
   exactly `matched "(it's)" -> groups=("it's",) (full: "it's")` — every quote flips to double.
5. **Given** haystack `abc 42` and pattern `\d+`, **When** matched, **Then** `detail` is
   exactly ``matched '\\d+' -> match='42'`` — the no-groups form.
6. **Given** haystack `ab` and pattern `(?<=a)b`, **When** matched, **Then** the step passes.
   Lookbehind is supported.
7. **Given** haystack `aa` and pattern `(a)\1`, **When** matched, **Then** the step passes.
   Backreferences are supported.

---

### Edge Cases

- **Every duration is a `float()` coercion, not a typed number.** `"0.05"`, `true` and `false`
  are all accepted. See FR-004.
- **`wait_for_regex` validates its timeout; nothing else does.** NaN, zero and negative are
  rejected by `wait_for_regex` and silently accepted by `wait_for_text`, `wait_for_idle` and
  `sleep`. See FR-005, FR-006 and OQ-001.
- **A non-positive or NaN timeout is not an error for the wait steps — it is an instant
  timeout.** The wait loop is `while now() < deadline`, immediately false.
- **`wait_for_idle` with `timeout_seconds: Infinity` passes instantly** (`stable for 0.0s`)
  while `timeout_seconds: NaN` fails instantly (`timed out waiting for idle`). Two non-finite
  values, two opposite verdicts, neither chosen.
- **`stable_seconds: -1` passes** with `detail: "stable for -1.0s"`. **`NaN` also passes**,
  with `detail: "stable for nans"` — the literal `s` suffix glued to the string `nan`. So does
  `Infinity`: `stable for infs`. All accidents. See OQ-003.
- **A step's detail can be a raw CPython or libc message.** `sleep` out of range yields
  `timestamp out of range for platform time_t`, which is neither TermProof's wording nor
  stable across platforms. See OQ-002.
- **`press` lowercases with full Unicode case folding**, so `ENTER` works. Rust's
  `to_ascii_lowercase` is a different function. See OQ-006.
- **`ctrl-<unmapped>` sends nothing and passes.** `sendcontrol('1')` returns `(0, b'')` — no
  bytes written, no error. See FR-016 and OQ-005.
- **`sendcontrol` lowercases a second time**, so `ctrl-A` and `ctrl-a` both send `\x01`, and
  `press` could not have prevented it either way.
- **Screen and raw output are searched independently.** A pattern spanning the boundary never
  matches: `FIRST.SECOND` against screen `FIRST` and raw `SECOND` times out. Deliberate, and
  commented as such in the source.
- **The `no output observed from the session` branch needs a live child.** A dead session
  short-circuits `wait_for_idle` to `true`, so the branch is only reachable while the child is
  alive and silent. See OQ-004.

---

## Requirements *(mandatory)*

### The step contract

- **FR-001** `[BEHAVIOURAL]` A step is an object with a required `action` naming a registered
  step implementation, plus implementation-specific keys. Executing a step produces a
  **StepResult** with four fields: `name`, `passed`, `detail`, `screen`.
  **Authority**: `SOURCE` (`models.py:41-56`, `builtin_steps.py:13-27`).

- **FR-002** `[BYTE-EXACT]` A step's `name` is the value of its `name` key if present,
  otherwise the string `<index>:<action>` where `<index>` is the step's 1-based position in
  the `steps` array — e.g. `1:wait_for_text`. For an unregistered action the same rule applies
  using the authored action name: `1:no_such_action`.
  **Authority**: `OBSERVED`, and `SOURCE` (`step.get("name", f"{index}:{self.name}")` in every
  step). It appears verbatim in `result.json`, so it is byte-exact.

- **FR-003** `[BEHAVIOURAL]` `screen` is the session's rendered screen at the moment the step
  finished, captured for passing and failing steps alike, including steps that failed by
  raising.
  **Authority**: `SOURCE` (every `StepResult(...)` construction, plus the runner's handler).

### Duration handling — attention item 1, resolved

- **FR-004** `[BYTE-EXACT]` Every duration-valued key — `timeout_seconds` on `wait_for_text`,
  `wait_for_idle` and `wait_for_regex`, `stable_seconds` on `wait_for_idle`, and `seconds` on
  `sleep` — is read with a Python `float()` coercion. It therefore accepts JSON numbers,
  numeric JSON strings (`"0.05"`), and booleans (`true` → `1.0`, `false` → `0.0`). For a value
  `float()` rejects, the step fails and the conversion's own message becomes `detail`:

  | Input | `detail` |
  |---|---|
  | `"abc"` | `could not convert string to float: 'abc'` |
  | `null` | `float() argument must be a string or a real number, not 'NoneType'` |
  | `[]` | `float() argument must be a string or a real number, not 'list'` |

  `wait_for_regex` is the exception: it catches the conversion and substitutes its own message
  (FR-005).
  **Authority**: `OBSERVED`, all three rows, on all four non-regex duration inputs.
  **Byte-exact under protest** — these are CPython strings. See 001-OQ-001 and OQ-002.

- **FR-005** `[BYTE-EXACT]` `wait_for_regex` — and **only** `wait_for_regex` — validates its
  timeout, failing with:

  | Input | `detail` |
  |---|---|
  | `NaN` | `wait_for_regex timeout_seconds must be finite, got nan` |
  | `Infinity` | `wait_for_regex timeout_seconds must be finite, got inf` |
  | `-Infinity` | `wait_for_regex timeout_seconds must be finite, got -inf` |
  | `0`, `0.0`, `false` | `wait_for_regex timeout_seconds must be > 0, got 0.0` |
  | `-1` | `wait_for_regex timeout_seconds must be > 0, got -1.0` |
  | `"abc"` | `wait_for_regex timeout_seconds must be a number, got 'abc'` |
  | `null` | `wait_for_regex timeout_seconds must be a number, got None` |
  | `[]` | `wait_for_regex timeout_seconds must be a number, got []` |

  The `must be finite` / `must be > 0` forms render the **coerced float**; the
  `must be a number` form renders the **original value** with Python `repr`.
  **Authority**: `OBSERVED`, every row. These strings are TermProof's own, so byte-exactness
  here is unreserved. Note the validation order: pattern is checked before timeout, so
  `{"pattern": 42, "timeout_seconds": "abc"}` reports the pattern error.

- **FR-006** `[BYTE-EXACT]` `wait_for_text` and `wait_for_idle` apply **no** range validation.
  NaN, zero, and negative `timeout_seconds` make the wait loop exit immediately and the step
  reports an ordinary timeout (`timed out waiting for 'zzz'` / `timed out waiting for idle`).
  `Infinity` waits, so with a satisfiable condition it passes immediately. A negative, NaN or
  infinite `stable_seconds` makes the idle check succeed on the first iteration:
  `stable for -1.0s`, `stable for nans`, `stable for infs`.
  **Authority**: `OBSERVED`, every value. This is behaviour the oracle *has*, not behaviour it
  *should* have — see OQ-003. It is nonetheless the contract until superseded.

- **FR-007** `[BEHAVIOURAL]` A large finite duration is legal for every waiting step. The
  oracle accepts `1e19` and `1e300` on `wait_for_text`, `wait_for_idle` and `wait_for_regex`
  and simply waits, reporting e.g. `timed out waiting for regex 'zzz' after 1e+300s`. A Rust
  implementation MUST NOT panic, saturate silently, or reject the value; the correct behaviour
  is to clamp the internal deadline to the far future and keep waiting.
  **Authority**: `OBSERVED`. **This is the requirement issue #2 exists for.**
  `Duration::from_secs_f64` and `Instant::now() + d` both panic above roughly `1e19`, and a
  recipe author cannot see that boundary.

- **FR-008** `[BYTE-EXACT]` `sleep` performs no validation and surfaces the sleep primitive's
  error text:

  | Input | `passed` | `detail` |
  |---|---|---|
  | `NaN` | `false` | `Invalid value NaN (not a number)` |
  | `Infinity`, `-Infinity`, `1e19`, `1e300` | `false` | `timestamp out of range for platform time_t` |
  | `-1` | `false` | `sleep length must be non-negative` |
  | `0`, `0.0`, `false`, `true`, `"0.05"` | `true` | `slept` |

  **Authority**: `OBSERVED` on macOS / CPython 3.12. **Byte-exact under protest and
  platform-dependent** — the `time_t` message is libc-shaped and the value at which it appears
  varies by platform. See OQ-002.

- **FR-009** `[BYTE-EXACT]` Where a duration is rendered into a detail — only in
  `stable for <n>s` and `after <n>s` — it uses Python's `float` `repr`, so integral values
  carry a decimal point and large values use exponent form with a sign and no zero-padding:
  `0.5`, `1.0`, `10.0`, `0.05`, `1e+19`, `1e+300`, `nan`, `inf`, `-inf`.
  **Authority**: `OBSERVED`. The review recorded Rust emitting `10s` where Python emits
  `10.0s`; this is the fix, and it needs Python's float-repr algorithm (shortest round-trip),
  not Rust's `{}`.

### Per-step semantics

- **FR-010** `[BYTE-EXACT]` **`wait_for_text`** — reads `text` (**required**) and
  `timeout_seconds` (default `10`). It polls until the literal `text` appears in either the
  rendered screen or the raw output, or the deadline passes, or the session dies (in which
  case it drains once and re-checks).

  | Outcome | `passed` | `detail` |
  |---|---|---|
  | found | `true` | `found <repr(text)>` |
  | not found | `false` | `timed out waiting for <repr(text)>` |

  The failure detail does **not** include the timeout value.
  **Authority**: `OBSERVED` and `SOURCE` (`builtin_steps.py:13-27`, `session.py:102-116`).

- **FR-011** `[BYTE-EXACT]` **`wait_for_idle`** — reads `stable_seconds` (default `0.5`) and
  `timeout_seconds` (default `10`). It polls until the screen and the raw-output length have
  been unchanged for `stable_seconds`, or the deadline passes, or the session dies — a dead
  session drains once and returns success. Three distinct details:

  | Outcome | `passed` | `detail` |
  |---|---|---|
  | settled, or session dead | `true` | `stable for <stable>s` |
  | timed out **and** raw output is empty | `false` | `no output observed from the session` |
  | timed out with output | `false` | `timed out waiting for idle` |

  The middle branch is selected by the raw output being empty, not by the reason for the
  timeout.
  **Authority**: `OBSERVED` (the middle branch required a live, silent child; a dead one
  short-circuits to `true`) and `SOURCE` (`builtin_steps.py:30-51`).

- **FR-012** `[BYTE-EXACT]` **`send_text`** — reads `text` (**required**, no default) and
  writes it to the child with no terminator. Detail on success is the constant `sent text`.
  A missing `text` fails with `detail: "'text'"`.
  **Authority**: `OBSERVED` and `SOURCE` (`builtin_steps.py:54-65`). The detail does not echo
  the text, so it needs no `repr`.

- **FR-013** `[BYTE-EXACT]` **`send_line`** — reads `text` (default `""`) and writes
  `text + "\r"` — a carriage return, **not** a line feed. Detail on success is the constant
  `sent line`. A missing `text` sends the bare `\r` and passes.
  **Authority**: `OBSERVED` and `SOURCE` (`builtin_steps.py:68-79`, `session.py:96-98`).
  The `\r` is load-bearing: a PTY in canonical mode treats `\n` differently.
  Note the asymmetry with FR-012 — `send_text` requires `text`, `send_line` defaults it. That
  is not a decision anyone recorded, but changing it would break recipes either way.

- **FR-014** `[BYTE-EXACT]` **`press`** — reads `key` (**required**). The key is lowercased
  with full Unicode case folding, then:
  - if it starts with `ctrl-`, the remainder is sent as a control character (FR-016);
  - otherwise it is looked up in this exact, exhaustive table and the mapped bytes are sent:

  | key | bytes |
  |---|---|
  | `enter` | `\r` |
  | `escape` | `\x1b` |
  | `tab` | `\t` |
  | `backspace` | `\x7f` |
  | `up` | `\x1b[A` |
  | `down` | `\x1b[B` |
  | `right` | `\x1b[C` |
  | `left` | `\x1b[D` |

  Detail on success is `pressed <key>` — the key **as authored**, unquoted and not lowercased:
  `press key: "ENTER"` yields `pressed ENTER`.
  There are no function keys, no `home`/`end`, no `pageup`, no `delete`, no `space`.
  **Authority**: `OBSERVED` and `SOURCE` (`session.py:26-36`, `builtin_steps.py:82-93`).

- **FR-015** `[BYTE-EXACT]` An unmapped key name fails the step with `detail` equal to the
  Python `repr` of the **lowercased** key alone: `press key: "f13"` produces the
  five-character detail `'f13'`. This is a bare `str(KeyError)` leaking through.
  **Authority**: `OBSERVED`. See OQ-007 — almost certainly not a designed diagnostic, but it
  is the observed one.

- **FR-016** `[BYTE-EXACT]` `ctrl-<c>` maps `<c>` — after a second lowercasing inside the send
  primitive — to a control byte, and **sends nothing at all while reporting success** for any
  `<c>` not in the table:

  | `<c>` | byte |
  |---|---|
  | `a`–`z` (and `A`–`Z`, via the second lowercasing) | 1–26 |
  | `@`, `` ` `` | 0 |
  | `[`, `{` | 27 |
  | `\`, `\|` | 28 |
  | `]`, `}` | 29 |
  | `^`, `~` | 30 |
  | `_` | 31 |
  | `?` | 127 |
  | anything else — `1`, `-`, space, `é` | *nothing sent; step passes with* `pressed ctrl-<c>` |

  The empty remainder (`key: "ctrl-"`) fails with
  `ord() expected a character, but string of length 0 found`.
  **Authority**: `OBSERVED` against `ptyprocess` 0.7.0, whose source returns `(0, b'')` for an
  unmapped character. **Version-dependent** — see OQ-005; the adversarial review recorded
  `ctrl-1` *failing* under a different resolution.

- **FR-017** `[BYTE-EXACT]` **`sleep`** — reads `seconds` (default `1`), sleeps, then drains
  pending output with a zero timeout. Detail on success is the constant `slept`.
  **Authority**: `OBSERVED` and `SOURCE` (`builtin_steps.py:96-108`).

- **FR-018** `[BYTE-EXACT]` **`wait_for_regex`** — reads `pattern` (**required**) and
  `timeout_seconds` (default `10`). It validates the pattern's type, compiles it, validates the
  timeout, then polls, searching the rendered screen and the raw output **independently** —
  never a concatenation. Details:

  | Outcome | `passed` | `detail` |
  |---|---|---|
  | matched | `true` | see FR-020 |
  | no match before deadline | `false` | `timed out waiting for regex <repr(pattern)> after <timeout>s` |
  | `pattern` absent or not a string | `false` | `wait_for_regex 'pattern' must be a string, got <type name>` |
  | pattern will not compile | `false` | `invalid regex <repr(pattern)>: <compiler message>` |

  An absent `pattern` is indistinguishable from `"pattern": null`: both report
  `got NoneType`.
  **Authority**: `OBSERVED` and `SOURCE` (`builtin_steps.py:111-230`).
  Searching screen and raw separately is a deliberate decision, commented in the source:
  concatenating them "creates synthetic boundaries that never existed in the terminal."

### Regex dialect — attention item 2, resolved

- **FR-019** `[BEHAVIOURAL]` The pattern dialect is **Python 3's `re` module**, not PCRE and
  not the Rust `regex` crate. Concretely, each of the following was executed against the
  oracle and MUST hold:

  | Feature | Example | Required outcome |
  |---|---|---|
  | Lookbehind | `(?<=a)b` on `ab` | matches — **the `regex` crate cannot do this** |
  | Backreference | `(a)\1` on `aa` | matches — **the `regex` crate cannot do this** |
  | Inline case flag | `(?i)ALICE` on `alice` | matches |
  | Inline multiline | `(?m)^world` on `hello\nworld` | matches |
  | Default `^` | `^hello` on `hello\nworld` | matches at string start only |
  | Default `.` | `h.llo` on `h\nllo` | does **not** match — `.` excludes newline |
  | Inline dotall | `(?s)h.llo` on `h\nllo` | matches |
  | Inline comment | `(?#comment)a` on `a` | matches |
  | Named group | `(?P<n>\d+)` — the `?P` spelling | matches |
  | String-start anchor | `\A\w+` on `alice` | matches |
  | String-end anchor | `\Zx` on `x` | does **not** match — `\Z` is true end-of-string, not PCRE's before-final-newline |
  | Non-capturing group | `(?:a\|b)+` on `ab` | matches |
  | Unicode property | `\p{L}` | **fails to compile**: `bad escape \p at position 0` |

  A Rust implementation therefore cannot use the `regex` crate for this step. It needs a
  backtracking engine with Python's syntax, or a documented dialect change.
  **Authority**: `OBSERVED` — every row executed. This is the single largest implementation
  constraint in the spec set.

- **FR-020** `[BYTE-EXACT]` A successful match's detail is built as follows. Let `P` be
  `repr(pattern)`, `M` be `repr(match.group(0))`, `named` the named-group mapping and `groups`
  the positional-group tuple.
  - With no groups of any kind: `matched <P> -> match=<M>`.
  - Otherwise, join with `; ` the parts that exist, in this order, and emit
    `matched <P> -> <joined> (full: <M>)`:
    - if `named` is non-empty: `k1=<repr(v1)>, k2=<repr(v2)>, ...` in group-definition order;
    - if `groups` is non-empty: `groups=<repr(tuple)>`.

  Named groups are shown **in addition to**, not instead of, the positional tuple. Unmatched
  groups render as `None`. The tuple `repr` keeps the trailing comma for one element. Every
  `repr` follows Python's quoting rules, including flipping to double quotes when the value
  contains an apostrophe and no double quote.
  **Authority**: `OBSERVED` — the seven worked examples in User Story 3 — and `SOURCE`
  (`builtin_steps.py:182-193`, `_format_match`).
  This is the most `repr`-dependent string in the product and the reason constitution
  Principle VIII demands one shared formatter.

- **FR-021** `[BYTE-EXACT]` An invalid pattern's `<compiler message>` is CPython's `re.error`
  text, shaped `<reason> at position <n>`:

  | Pattern | `detail` |
  |---|---|
  | `[bad` | `invalid regex '[bad': unterminated character set at position 0` |
  | `a{2,1}` | `invalid regex 'a{2,1}': min repeat greater than max repeat at position 2` |
  | `\p{L}` | ``invalid regex '\\p{L}': bad escape \p at position 0`` |

  **Authority**: `OBSERVED`. **Byte-exact under protest** — CPython's wording; see OQ-002.
  However OQ-002 is resolved, the detail MUST remain a single line: the `regex` crate's
  multi-line ASCII-art parse errors are forbidden outright by constitution Principle VIII.

### Type strictness — attention item 3, resolved

- **FR-022** `[BYTE-EXACT]` Apart from `wait_for_regex`'s `pattern`, the step layer performs
  **no type validation**. A wrong-typed value is handed to the operation it feeds and the
  resulting exception's message becomes `detail`:

  | Step and input | `passed` | `detail` |
  |---|---|---|
  | `wait_for_text`, no `text` | `false` | `'text'` |
  | `wait_for_text` `text: 5` | `false` | `'in <string>' requires string as left operand, not int` |
  | `wait_for_text` `text: null` | `false` | `'in <string>' requires string as left operand, not NoneType` |
  | `send_text`, no `text` | `false` | `'text'` |
  | `send_text` `text: 5` | `false` | `utf_8_encode() argument 1 must be str, not int` |
  | `send_text` `text: null` | `false` | `utf_8_encode() argument 1 must be str, not None` |
  | `send_line` `text: 5` | `false` | `unsupported operand type(s) for +: 'int' and 'str'` |
  | `send_line` `text: null` | `false` | `unsupported operand type(s) for +: 'NoneType' and 'str'` |
  | `press`, no `key` | `false` | `'key'` |
  | `press` `key: 5` | `false` | `'int' object has no attribute 'lower'` |
  | `press` `key: "f13"` | `false` | `'f13'` |
  | `wait_for_regex` `pattern: 42` | `false` | `wait_for_regex 'pattern' must be a string, got int` |

  **Authority**: `OBSERVED`. The three `send_text` rows were measured against a **real
  `pexpect` child**; a stub session that appends to a list records `send_text {text: 5}` as
  *passing*, and both the port's harness and the adversarial review's harness made exactly
  that mistake. See constitution Principle IV.

- **FR-023** `[BEHAVIOURAL]` Keys and their defaults, exhaustively:

  | Step | Required | Optional (default) |
  |---|---|---|
  | `wait_for_text` | `text` | `timeout_seconds` (`10`) |
  | `wait_for_idle` | — | `stable_seconds` (`0.5`), `timeout_seconds` (`10`) |
  | `send_text` | `text` | — |
  | `send_line` | — | `text` (`""`) |
  | `press` | `key` | — |
  | `sleep` | — | `seconds` (`1`) |
  | `wait_for_regex` | `pattern` | `timeout_seconds` (`10`) |

  Every step also accepts `name` (FR-002). Unknown extra keys are ignored silently.
  **Authority**: `OBSERVED` and `SOURCE` (each `step[...]` / `step.get(...)` call).

### Dispatch, failure containment, and ordering

- **FR-024** `[BYTE-EXACT]` An `action` naming no registered step fails that step with
  `detail: "unknown step action: <action>"` — the action name **bare**, not `repr`-quoted,
  unlike the validator's message in 001-FR-020. The two layers disagree on quoting and both
  are contract.
  **Authority**: `OBSERVED` and `SOURCE` (`runner.py`, `_run_step`).

- **FR-025** `[BEHAVIOURAL]` **No step, and no step input, may terminate the run.** Every
  failure — a missing key, a wrong type, an uncompilable pattern, an out-of-range duration —
  produces a `StepResult` with `passed: false` and a `detail`, and execution proceeds
  according to the failure policy.
  **Authority**: Constitution Principle VI; issue #2. **This requirement supersedes the
  oracle** under constitution Principle III: a step object with no `action` key raises an
  unhandled `KeyError: 'action'` in Python — the runner's own handler reads `step["action"]`
  as its first line — and kills the whole run, losing every prior step's result. That is a
  defect, not a contract. A structured failure is correct and the Python side is what needs
  to change. See OQ-008 for the diagnostic text, which is not yet decided.

- **FR-026** `[BEHAVIOURAL]` Steps execute in array order and each result appears in
  `result.json`'s `steps` array in that same order. A failing step neither removes itself nor
  removes later steps from the array.
  **Authority**: `SOURCE` (`runner.py`, the step loop).

- **FR-027** `[BEHAVIOURAL]` The step registry is extensible: a plugin may register an action
  name, and dispatch MUST consult the registry rather than a closed set of built-in names.
  **Authority**: `SOURCE` (`runner.py`, `_build_step_registry` from `VerifierConfig`) and
  `DOC` (`docs/plugin-protocols.md` — out of scope here, but real). The Rust port's closed
  `match` over seven names cannot satisfy this; see OQ-009.

- **FR-028** `[BEHAVIOURAL]` No Rust `Debug` rendering appears in any `detail`. Where a detail
  embeds a value, the shared Python-`repr` formatter produces it.
  **Authority**: Constitution Principle VIII. The review found `UnknownKey("f13")`,
  `Some(Bool(true))` and `BadCtrl("ctrl-")` in `detail` fields.

### Key Entities

- **Step** — an object with `action` plus implementation-specific keys, loaded by spec 001
  without inspection.
- **StepResult** — `name`, `passed`, `detail`, `screen`. Serialised into `result.json`'s
  `steps` array.
- **Session** — the running child and its terminal emulation: the rendered `screen`, the
  accumulated `raw_output`, liveness, and the write operations steps use. Its own contract —
  PTY setup, encoding, drain semantics — is out of scope for this spec set.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A differential harness drives at least 110 step cases through both runtimes
  against a **real child process** — not a stub session — and reports **zero** divergences in
  `passed`, and zero in `detail` compared as literal strings. The review's baseline was 26
  agreements out of 110.
- **SC-002**: The corpus covers every row of every table in FR-004, FR-005, FR-008, FR-014,
  FR-016, FR-019, FR-021, FR-022 and FR-023 — at least one case per row.
- **SC-003**: Every one of the seven actions is exercised by at least one passing and at least
  one failing case. Per constitution Principle I, deleting any single dispatch arm makes at
  least one case fail; verified by deletion.
- **SC-004**: **Zero** process crashes across the whole corpus in either runtime, measured as
  the harness observing a normal exit for every case. A panic, an abort, or any non-result
  exit is a hard failure of this spec.
- **SC-005**: The corpus includes `timeout_seconds` of `1e18`, `1e19` and `1e300` on all three
  waiting steps, and `seconds` of the same on `sleep`, and all pass SC-004.
- **SC-006**: A `repr` conformance suite compares the shared formatter against CPython's
  `repr` over at least 200 strings covering apostrophes, double quotes, both together,
  backslashes, tabs, newlines, `\x00`–`\x1f`, DEL, non-ASCII BMP, astral-plane characters, and
  lone-surrogate-adjacent values — plus tuples of length 0, 1, 2 and 3, and `None`. Zero
  divergences.
- **SC-007**: A float-rendering suite compares the shared formatter against CPython's
  `repr(float)` over at least 100 values including `0.5`, `1.0`, `10.0`, `0.05`, `1e-5`,
  `1e16`, `1e17`, `1e19`, `1e300`, `nan`, `inf`, `-inf`. Zero divergences.
- **SC-008**: A regex dialect suite confirms every row of FR-019, including that `\p{L}` fails
  to compile with the stated message and that `\Zx` does not match `x`.
- **SC-009**: No `detail` produced anywhere in the corpus contains a newline character.

---

## Assumptions

- The oracle environment is CPython 3.12 with `pexpect` 4.9.0 and `ptyprocess` 0.7.0.
  `pyproject.toml` declares floors, not pins (`pexpect>=4.9.0`), so a different resolution can
  change FR-016. Pinning is proposed in OQ-005.
- The probes drove the step classes with a stub session for everything except the cases noted
  as real-child in FR-022 and FR-011. The stub's wait loops were transcribed from
  `session.py`, so any divergence between the stub and the real session would show up as a
  wrong observation. `specs/OBSERVATION-LOG.md` records which rows came from which.
- `screen` fidelity — wrapping, scrollback, escape-sequence coverage — belongs to the terminal
  emulator and is out of scope for this spec set.
- Steps are the only writers to the child. Nothing else injects input during a run.

---

## Out of Scope

Noted as future spec work, not started here: the terminal session and PTY layer itself, the
evidence pipeline, renderers, video backends, reporters, the plugin protocol, and the CLI
surface. The step *failure policy* — whether a failing step stops the run — is decided by the
execution mode and is specified with the modes, not here.

---

## Open Questions

- **OQ-001 — Only one of five duration inputs is validated.**
  `wait_for_regex` rejects NaN, infinity, zero and negative; `wait_for_text`, `wait_for_idle`
  (both keys) and `sleep` accept all of them and produce different, mostly accidental
  behaviour. The asymmetry has no visible rationale: `wait_for_regex` was hardened at some
  point and nothing else was. **Decision needed**: extend FR-005's validation to all five
  inputs — a behaviour change to Python as well as Rust — or keep the asymmetry and document
  it as intentional. Extending it would also resolve most of OQ-002 and all of OQ-003.

- **OQ-002 — CPython and libc error text is currently the contract.**
  FR-004, FR-008, FR-021 and FR-022 make foreign strings byte-exact requirements:
  `timestamp out of range for platform time_t`,
  `could not convert string to float: 'abc'`,
  `'in <string>' requires string as left operand, not int`,
  `utf_8_encode() argument 1 must be str, not int`,
  `unterminated character set at position 0`. None belong to TermProof. The `time_t` message
  is libc-shaped and the value at which it appears varies by platform; `utf_8_encode` is a
  CPython internal function name. A Rust implementation must hardcode a table of other
  projects' strings and keep it current. **Decision needed**: freeze them as an explicit,
  TermProof-owned compatibility table with a Python-side test pinning each one; or declare
  this class of detail behavioural and require only `passed` to match. Same decision as
  001-OQ-001 and 003-OQ-010 — make it once for all three.

- **OQ-003 — `stable for nans`, `stable for infs` and `stable for -1.0s` are accidents.**
  A non-finite or negative `stable_seconds` makes the idle check pass on the first iteration,
  and the detail is the string `nan`/`inf`/`-1.0` with an `s` glued on. Separately,
  `timeout_seconds: Infinity` passes while `timeout_seconds: NaN` fails, for reasons nobody
  chose. **Decision needed**: reject non-finite and non-positive `stable_seconds` and
  `timeout_seconds` (see OQ-001), or freeze `stable for nans` as the contract — which would
  be absurd, but at least explicit.

- **OQ-004 — `wait_for_idle`'s "no output" branch is a heuristic, not a state.**
  The branch is chosen by `raw_output` being empty at the end of the wait, so a session that
  emits one byte then stalls forever reports `timed out waiting for idle`, while a child that
  was never started reports `no output observed from the session` — the same as a live,
  correctly-silent child. It is also only reachable while the child is alive, because a dead
  session short-circuits the whole step to success. **Decision needed**: is "no output" meant
  to diagnose a failed-to-start child? If so it should test liveness, not buffer length.

- **OQ-005 — `ctrl-<unmapped>` silently sends nothing, and that is a dependency's choice.**
  Under `ptyprocess` 0.7.0, `sendcontrol('1')` returns `(0, b'')` and the step passes having
  written no bytes. The adversarial review recorded the same input as *failing* — a different
  `ptyprocess` resolution. FR-016's bottom row is therefore not stable across a
  `pip install`, and neither the port nor its reviewer could be sure which behaviour is the
  oracle's. **Decision needed**: pin `ptyprocess` in the Python package, and separately decide
  whether `press ctrl-1` should fail loudly. Silently doing nothing for a key the author
  explicitly asked for is a bad default at any version.

- **OQ-006 — `press` uses Unicode case folding.**
  `key.lower()` is Unicode-aware: `'ENTERİ'.lower()` is `'enteri̇'`. The Rust port used
  `to_ascii_lowercase`. Every key in FR-014 is ASCII, so full case folding buys nothing and
  can only produce surprising matches — and the send primitive lowercases a *second* time
  inside `ctrl-` handling, so the behaviour is doubly implicit. **Decision needed**: specify
  ASCII lowercasing and change Python, or specify Unicode and pay to match Python's exact
  folding table in Rust.

- **OQ-007 — Missing-key and unknown-key diagnostics are bare `str(KeyError)`.**
  `press` with no `key` yields the detail `'key'`; `press key: "f13"` yields `'f13'`;
  `wait_for_text` with no `text` yields `'text'`. A user reading a report sees a five-character
  quoted string with no verb, no step name, and no explanation — and cannot tell "you omitted
  a key" from "that key name is unsupported", because both render the same way. These are
  leaks, not messages. **Decision needed**: define real diagnostics following the pattern
  `wait_for_regex` already sets (`press requires a 'key'`, `press: unsupported key 'f13'`) and
  change Python accordingly.

- **OQ-008 — A step with no `action` kills the whole run, and the replacement text is
  undecided.**
  FR-025 supersedes the oracle on the *behaviour*. It does not decide the *diagnostic*, and
  the oracle offers no string to copy because it never produces a result at all. **Decision
  needed**: the exact `name` and `detail` for a step with no `action`. Suggested
  `name: "<index>:<missing>"` and `detail: "step requires an 'action'"` — but this is a new
  contract surface, and it needs a matching fix on the Python side so the two agree.

- **OQ-009 — A closed `match` cannot host plugins.**
  FR-027 requires registry dispatch; the Rust port dispatches on a closed `match` over the
  seven built-in names, so a plugin-provided action can never be reached and the failure mode
  is a misleading `unknown step action`. **Decision needed**: strictly this belongs to the
  plugin-protocol spec (out of scope here), but the dispatcher's shape must be settled before
  it is written a second time.

- **OQ-010 — `send_text` requires `text` while `send_line` defaults it.**
  `send_text` with no `text` fails with `'text'`; `send_line` with no `text` sends a bare `\r`
  and passes. Two adjacent steps, opposite conventions, no recorded reason. **Decision
  needed**: pick one. Either is defensible; the current split is not.
