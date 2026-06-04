# ttk mutate — language-agnostic mutation testing

Date: 2026-06-03
Status: Approved design

## Goal

Add `ttk mutate`: a language-agnostic mutation testing command. ttk knows nothing
about any language. It treats source as text and the user's test command as the
only source of truth: green command = healthy code. Mutate code, re-run the
command — if it stays green, the tests are blind there (a survivor).

Works for any language with a pass/fail command: Rust, JS, Python, Go, shell, etc.

## Core model

Two inputs:

- **source files** — any glob, any extension
- **one verdict command** — `--test "<cmd>"`, e.g. `cargo test`, `pytest`, `npm test`

Belief: a green verdict command means the code is correct. A mutation that keeps
the command green is an untested behavior change. A mutation that turns it red is
a *killed* mutant (good — tests caught it).

## CLI

```
ttk mutate <paths>... --test "<cmd>" [opts]
```

| flag | meaning | default |
|------|---------|---------|
| `<paths>...` | files/dirs to mutate | required |
| `--test <cmd>` | verdict command; green = pass | required |
| `--config <file>` | rule overlay TOML | `.ttk-mutate.toml` if present |
| `--jobs <N>` | parallel mutants | num cpu cores |
| `--since <gitref>` | only mutate lines changed since ref | none (all lines) |
| `--include <glob>` | restrict files | none |
| `--exclude <glob>` | skip files | none |
| `--timeout <secs>` | per-mutant kill | 3× baseline duration |
| `--report <file>` | write markdown survivors report | none (stdout only) |

## Mutation engine (the agnostic core)

### 1. Universal operator table (built-in)

Symbolic operators near-universal across C-family and most languages. Applied at
token boundaries, not naive substring:

```
==  ↔ !=        <  ↔ <=       >  ↔ >=
+   ↔ -         *  ↔ /        && ↔ ||
++  ↔ --        true ↔ false  0  ↔ 1
return X ↔ return       continue ↔ break
```

Each rule is `from → to`. Bidirectional pairs expand to two rules.

### 2. Config overlay (`.ttk-mutate.toml`)

Data-driven rules extend/override the built-in table per repo. No code change to
support a new language.

```toml
# .ttk-mutate.toml
[[rule]]
find = "and"
replace = "or"

[[rule]]
find = "is not"
replace = "is"
```

Overlay merges with built-in table. A rule whose `find` matches a built-in
`from` overrides it. `disable_builtins = true` at top level drops the built-in
table entirely (purist data-only mode).

### 3. String/comment masking

Minimal lexer heuristic masks string literals and comments before site
discovery, so operators inside `"a==b"` or `// x == y` are not mutated. Heuristic
markers: `"`, `'`, backtick for strings; `//`, `#`, `/* */` for comments. Best
effort — documented as heuristic, not a real parser.

### 4. Self-discovery

Scan target files, count which operators actually appear, mutate only real sites.
No wasted runs on operators a language never uses. Rank sites by operator
frequency for deterministic ordering.

## Data flow

1. **Baseline** — run `--test` once in the real project. Must be green; else abort
   with `error: baseline test command failed — fix tests before mutating`. Record
   wall-clock duration → timeout budget (`3×` unless `--timeout` given).
2. **Discover** — walk `<paths>` (respect `--include`/`--exclude`), apply `--since`
   git diff scope (only changed lines), mask strings/comments, emit
   `Site { file, line, col, from, to }`. Rank by operator frequency.
3. **Run** — pool of `--jobs` workers. Each worker owns a full temp copy of the
   project (`/tmp/ttk-mut-<n>/`, reuse copy-clean logic). For each assigned site:
   apply the single mutation in its copy → run `--test` in that copy (cwd = copy)
   → classify → revert the file in the copy for the next site.
4. **Report** — compute score, print survivors as diff hunks to stdout, optional
   markdown to `--report`.

### Classification

| outcome | meaning |
|---------|---------|
| **killed** | verdict went red — tests caught it |
| **survived** | verdict stayed green — test gap |
| **timeout** | exceeded timeout budget (likely infinite loop) |
| **unviable** | code failed to build/parse — killed but flagged separately |

`mutation score = killed / (total − unviable − timeout)`.

Unviable and timeout reported separately — text mutation produces occasional
unviable mutants (e.g. a swap that won't compile). We measure the noise instead of
pretending it is zero.

## Crate structure

New crate `crates/mutate/` (`ttk-mutate`), wired into `crates/cli/src/main.rs` as
a `Mutate` subcommand following the existing `run(Args)` pattern.

| file | responsibility |
|------|----------------|
| `lib.rs` | `run(MutateArgs)` orchestrator; baseline → discover → run → report |
| `rules.rs` | universal table + TOML overlay loader → `Vec<Rule>` |
| `lexer.rs` | string/comment masker (quote + comment-marker heuristic) |
| `discover.rs` | walk files, apply `--since` git scope, emit `Site`s |
| `runner.rs` | temp project copies, apply mutation, exec verdict, classify |
| `report.rs` | score math + stdout diff hunks + markdown |

### Reuse from `ttk-core`

- `git.rs` — `--since` diff scope (changed line ranges)
- `markdown.rs` — markdown report generation
- `output` / `tlog` — timestamped logging
- `fs_utils` — project copy (shared with copy-clean)

### Dependencies

- `walkdir` — file walking (already used elsewhere)
- `toml` — config overlay parsing
- `rayon` or std threads — `--jobs` worker pool
- `glob` — `--include`/`--exclude`
- `tempfile` (dev + runtime) — temp project copies

## Error handling

- Baseline red → abort, clear message, exit non-zero.
- Missing `--test` → clap required-arg error.
- Bad `--config` TOML → abort with parse error + file path.
- Temp copy failure → abort that worker, log, continue others; report partial.
- Verdict command not found / non-exec → treat as build failure context, abort
  baseline.

## Testing

Unit:
- `lexer` — string/comment masking: `"a==b"` and `// ==` not mutated; real `==` is.
- `rules` — overlay merge, override, `disable_builtins`.
- `discover` — site extraction, `--since` scoping, include/exclude globs.
- `report` — score math incl. unviable/timeout exclusion.

Integration:
- Tiny fixture project (any language with a cheap test cmd) with one known test
  gap → assert exactly one survivor at the expected site and correct score.
- Fixture where all mutants killed → score 1.0, zero survivors.

## Tradeoffs (honest)

- Text mutation, not AST → occasional unviable mutant and rare false site. Kept on
  purpose: AST mutation needs a parser per language and kills agnosticism. We
  measure and report the noise (separate unviable/timeout counts) rather than hide
  it.
- Whole-project temp copy per worker → more disk + copy time, but true parallel
  isolation and no git-worktree dependency.

## Out of scope (YAGNI)

- Per-language AST mutation.
- Incremental/resumable verdict cache (mentioned as future dream; not v1).
- Equivalent-mutant detection beyond unviable flagging.
