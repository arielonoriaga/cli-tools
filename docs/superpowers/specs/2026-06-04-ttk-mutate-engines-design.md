# ttk mutate — pluggable engines (cargo-mutants delegation)

Date: 2026-06-04
Status: Approved design
Extends: `docs/superpowers/specs/2026-06-03-ttk-mutate-design.md`

## Problem

The text engine cold-builds the project in a temp sandbox once per mutant. For
compiled languages (Rust especially) that is brutally slow and produces noisy
unviable mutants. `cargo-mutants` is AST-accurate, only emits viable mutants,
builds incrementally, and is the de-facto Rust mutation tester.

## Goal

Keep ttk's language-agnostic text engine as the universal fallback, but
transparently delegate to a best-in-class per-language tool when one is available.
v1 adds exactly one delegated engine: `cargo-mutants` for Rust. The design leaves
room for more engines (mutmut, Stryker) later without rework.

Both engines produce the same `Report`; output and scoring are identical
regardless of engine.

## CLI addition

```
ttk mutate <paths>... --test "<cmd>" [--engine auto|text|cargo-mutants] [existing flags]
```

| value | behavior |
|-------|----------|
| `auto` (default) | detect; delegate to cargo-mutants on Rust if installed, else text |
| `text` | force the agnostic text engine (escape hatch for mixed repos) |
| `cargo-mutants` | force delegation; error if not installed |

## Engine resolution (`engine.rs`)

```
enum Engine { Auto, Text, CargoMutants }
enum Resolved { Text, CargoMutants }
fn resolve(engine: Engine, project: &Path) -> Result<Resolved, String>
```

- **Auto**: `is_rust(project)` (a `Cargo.toml` in the project root) AND
  `cargo_mutants_available()` (`cargo mutants --version` exits 0) → `CargoMutants`.
  - Rust but binary missing → `Text` + one-line note:
    `note: cargo-mutants not found; using text engine. \`cargo install cargo-mutants\` for faster Rust results.`
  - Not Rust → `Text` (no note).
- **Text**: always `Resolved::Text`.
- **CargoMutants**: if `cargo_mutants_available()` → `CargoMutants`; else
  `Err("cargo-mutants engine requested but not installed — `cargo install cargo-mutants` or use --engine text")`.

`resolve` performs no mutation; it only decides and logs. Pure enough to unit-test
the decision matrix (binary-availability is injected as a bool in tests).

## Architecture refactor

Both engines return a `Report`; `lib.rs` renders once.

```
lib.rs            run(MutateArgs): resolve engine → Report → render (stdout + optional markdown)
engine.rs         Engine/Resolved + resolve() + is_rust() + cargo_mutants_available()
text engine       run_text(args, project, rules) -> Result<Report, String>   (existing orchestrator body, refactored to return Report instead of printing)
cargo_mutants.rs  run_cargo_mutants(args, project) -> Result<Report, String>
```

Refactor scope: the current `lib.rs::run` body (baseline → discover → cap →
parallel run → aggregate) moves verbatim into `run_text`, returning the `Report`
instead of printing it. `lib.rs::run` becomes: resolve cwd, branch on engine,
obtain `Report`, print `render_stdout()`, write `render_markdown()` if `--report`.
The baseline-green check, sandboxing, and parallelism stay entirely inside
`run_text`.

## cargo-mutants delegation (`cargo_mutants.rs`)

Run `cargo mutants` from the project dir with `--output <tempdir>` so the
`mutants.out/` directory never lands in the user's repo. After it exits, parse
`<tempdir>/mutants.out/outcomes.json` into a `Report`.

### Flag mapping

| ttk flag | cargo-mutants flag | notes |
|----------|--------------------|-------|
| `--jobs N` | `-j N` | |
| `--timeout S` | `--timeout S` | per-mutant |
| each positional path / `--include <glob>` | `-f <glob>` | examine filter |
| `--exclude <glob>` | `-e <glob>` | |
| `--since <ref>` | `--in-diff <file>` | generate `git diff <ref>` to a temp file first |
| `--test` contains `nextest` | `--test-tool nextest` | otherwise cargo-mutants uses default `cargo test` |

