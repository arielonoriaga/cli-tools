# ttk mutate — Pluggable Engines (cargo-mutants) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an engine layer to `ttk mutate` so that, on Rust repos with `cargo-mutants` installed, it transparently delegates to `cargo-mutants` (AST-accurate, fast); otherwise it uses the existing language-agnostic text engine. Both engines produce one identical `Report`.

**Architecture:** New `--engine auto|text|cargo-mutants` flag. A pure `decide()` function picks the engine; `cargo_mutants.rs` shells out to `cargo mutants --output <tempdir>` and parses `outcomes.json` into a `Report`; the existing orchestrator is refactored into `run_text(...) -> Report`. `lib.rs::run` resolves the engine, gets a `Report`, and renders once (stdout + optional markdown).

**Tech Stack:** Rust 2021, clap, serde + serde_json (new), tempfile, ttk-core. Existing crate: `crates/mutate`.

**Spec:** `docs/superpowers/specs/2026-06-04-ttk-mutate-engines-design.md`

**Verified facts (cargo-mutants 27.1.0):**
- `cargo mutants --output <DIR>` writes `<DIR>/mutants.out/outcomes.json`.
- `outcomes.json` top-level keys include exact counts: `caught`, `missed`, `timeout`, `unviable`, `total_mutants`, `success`, and an `outcomes` array.
- Each `outcomes[]` entry: `{ "summary": "...", "scenario": ..., "log_path", "diff_path", "phase_results" }`.
- `summary` ∈ `CaughtMutant | MissedMutant | Unviable | Timeout | Success`.
- `scenario` is the string-ish value `"Baseline"` for the baseline run, or `{ "Mutant": { "name": "<file:line:col: describe>", "file": "<path>", "span": { "start": { "line": N, "column": M }, ... }, "replacement": "...", "genre": "..." } }` for a mutant.
- cargo-mutants exit code is NOT a reliable success signal (non-zero when mutants are missed). The presence of a parseable `outcomes.json` is the success signal; its absence (e.g. baseline build failed) is the error.

**Repo conventions:** errors `Result<_, String>` via `format!`; no code comments; `#[cfg(test)] mod tests` with `tempfile`; logging via `ttk_core::tlog`.

---

## File Structure

```
crates/mutate/
  Cargo.toml        # + serde_json dependency
  src/
    lib.rs          # MutateArgs gains `engine`; run() resolves engine + renders; new run_text()
    engine.rs       # NEW: Engine/Resolved enums, decide() (pure), resolve(), is_rust(), cargo_mutants_available()
    cargo_mutants.rs# NEW: run() shell-out + parse_outcomes() -> Report
    report.rs       # render tweak: survivors with empty `to` print description only
    (rules/lexer/discover/sandbox/runner unchanged)
crates/cli/src/main.rs   # --engine flag + pass-through
```

---

## Task 1: Report rendering for description-only survivors + serde_json dep

cargo-mutants survivors carry a description, not a `from → to` pair. When a
survivor's `to` is empty, render `file:line  <from>` (the description) instead of
`file:line  <from> -> <to>`.

**Files:**
- Modify: `crates/mutate/Cargo.toml`
- Modify: `crates/mutate/src/report.rs`

- [ ] **Step 1: Add serde_json dependency**

In `crates/mutate/Cargo.toml`, under `[dependencies]`, add (keep the existing deps):
```toml
serde_json = "1"
```

- [ ] **Step 2: Write failing tests for empty-`to` rendering**

In `crates/mutate/src/report.rs`, inside `mod tests`, add:
```rust
    fn survivor_with(from: &str, to: &str) -> MutationResult {
        MutationResult {
            site: Site { file: PathBuf::from("src/lib.rs"), line: 7, col: 0, from: from.into(), to: to.into() },
            outcome: Outcome::Survived,
        }
    }

    #[test]
    fn test_stdout_description_only_when_to_empty() {
        let mut r = Report::from_results(&[], 0, true);
        r.survived = 1;
        r.survivors.push(survivor_with("replace == with != in is_even", ""));
        let out = r.render_stdout();
        assert!(out.contains("src/lib.rs:7  replace == with != in is_even"));
        assert!(!out.contains("-> "));
    }

    #[test]
    fn test_stdout_arrow_form_when_to_present() {
        let mut r = Report::from_results(&[], 0, true);
        r.survived = 1;
        r.survivors.push(survivor_with("==", "!="));
        let out = r.render_stdout();
        assert!(out.contains("src/lib.rs:7  == -> !="));
    }
```

