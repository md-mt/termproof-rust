# termproof-core

Recipe model, schema, validation, planning, steps, assertions and run
orchestration — the core of
[TermProof](https://github.com/md-mt/termproof-rust).

> **Maturity: this port is in progress and is not at parity with the Python
> implementation.** The Python implementation at
> [`md-mt/termproof`](https://github.com/md-mt/termproof) is the shipped product
> and the behavioural oracle for TermProof; there is no parity gate for this
> port. Read
> [the maturity section of the workspace README](https://github.com/md-mt/termproof-rust#maturity--read-this-before-using-it)
> before depending on this crate.

## What it provides

- `Recipe`, `Step`, `Assertion`, `VerifierConfig` — the recipe model, loaded
  from JSON or YAML, with a Draft 2020-12 schema and structured validation.
- `steps` — the seven built-in step actions and their dispatch.
- `assertions` — the eight built-in assertions.
- `planner` / `runner` / `execution` — recipe × renderer planning and execution
  against a `termproof-terminal` session.
- `store` / `cache` — canonical artifact storage with a path-traversal guard,
  and a content-addressed run cache.
- `pyregex` / `pyrepr` / `pypath` / `pyschema` — the compatibility shims that
  keep this port's behaviour close to the Python oracle's.

## Measured agreement, not parity

The step and assertion layers are measured against corpora recorded from the
Python implementation. On those corpora the two runtimes reach 82/115 full
agreement on steps and 124/147 on assertions. That is a layer-level number, not
a product-level one — screen fidelity and whole-recipe execution are outside
it. `harness/README.md` in the repository is the authority on the counts and
the divergences.

`load_canonical_schema` finds nothing in this repository: the canonical recipe
schema and the example corpus stay with the Python repository on purpose, as
the contract both implementations answer to.

## Package contents

The two differential tests (`tests/differential_steps.rs`,
`tests/differential_assertions.rs`) are excluded from the published package —
they replay a corpus that lives at the repository root and is not shipped.
Run them from a repository checkout.

## Licence

MIT — see [LICENSE](LICENSE).