### Ignored flags (logged, not silent)

In cargo-mutants mode these text-engine-only flags are ignored with a single note
listing them: `--build`, `--config`, `--retest`, `--max-mutants`. cargo-mutants
runs its own build/check + baseline, so unviable detection is real here with no
`--build` (the no-`--build` "counted as killed" caveat does NOT apply in this
mode; `build_used` is reported as true).

### outcomes.json → Report

cargo-mutants writes `outcomes.json` with an `outcomes` array. Each entry has a
`summary` string and a mutant `scenario`. Map summary → outcome:

| cargo-mutants summary | ttk outcome |
|-----------------------|-------------|
| `CaughtMutant` | killed |
| `MissedMutant` | survived |
| `Unviable` | unviable |
| `Timeout` | timeout |
| `Success` (the baseline scenario) | skipped (not a mutant) |

For survivors, extract file + line + a human description from the scenario
(cargo-mutants provides the mutated file path and a change description). Build a
`Report` with the same counts/score logic as the text engine.

Parsing is best-effort on detail, exact on counts: the `summary` field drives the
counts and is stable; survivor file/line/description are read from documented
fields and degrade to "(detail unavailable)" if a future cargo-mutants version
renames them. Counts and score never silently break — if `outcomes.json` is
missing or unparseable, return
`Err("could not read cargo-mutants outcomes.json at <path>: <e>")`.

## Report compatibility

cargo-mutants survivors have no `from → to` token pair, only a description. Extend
the survivor rendering: when a survivor's `to` is empty, print
`file:line  <from>` (where `from` holds the description) instead of
`file:line  <from> -> <to>`. One report format serves both engines. Markdown
table: when `to` is empty, render the description in the `from` column and leave
`to` blank.

## Error handling

- `--engine cargo-mutants` without the binary → hard error (above).
- `cargo mutants` exits non-zero for a real reason (e.g. baseline build fails):
  surface its stderr tail in the returned `Err`. (A baseline failure is the
  cargo-mutants analog of the text engine's red-baseline abort.)
- `--in-diff` temp file write failure → `Err`.
- outcomes.json missing/unparseable → `Err` (above).

## Testing

Unit:
- `engine::resolve` decision matrix (availability injected): Rust+avail→CargoMutants;
  Rust+missing→Text (+note); non-Rust→Text; force-text→Text; force-cargo-mutants+avail→CargoMutants;
  force-cargo-mutants+missing→Err.
- `is_rust`: Cargo.toml present/absent in a tempdir.
- outcomes.json parser: a checked-in fixture JSON (caught + missed + unviable +
  timeout + baseline-success) → asserts Report counts, score, and survivor list.

Integration:
- e2e against a real `cargo mutants` run on a tiny throwaway crate with a known
  test gap. Gated: `#[ignore]` (or skip with a logged message) when
  `cargo mutants --version` is unavailable, so CI without the binary stays green.

Regression:
- All 39 existing tests stay green. `--engine text` and non-Rust repos hit the
  unchanged text path (now via `run_text`).

## Invariants

- **V7: one report, two engines** — both engines return a `Report`; rendering and
  score math live in one place. An engine never prints directly.
- **V8: no silent flag loss** — flags ignored by the active engine are logged once,
  never dropped silently.
- **V9: forced engine is honored or errors** — `--engine cargo-mutants` never
  falls back to text; it errors if it cannot run.
- **V10: counts exact, detail best-effort** — cargo-mutants count/score parsing is
  pinned to the stable `summary` field; only survivor detail may degrade across
  versions, and missing/unparseable output errors rather than reporting wrong
  numbers.

## Out of scope (YAGNI)

- Engines beyond cargo-mutants (mutmut, Stryker) — the layer is built to allow
  them, but none are implemented in v1.
- Mapping text-engine-only flags onto cargo-mutants equivalents beyond the table
  above.
- Merging results from two engines in one run.