- [ ] **Step 3: Run tests, expect the first to FAIL**

Run: `cargo test -p ttk-mutate report`
Expected: `test_stdout_description_only_when_to_empty` FAILS (current code always prints `from -> to`), others pass.

- [ ] **Step 4: Update the renderers**

In `render_stdout`, replace the survivor loop body:
```rust
            for s in &self.survivors {
                out.push_str(&format!(
                    "  {}:{}  {} -> {}\n",
                    s.site.file.display(),
                    s.site.line,
                    s.site.from,
                    s.site.to
                ));
            }
```
with:
```rust
            for s in &self.survivors {
                if s.site.to.is_empty() {
                    out.push_str(&format!(
                        "  {}:{}  {}\n",
                        s.site.file.display(),
                        s.site.line,
                        s.site.from
                    ));
                } else {
                    out.push_str(&format!(
                        "  {}:{}  {} -> {}\n",
                        s.site.file.display(),
                        s.site.line,
                        s.site.from,
                        s.site.to
                    ));
                }
            }
```

In `render_markdown`, replace the survivor table row loop:
```rust
            for s in &self.survivors {
                out.push_str(&format!(
                    "| {} | {} | `{}` | `{}` |\n",
                    s.site.file.display(),
                    s.site.line,
                    s.site.from,
                    s.site.to
                ));
            }
```
with:
```rust
            for s in &self.survivors {
                if s.site.to.is_empty() {
                    out.push_str(&format!(
                        "| {} | {} | {} |  |\n",
                        s.site.file.display(),
                        s.site.line,
                        s.site.from
                    ));
                } else {
                    out.push_str(&format!(
                        "| {} | {} | `{}` | `{}` |\n",
                        s.site.file.display(),
                        s.site.line,
                        s.site.from,
                        s.site.to
                    ));
                }
            }
```

- [ ] **Step 5: Run tests, expect all pass**

Run: `cargo test -p ttk-mutate report`
Expected: all report tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/mutate/Cargo.toml crates/mutate/src/report.rs
git commit -m "feat(mutate): description-only survivor rendering + serde_json dep"
```

---

## Task 2: `engine.rs` — engine selection

**Files:**
- Create: `crates/mutate/src/engine.rs`
- Modify: `crates/mutate/src/lib.rs` (add `pub mod engine;`)

- [ ] **Step 1: Add the module declaration**

In `crates/mutate/src/lib.rs`, add to the top module list (with the other `pub mod` lines):
```rust
pub mod engine;
```

- [ ] **Step 2: Write `engine.rs` (implementation + tests)**

Create `crates/mutate/src/engine.rs`:
```rust
use std::path::Path;
use std::process::Command;
use ttk_core::tlog;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Auto,
    Text,
    CargoMutants,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolved {
    Text,
    CargoMutants,
}

impl Engine {
    pub fn parse(s: &str) -> Result<Engine, String> {
        match s {
            "auto" => Ok(Engine::Auto),
            "text" => Ok(Engine::Text),
            "cargo-mutants" => Ok(Engine::CargoMutants),
            other => Err(format!("unknown --engine '{}', use auto|text|cargo-mutants", other)),
        }
    }
}

pub fn is_rust(project: &Path) -> bool {
    project.join("Cargo.toml").exists()
}

