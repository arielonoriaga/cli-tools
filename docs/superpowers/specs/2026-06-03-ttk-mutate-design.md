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
| `--build <cmd>` | optional compile/build step run before `--test`; non-zero = unviable mutant (not killed) | none |
| `--config <file>` | rule overlay TOML | `.ttk-mutate.toml` if present |
| `--jobs <N>` | parallel mutants | num cpu cores |
| `--since <gitref>` | only mutate lines changed since ref | none (all lines) |
| `--include <glob>` | restrict files | none |
| `--exclude <glob>` | skip files | none |
| `--timeout <secs>` | per-mutant kill | 3× baseline duration |
| `--max-mutants <N>` | cap total mutants; rank by freq, drop rest (logged) | 0 = unlimited (warn >1000) |
| `--retest <n>` | re-run a flipped survivor/kill n times to defend against flaky verdicts | 1 (no retest) |
| `--report <file>` | write markdown survivors report | none (stdout only) |

## Mutation engine (the agnostic core)

### 1. Universal operator table (built-in)

Symbolic/keyword operators near-universal across C-family and most languages.
**v1 is fixed-string → fixed-string only** (no captures, no regex). Applied at
token boundaries, not naive substring:

```
==  ↔ !=        <  ↔ <=       >  ↔ >=
+   ↔ -         *  ↔ /        && ↔ ||
++  ↔ --        true ↔ false  0  ↔ 1
continue ↔ break
```

Each rule is `from(str) → to(str)`. Bidirectional pairs expand to two rules.

**Cut from v1 (deferred to v2):** structural mutations like `return X → return`
need a capture/pattern engine (parsing), which breaks the pure token-swap model
and raises false-site rate. Out of scope for v1.

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

1. **Baseline** — run `--test` once in the real project, under a baseline timeout
   (default 5 min, configurable later). Must be green; else abort with
   `error: baseline test command failed — fix tests before mutating`. Record
   wall-clock duration → per-mutant timeout budget (`3×` unless `--timeout` given).
2. **Discover** — walk `<paths>` (respect `--include`/`--exclude`), apply `--since`
   git diff scope (only changed lines), mask strings/comments, emit
   `Site { file, line, col, from, to }`. Rank by operator frequency. Apply
   `--max-mutants` cap after ranking; **log dropped count** (no silent truncation).
3. **Run** — pool of `--jobs` workers. Each worker owns a temp **sandbox** copy of
   the project (`/tmp/ttk-mut-<n>/`): source files copied, heavy dirs
   (`node_modules`, `target`, `.venv`, `vendor`, `dist`, `build`) **symlinked**
   so the verdict command still finds installed deps, `.git` and VCS noise
   skipped. Sandbox lifetime is RAII (`tempfile::TempDir`) — auto-removed on drop,
   even on panic. For each assigned site: apply the single mutation in its copy →
   run `--test` in that copy (cwd = copy, under per-mutant timeout) → classify →
   revert the file for the next site. A result that flips vs baseline is re-run
   `--retest` times before being trusted (flaky-verdict defense).
4. **Report** — workers return results; the orchestrator **collects after join**
   and prints (no interleaved streaming from parallel workers). Compute score,
   print survivors as diff hunks + prominent unviable/timeout counts to stdout,
   optional markdown to `--report`.

### Classification

| outcome | meaning |
|---------|---------|
| **killed** | verdict went red — tests caught it |
| **survived** | verdict stayed green — test gap |
| **timeout** | exceeded timeout budget (likely infinite loop) |
| **unviable** | `--build` step failed — mutant doesn't compile; flagged separately |

`mutation score = killed / (total − unviable − timeout)`.

**Unviable detection requires `--build`.** A language-agnostic tool cannot tell a
compile error from a test failure — both are a non-zero verdict. So unviable is
only separated when the user supplies a `--build` command (run before `--test` in
the sandbox; non-zero build = unviable). Without `--build`, unviable count is 0
and compile-broken mutants count as killed — the report states this caveat
explicitly. Timeout is always detectable (wall-clock).

## Crate structure

New crate `crates/mutate/` (`ttk-mutate`), wired into `crates/cli/src/main.rs` as
a `Mutate` subcommand following the existing `run(Args)` pattern.

| file | responsibility |
|------|----------------|
| `lib.rs` | `run(MutateArgs)` orchestrator; baseline → discover → run → report |
| `rules.rs` | universal table + TOML overlay loader → `Vec<Rule>` |
| `lexer.rs` | string/comment masker (quote + comment-marker heuristic) |
| `discover.rs` | walk files, apply `--since` git scope, cap, emit `Site`s |
| `sandbox.rs` | temp copy lifecycle: copy source + symlink heavy dirs, RAII drop-cleanup. Interface: `Sandbox::new(project)→path`, `Drop→remove`. |
| `runner.rs` | apply mutation in a sandbox, exec verdict (timeout, retest), classify, revert |
| `report.rs` | score math + ordered stdout diff hunks + markdown |

`sandbox.rs` and `runner.rs` are split deliberately (deep modules): sandbox owns
the filesystem/RAII concern, runner owns the mutate-exec-classify concern.

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

## Invariants (review-derived)

- **V1: verdict purity assumption** — score is only as trustworthy as the verdict
  command's determinism. A flaky test = phantom kill/survive. Defense: `--retest`
  re-runs flipped results; assumption documented loudly in `--help` and report.
- **V2: unviable never counts as killed silently** — unviable/timeout excluded from
  the score denominator AND printed as prominent separate counts.
- **V3: no silent truncation** — `--max-mutants` drops are logged with the dropped
  count, never hidden.
- **V4: sandbox cleanup is RAII** — temp copies removed on `Drop`, surviving panic.
- **V5: config schema frozen at v1** — `.ttk-mutate.toml` shape (`[[rule]]
  find`/`replace`, top-level `disable_builtins`) is a one-way door; additive only
  after v1.
- **V6: parallel output is collected, not streamed** — survivor diffs printed after
  join to avoid interleaved garble.

## Tradeoffs (honest)

- Text mutation, not AST → occasional unviable mutant and rare false site. Kept on
  purpose: AST mutation needs a parser per language and kills agnosticism. We
  measure and report the noise (separate unviable/timeout counts) rather than hide
  it.
- Temp sandbox per worker (source copied, heavy dirs symlinked) → small disk
  footprint, deps still resolvable, true parallel isolation, no git-worktree
  dependency. Symlinked dep dirs are treated read-only; mutations only touch
  copied source.
- `0↔1` / `+↔-` token swaps hit indices, versions, format strings → higher
  unviable rate. Accepted; bounded by `--max-mutants` and reported honestly.

## Out of scope (YAGNI)

- Per-language AST mutation.
- Structural/capture mutations (`return X → return`) — needs pattern engine; v2.
- Incremental/resumable verdict cache (mentioned as future dream; not v1).
- Equivalent-mutant detection beyond unviable flagging.