pub fn cargo_mutants_available() -> bool {
    Command::new("cargo")
        .args(["mutants", "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn decide(engine: Engine, rust: bool, available: bool) -> Result<(Resolved, Option<String>), String> {
    match engine {
        Engine::Text => Ok((Resolved::Text, None)),
        Engine::CargoMutants => {
            if available {
                Ok((Resolved::CargoMutants, None))
            } else {
                Err("cargo-mutants engine requested but not installed — `cargo install cargo-mutants` or use --engine text".to_string())
            }
        }
        Engine::Auto => {
            if rust && available {
                Ok((Resolved::CargoMutants, None))
            } else if rust {
                Ok((
                    Resolved::Text,
                    Some("note: cargo-mutants not found; using text engine. `cargo install cargo-mutants` for faster Rust results.".to_string()),
                ))
            } else {
                Ok((Resolved::Text, None))
            }
        }
    }
}

pub fn resolve(engine: Engine, project: &Path) -> Result<Resolved, String> {
    let (resolved, note) = decide(engine, is_rust(project), cargo_mutants_available())?;
    if let Some(n) = note {
        tlog(&n);
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_parse() {
        assert_eq!(Engine::parse("auto").unwrap(), Engine::Auto);
        assert_eq!(Engine::parse("text").unwrap(), Engine::Text);
        assert_eq!(Engine::parse("cargo-mutants").unwrap(), Engine::CargoMutants);
        assert!(Engine::parse("nope").is_err());
    }

    #[test]
    fn test_is_rust() {
        let dir = tempdir().unwrap();
        assert!(!is_rust(dir.path()));
        fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        assert!(is_rust(dir.path()));
    }

    #[test]
    fn test_decide_auto_rust_available_uses_cargo_mutants() {
        let (r, note) = decide(Engine::Auto, true, true).unwrap();
        assert_eq!(r, Resolved::CargoMutants);
        assert!(note.is_none());
    }

    #[test]
    fn test_decide_auto_rust_missing_falls_back_with_note() {
        let (r, note) = decide(Engine::Auto, true, false).unwrap();
        assert_eq!(r, Resolved::Text);
        assert!(note.unwrap().contains("cargo-mutants not found"));
    }

    #[test]
    fn test_decide_auto_non_rust_uses_text() {
        let (r, note) = decide(Engine::Auto, false, false).unwrap();
        assert_eq!(r, Resolved::Text);
        assert!(note.is_none());
    }

    #[test]
    fn test_decide_force_text_always_text() {
        assert_eq!(decide(Engine::Text, true, true).unwrap().0, Resolved::Text);
    }

    #[test]
    fn test_decide_force_cargo_mutants_available() {
        assert_eq!(decide(Engine::CargoMutants, false, true).unwrap().0, Resolved::CargoMutants);
    }

    #[test]
    fn test_decide_force_cargo_mutants_missing_errors() {
        let err = decide(Engine::CargoMutants, true, false).unwrap_err();
        assert!(err.contains("not installed"));
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p ttk-mutate engine`
Expected: 8 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/mutate/src/engine.rs crates/mutate/src/lib.rs
git commit -m "feat(mutate): engine selection (auto/text/cargo-mutants)"
```

---

## Task 3: `cargo_mutants.rs` — delegation + outcomes.json parsing

The parser (`parse_outcomes`) is the unit-testable core. `run` does the IO:
builds the command, runs it with `--output <tempdir>`, then parses
`<tempdir>/mutants.out/outcomes.json`.

**Files:**
- Create: `crates/mutate/src/cargo_mutants.rs`
- Modify: `crates/mutate/src/lib.rs` (add `pub mod cargo_mutants;`)

- [ ] **Step 1: Add the module declaration**

In `crates/mutate/src/lib.rs`, add to the top module list:
```rust
pub mod cargo_mutants;
```

- [ ] **Step 2: Write `cargo_mutants.rs` (implementation + parser tests)**

Create `crates/mutate/src/cargo_mutants.rs`:
```rust
use crate::discover::Site;
use crate::report::Report;
use crate::runner::{MutationResult, Outcome};
use crate::MutateArgs;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use ttk_core::tlog;

#[derive(Deserialize)]
struct OutcomesFile {
    #[serde(default)]
    caught: usize,
    #[serde(default)]
    missed: usize,
    #[serde(default)]
    timeout: usize,
    #[serde(default)]
    unviable: usize,
    #[serde(default)]
    outcomes: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    summary: String,
    scenario: serde_json::Value,
}

pub fn parse_outcomes(json: &str) -> Result<Report, String> {
    let f: OutcomesFile =
        serde_json::from_str(json).map_err(|e| format!("malformed cargo-mutants outcomes.json: {}", e))?;

    let mut survivors = Vec::new();
    for entry in &f.outcomes {
        if entry.summary != "MissedMutant" {
            continue;
        }
        let mutant = &entry.scenario["Mutant"];
        let file = mutant["file"].as_str().unwrap_or("(detail unavailable)").to_string();
        let line = mutant["span"]["start"]["line"].as_u64().unwrap_or(0) as usize;
        let desc = mutant["name"].as_str().unwrap_or("(detail unavailable)").to_string();
        survivors.push(MutationResult {
            site: Site { file: PathBuf::from(file), line, col: 0, from: desc, to: String::new() },
            outcome: Outcome::Survived,
        });
    }

    Ok(Report {
        killed: f.caught,
        survived: f.missed,
        timeout: f.timeout,
        unviable: f.unviable,
        survivors,
        dropped: 0,
        build_used: true,
    })
}

fn write_diff(project: &Path, since: &str, dest: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project)
        .args(["diff", since, "--"])
        .output()
        .map_err(|e| format!("git diff for --since failed: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "git diff {} failed: {}",
            since,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    std::fs::write(dest, &output.stdout)
        .map_err(|e| format!("cannot write diff file {}: {}", dest.display(), e))
}

pub fn run(args: &MutateArgs, project: &Path) -> Result<Report, String> {
    let mut ignored = Vec::new();
    if args.build.is_some() {
        ignored.push("--build");
    }
    if args.config.is_some() {
        ignored.push("--config");
    }
    if args.retest > 1 {
        ignored.push("--retest");
    }
    if args.max_mutants > 0 {
        ignored.push("--max-mutants");
    }
    if !ignored.is_empty() {
        tlog(&format!("note: cargo-mutants engine ignores: {}", ignored.join(", ")));
    }

    let outdir = tempfile::Builder::new()
        .prefix("ttk-cm-")
        .tempdir()
        .map_err(|e| format!("cannot create cargo-mutants output dir: {}", e))?;

    let mut cmd = Command::new("cargo");
    cmd.arg("mutants").current_dir(project);
    cmd.arg("--output").arg(outdir.path());
    cmd.args(["-j", &args.jobs.max(1).to_string()]);
    if let Some(t) = args.timeout {
        cmd.args(["--timeout", &t.to_string()]);
    }
    for p in args.paths {
        if p != Path::new(".") {
            cmd.arg("-f").arg(p);
        }
    }
    if let Some(inc) = args.include {
        cmd.arg("-f").arg(inc);
    }
    if let Some(exc) = args.exclude {
        cmd.arg("-e").arg(exc);
    }
    if args.test.contains("nextest") {
        cmd.args(["--test-tool", "nextest"]);
    }
    if let Some(since) = args.since {
        let diff_path = outdir.path().join("ttk-since.diff");
        write_diff(project, since, &diff_path)?;
        cmd.arg("--in-diff").arg(&diff_path);
    }

    tlog("EXECUTING: delegating to cargo-mutants...");
    cmd.status()
        .map_err(|e| format!("failed to launch cargo-mutants: {}", e))?;

    let outcomes_path = outdir.path().join("mutants.out").join("outcomes.json");
    let json = std::fs::read_to_string(&outcomes_path).map_err(|e| {
        format!(
            "cargo-mutants produced no readable outcomes.json at {} ({}); the baseline build/tests may have failed — see output above",
            outcomes_path.display(),
            e
        )
    })?;
    parse_outcomes(&json)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
        "caught": 1,
        "missed": 1,
        "timeout": 1,
        "unviable": 1,
        "total_mutants": 4,
        "success": 1,
        "outcomes": [
            { "summary": "Success", "scenario": "Baseline", "log_path": "", "diff_path": "", "phase_results": [] },
            { "summary": "CaughtMutant", "scenario": { "Mutant": { "name": "src/lib.rs:1:37: replace add -> i32 with 0", "file": "src/lib.rs", "span": { "start": { "line": 1, "column": 37 } }, "replacement": "0" } }, "log_path": "", "diff_path": "", "phase_results": [] },
            { "summary": "MissedMutant", "scenario": { "Mutant": { "name": "src/lib.rs:2:40: replace == with != in is_even", "file": "src/lib.rs", "span": { "start": { "line": 2, "column": 40 } }, "replacement": "!=" } }, "log_path": "", "diff_path": "", "phase_results": [] },
            { "summary": "Timeout", "scenario": { "Mutant": { "name": "src/lib.rs:3:1: slow", "file": "src/lib.rs", "span": { "start": { "line": 3, "column": 1 } }, "replacement": "loop{}" } }, "log_path": "", "diff_path": "", "phase_results": [] },
            { "summary": "Unviable", "scenario": { "Mutant": { "name": "src/lib.rs:4:1: bad", "file": "src/lib.rs", "span": { "start": { "line": 4, "column": 1 } }, "replacement": "x" } }, "log_path": "", "diff_path": "", "phase_results": [] }
        ]
    }"#;

    #[test]
    fn test_parse_counts() {
        let r = parse_outcomes(FIXTURE).unwrap();
        assert_eq!(r.killed, 1);
        assert_eq!(r.survived, 1);
        assert_eq!(r.timeout, 1);
        assert_eq!(r.unviable, 1);
        assert_eq!(r.score(), Some(0.5));
        assert!(r.build_used);
    }

    #[test]
    fn test_parse_survivor_detail() {
        let r = parse_outcomes(FIXTURE).unwrap();
        assert_eq!(r.survivors.len(), 1);
        let s = &r.survivors[0].site;
        assert_eq!(s.file, PathBuf::from("src/lib.rs"));
        assert_eq!(s.line, 2);
        assert!(s.from.contains("replace == with !="));
        assert_eq!(s.to, "");
    }

    #[test]
    fn test_parse_missing_fields_degrade() {
        let json = r#"{ "caught": 0, "missed": 1, "timeout": 0, "unviable": 0, "outcomes": [ { "summary": "MissedMutant", "scenario": { "Mutant": {} } } ] }"#;
        let r = parse_outcomes(json).unwrap();
        assert_eq!(r.survived, 1);
        assert_eq!(r.survivors[0].site.from, "(detail unavailable)");
        assert_eq!(r.survivors[0].site.line, 0);
    }

    #[test]
    fn test_parse_malformed_errors() {
        assert!(parse_outcomes("{ not json").is_err());
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p ttk-mutate cargo_mutants`
Expected: 4 parser tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/mutate/src/cargo_mutants.rs crates/mutate/src/lib.rs
git commit -m "feat(mutate): cargo-mutants delegation and outcomes.json parser"
```

---

## Task 4: Refactor `lib.rs` — `run_text` + engine dispatch, wire `--engine`

**Files:**
- Modify: `crates/mutate/src/lib.rs`
- Modify: `crates/cli/src/main.rs`

- [ ] **Step 1: Add `engine` to `MutateArgs` and refactor `run`**

In `crates/mutate/src/lib.rs`:

1a. Add an import near the other `use` lines:
```rust
use engine::{Engine, Resolved};
```

1b. Add the `engine` field to `MutateArgs` (place it after `paths` or anywhere in the struct):
```rust
    pub engine: Engine,
```

1c. Replace the ENTIRE existing `pub fn run(args: MutateArgs) -> Result<(), String> { ... }` function with the following two functions. The body of the new `run_text` is the old `run` body with these changes: it takes `args: &MutateArgs` and `project: &Path`, it removes the `let project = std::env::current_dir()...` line (now passed in), and every early/late return yields a `Report` instead of printing. The new `run` resolves the engine and renders.

```rust
pub fn run(args: MutateArgs) -> Result<(), String> {
    let project = std::env::current_dir().map_err(|e| format!("cannot read cwd: {}", e))?;
    let resolved = engine::resolve(args.engine, &project)?;
    let report = match resolved {
        Resolved::Text => run_text(&args, &project)?,
        Resolved::CargoMutants => cargo_mutants::run(&args, &project)?,
    };
    print!("{}", report.render_stdout());
    if let Some(path) = args.report {
        std::fs::write(path, report.render_markdown())
            .map_err(|e| format!("cannot write report {}: {}", path.display(), e))?;
        tlog(&format!("markdown report written to {}", path.display()));
    }
    Ok(())
}

fn run_text(args: &MutateArgs, project: &Path) -> Result<Report, String> {
    let rules = rules::load_rules(args.config)?;

    tlog("EXECUTING: baseline test run...");
    let start = Instant::now();
    let baseline_green = run_shell(args.test, project, BASELINE_TIMEOUT)?;
    if !baseline_green {
        return Err("baseline test command failed — fix tests before mutating".to_string());
    }
    let baseline = start.elapsed();
    let per_mutant_timeout = match args.timeout {
        Some(s) => Duration::from_secs(s),
        None => (baseline * 3).max(Duration::from_secs(5)),
    };

    tlog("EXECUTING: discovering mutation sites...");
    let sites = discover_sites(
        project,
        args.paths,
        &rules,
        args.include,
        args.exclude,
        args.since,
    )?;
    let (sites, dropped) = rank_and_cap(sites, args.max_mutants);
    if dropped > 0 {
        tlog(&format!("dropped {} sites due to --max-mutants cap", dropped));
    }
    if sites.is_empty() {
        tlog("no mutation sites found");
        return Ok(Report::from_results(&[], 0, args.build.is_some()));
    }
    tlog(&format!("{} mutants across {} jobs", sites.len(), args.jobs.max(1)));

    let jobs = args.jobs.max(1);
    let mut buckets: Vec<Vec<discover::Site>> = (0..jobs).map(|_| Vec::new()).collect();
    for (i, site) in sites.into_iter().enumerate() {
        buckets[i % jobs].push(site);
    }

    let cfg = RunConfig {
        test: args.test,
        build: args.build,
        timeout: per_mutant_timeout,
        retest: args.retest.max(1),
    };

    let project_ref = project;
    let cfg_ref = &cfg;
    let results: Vec<MutationResult> = std::thread::scope(|scope| {
        let handles: Vec<_> = buckets
            .into_iter()
            .map(|bucket| {
                scope.spawn(move || {
                    let sandbox = match Sandbox::new(project_ref) {
                        Ok(s) => s,
                        Err(e) => {
                            tlog(&format!("sandbox error (bucket skipped): {}", e));
                            return Vec::new();
                        }
                    };
                    bucket.iter().map(|site| run_site(&sandbox, site, cfg_ref)).collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| {
                h.join().unwrap_or_else(|_| {
                    tlog("error: a worker thread panicked; its mutants are missing from the report");
                    Vec::new()
                })
            })
            .collect()
    });

    Ok(Report::from_results(&results, dropped, args.build.is_some()))
}
```

Note: if any import becomes unused after the refactor (e.g. nothing else references it), remove it to keep clippy clean. `Report` is already imported via `use report::Report;`. `Path` must be in scope — it is (`use std::path::{Path, PathBuf};`).

- [ ] **Step 2: Verify the crate builds and all existing tests pass**

Run: `cargo build -p ttk-mutate` then `cargo test -p ttk-mutate`
Expected: clean build; all prior tests still pass (the text path is behavior-identical). The crate's own tests do not construct `MutateArgs`, so adding the field does not break them. If `cargo clippy -p ttk-mutate` reports an unused import, remove it.

- [ ] **Step 3: Wire `--engine` into the CLI**

In `crates/cli/src/main.rs`, inside the `Mutate { ... }` variant (add near the other options, e.g. after `retest`):
```rust
        /// Mutation engine: auto (delegate to cargo-mutants on Rust), text, or cargo-mutants
        #[arg(long, default_value = "auto")]
        engine: String,
```

In the `Commands::Mutate { ... }` match arm, add `engine,` to the destructured field list, and set the `engine` field when constructing `MutateArgs`. The arm becomes (note the added `engine` in both the pattern and the struct literal):
```rust
        Commands::Mutate {
            paths,
            test,
            build,
            config,
            jobs,
            since,
            include,
            exclude,
            timeout,
            max_mutants,
            retest,
            report,
            engine,
        } => {
            if paths.is_empty() {
                return Err("mutate: provide at least one path to mutate".to_string());
            }
            let jobs = jobs.unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1));
            let engine = ttk_mutate::engine::Engine::parse(&engine)?;
            ttk_mutate::run(ttk_mutate::MutateArgs {
                paths: &paths,
                test: &test,
                build: build.as_deref(),
                config: config.as_deref(),
                jobs,
                since: since.as_deref(),
                include: include.as_deref(),
                exclude: exclude.as_deref(),
                timeout,
                max_mutants,
                retest,
                report: report.as_deref(),
                engine,
            })
        }
```

- [ ] **Step 4: Build the whole workspace and smoke-test help**

Run: `cargo build`
Expected: clean build.

Run: `cargo run -- mutate --help`
Expected: usage now lists `--engine`.

- [ ] **Step 5: Commit**

```bash
git add crates/mutate/src/lib.rs crates/cli/src/main.rs
git commit -m "feat(mutate): wire engine dispatch and --engine flag"
```

---

## Task 5: cargo-mutants e2e (ignored) + docs

**Files:**
- Modify: `crates/mutate/tests/e2e.rs`
- Modify: `README.md`

- [ ] **Step 1: Add a gated e2e test**

Append to `crates/mutate/tests/e2e.rs` (the helper `ttk_bin`, `write`, and the `use` lines already exist at the top of the file — do not duplicate them):
```rust
fn cargo_mutants_installed() -> bool {
    std::process::Command::new("cargo")
        .args(["mutants", "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn test_mutate_cargo_mutants_engine_reports_survivor() {
    let bin = ttk_bin();
    if !bin.exists() {
        panic!("build the workspace first: cargo build");
    }
    if !cargo_mutants_installed() {
        eprintln!("skipping: cargo-mutants not installed");
        return;
    }
    let proj = tempdir().unwrap();
    let root = proj.path();
    write(&root.join("Cargo.toml"), "[package]\nname = \"cmfix\"\nversion = \"0.0.0\"\nedition = \"2021\"\n");
    write(
        &root.join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\npub fn is_even(n: i32) -> bool { n % 2 == 0 }\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn t_add() { assert_eq!(add(2, 3), 5); }\n}\n",
    );

    let report_path = root.join("report.md");
    let output = Command::new(&bin)
        .current_dir(root)
        .args(["mutate", "src", "--test", "cargo test", "--engine", "cargo-mutants", "--jobs", "2", "--report"])
        .arg(&report_path)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "ttk mutate (cargo-mutants) failed: {}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains("survivors"), "expected survivors (is_even is untested):\n{}", stdout);
    let report = fs::read_to_string(&report_path).unwrap();
    assert!(report.contains("Mutation score"));
}
```

- [ ] **Step 2: Build then run the e2e (cargo-mutants is installed in this env)**

Run: `cargo build && cargo test -p ttk-mutate --test e2e`
Expected: all e2e tests pass (the new one exercises real cargo-mutants; `is_even` mutants survive because only `add` is tested). If cargo-mutants were absent it would early-return without failing.

- [ ] **Step 3: Document `--engine` in the README**

In `README.md`, in the `ttk mutate` section, add to the bullet list (after the `--build` bullet):
```markdown
- `--engine auto|text|cargo-mutants` (default `auto`): on a Rust repo with `cargo-mutants` installed, `auto` delegates to it (AST-accurate, much faster than the text engine, no unviable noise); otherwise the text engine runs. `text` forces the agnostic engine; `cargo-mutants` forces delegation (errors if not installed). cargo-mutants mode ignores `--build`/`--config`/`--retest`/`--max-mutants` (logged) and maps `--since` to its `--in-diff`.
```

- [ ] **Step 4: Commit**

```bash
git add crates/mutate/tests/e2e.rs README.md
git commit -m "test(mutate): cargo-mutants e2e + docs for --engine"
```

---

## Self-Review Notes (already applied)

- **Spec coverage:** `--engine` flag + values (T4 CLI) · resolution incl. fallback note + forced-error (T2 `decide`/`resolve`) · text-engine refactor to `run_text` returning Report (T4) · cargo-mutants delegation with flag mapping + ignored-flags note + `--output` tempdir (T3 `run`) · outcomes.json → Report via stable top-level counts + best-effort survivor detail (T3 `parse_outcomes`) · description-only report rendering (T1) · tests incl. decision matrix, parser fixture, gated e2e (T2/T3/T5) · docs (T5). All spec sections and invariants V7–V10 mapped.
- **Type consistency:** `Engine`/`Resolved`, `decide`/`resolve`, `parse_outcomes`/`run`, `Report` field names (`killed/survived/timeout/unviable/survivors/dropped/build_used`), `MutationResult`/`Site`/`Outcome`, `MutateArgs.engine` are consistent across tasks and match the existing crate from the prior plan.
- **Ordering safety:** modules (`engine`, `cargo_mutants`) are built and tested standalone (T2, T3) before being wired into `run` (T4), so the crate compiles after every task.
- **Known intentional limitations (per spec):** in cargo-mutants mode `--build/--config/--retest/--max-mutants` are ignored (logged); survivor detail is best-effort across cargo-mutants versions while counts are exact; only Rust/cargo-mutants is implemented (engine layer allows more later).
```
