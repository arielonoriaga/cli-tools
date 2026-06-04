# ttk mutate — Language-Agnostic Mutation Testing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `ttk mutate` command that runs language-agnostic mutation testing: mutate source text with a universal/configurable rule table, run the user's verdict command in a sandbox, and report which mutants survived (test gaps).

**Architecture:** New workspace crate `ttk-mutate` exposing `run(MutateArgs) -> Result<(), String>`, wired into the CLI like every other command. Six focused modules: `rules` (mutation table + TOML overlay), `lexer` (string/comment masker), `discover` (site extraction + git/glob scoping + cap), `sandbox` (RAII temp project copy with symlinked dep dirs), `runner` (apply mutation, exec verdict with timeout/retest, classify), `report` (score + stdout + markdown). Parallelism via `std::thread::scope`, one sandbox per worker thread.

**Tech Stack:** Rust 2021, clap (CLI, already used), `walkdir`, `toml` + `serde` (config), `glob` (include/exclude), `wait-timeout` (verdict timeout), `tempfile` (RAII sandbox), `ttk-core` (reuse `copy_clean_dir`, `tlog`).

**Spec:** `docs/superpowers/specs/2026-06-03-ttk-mutate-design.md`

**Conventions in this repo (follow exactly):**
- Errors are `Result<(), String>` / `Result<T, String>`; build error strings with `format!`.
- No comments / docstrings on code.
- Tests live in a `#[cfg(test)] mod tests` block at the bottom of each module, using `tempfile::tempdir`.
- Logging via `ttk_core::tlog(&format!(...))`.
- Run tests for one crate with `cargo test -p ttk-mutate`.

---

## File Structure

```
crates/mutate/
  Cargo.toml
  src/
    lib.rs        # run(MutateArgs); orchestrates baseline → discover → run → report
    rules.rs      # Rule, builtin_rules(), load_rules(config)
    lexer.rs      # mask_line(line) -> String (string/comment masking, length-preserving)
    discover.rs   # Site, discover_sites(...), --since git scope, include/exclude, max-mutants cap
    sandbox.rs    # Sandbox (RAII temp copy + symlinked dep dirs)
    runner.rs     # Outcome, MutationResult, run_site(...), verdict exec w/ timeout+retest+build gate
    report.rs     # Report, build_report(...), print + markdown
```
CLI wiring lives in `crates/cli/src/main.rs` (new `Mutate` subcommand) and `crates/cli/Cargo.toml` (dep).

---

## Task 1: Crate scaffold + workspace registration

**Files:**
- Create: `crates/mutate/Cargo.toml`
- Create: `crates/mutate/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

- [ ] **Step 1: Create the crate manifest**

Create `crates/mutate/Cargo.toml`:

```toml
[package]
name = "ttk-mutate"
version = "0.1.0"
edition = "2021"

[dependencies]
walkdir = "2"
toml = "0.8"
serde = { version = "1", features = ["derive"] }
glob = "0.3"
wait-timeout = "0.2"
tempfile = "3"
ttk-core = { path = "../core" }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Create a placeholder lib so the crate builds**

Create `crates/mutate/src/lib.rs`:

```rust
pub mod discover;
pub mod lexer;
pub mod report;
pub mod rules;
pub mod runner;
pub mod sandbox;
```

Also create empty module files so it compiles:

`crates/mutate/src/rules.rs`:
```rust
```
`crates/mutate/src/lexer.rs`:
```rust
```
`crates/mutate/src/discover.rs`:
```rust
```
`crates/mutate/src/sandbox.rs`:
```rust
```
`crates/mutate/src/runner.rs`:
```rust
```
`crates/mutate/src/report.rs`:
```rust
```

- [ ] **Step 3: Register the crate in the workspace**

In `Cargo.toml`, add `"crates/mutate",` to the `members` list (after `"crates/mp4-optimize",`).

- [ ] **Step 4: Verify it builds**

Run: `cargo build -p ttk-mutate`
Expected: compiles clean (warnings about empty modules are fine).

- [ ] **Step 5: Commit**

```bash
git add crates/mutate Cargo.toml
git commit -m "chore(mutate): scaffold ttk-mutate crate"
```

---

## Task 2: `rules.rs` — mutation table + TOML overlay

**Files:**
- Modify: `crates/mutate/src/rules.rs`

- [ ] **Step 1: Write the failing tests**

Put this in `crates/mutate/src/rules.rs`:

```rust
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub find: String,
    pub replace: String,
}

#[derive(Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    disable_builtins: bool,
    #[serde(default)]
    rule: Vec<RuleEntry>,
}

#[derive(Deserialize)]
struct RuleEntry {
    find: String,
    replace: String,
}

pub fn builtin_rules() -> Vec<Rule> {
    let pairs = [
        ("==", "!="),
        ("<", "<="),
        (">", ">="),
        ("+", "-"),
        ("*", "/"),
        ("&&", "||"),
        ("++", "--"),
        ("true", "false"),
        ("0", "1"),
        ("continue", "break"),
    ];
    let mut rules = Vec::new();
    for (a, b) in pairs {
        rules.push(Rule { find: a.to_string(), replace: b.to_string() });
        rules.push(Rule { find: b.to_string(), replace: a.to_string() });
    }
    rules
}

pub fn load_rules(config: Option<&Path>) -> Result<Vec<Rule>, String> {
    let cf = match config {
        Some(p) => {
            let text = fs::read_to_string(p)
                .map_err(|e| format!("cannot read config {}: {}", p.display(), e))?;
            toml::from_str::<ConfigFile>(&text)
                .map_err(|e| format!("invalid config {}: {}", p.display(), e))?
        }
        None => ConfigFile::default(),
    };
    let mut rules: Vec<Rule> = if cf.disable_builtins {
        Vec::new()
    } else {
        builtin_rules()
    };
    for entry in cf.rule {
        if let Some(existing) = rules.iter_mut().find(|r| r.find == entry.find) {
            existing.replace = entry.replace;
        } else {
            rules.push(Rule { find: entry.find, replace: entry.replace });
        }
    }
    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_builtins_are_bidirectional() {
        let rules = builtin_rules();
        assert!(rules.iter().any(|r| r.find == "==" && r.replace == "!="));
        assert!(rules.iter().any(|r| r.find == "!=" && r.replace == "=="));
    }

    #[test]
    fn test_load_rules_none_returns_builtins() {
        let rules = load_rules(None).unwrap();
        assert_eq!(rules, builtin_rules());
    }

    #[test]
    fn test_overlay_appends_new_rule() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".ttk-mutate.toml");
        let mut f = std::fs::File::create(&cfg).unwrap();
        write!(f, "[[rule]]\nfind = \"and\"\nreplace = \"or\"\n").unwrap();
        let rules = load_rules(Some(&cfg)).unwrap();
        assert!(rules.iter().any(|r| r.find == "and" && r.replace == "or"));
        assert!(rules.iter().any(|r| r.find == "=="));
    }

    #[test]
    fn test_overlay_overrides_builtin() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".ttk-mutate.toml");
        let mut f = std::fs::File::create(&cfg).unwrap();
        write!(f, "[[rule]]\nfind = \"==\"\nreplace = \"<=\"\n").unwrap();
        let rules = load_rules(Some(&cfg)).unwrap();
        let eq: Vec<_> = rules.iter().filter(|r| r.find == "==").collect();
        assert_eq!(eq.len(), 1);
        assert_eq!(eq[0].replace, "<=");
    }

    #[test]
    fn test_disable_builtins() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".ttk-mutate.toml");
        let mut f = std::fs::File::create(&cfg).unwrap();
        write!(f, "disable_builtins = true\n[[rule]]\nfind = \"a\"\nreplace = \"b\"\n").unwrap();
        let rules = load_rules(Some(&cfg)).unwrap();
        assert_eq!(rules, vec![Rule { find: "a".into(), replace: "b".into() }]);
    }

    #[test]
    fn test_bad_toml_errors() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("bad.toml");
        std::fs::write(&cfg, "this is not = = toml").unwrap();
        let err = load_rules(Some(&cfg)).unwrap_err();
        assert!(err.contains("invalid config"));
    }
}
```

- [ ] **Step 2: Run the tests, expect they pass (impl is written with the tests)**

Run: `cargo test -p ttk-mutate rules`
Expected: 6 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/mutate/src/rules.rs
git commit -m "feat(mutate): mutation rule table with TOML overlay"
```

---

## Task 3: `lexer.rs` — length-preserving string/comment masker

The masker replaces string-literal and comment regions with ASCII spaces so the
site scanner never matches operators inside them. It preserves byte length and
UTF-8 validity (only ASCII bytes are overwritten; multibyte bytes inside strings
are left untouched — they never match ASCII operator tokens anyway).

**Files:**
- Modify: `crates/mutate/src/lexer.rs`

- [ ] **Step 1: Write the failing tests + implementation**

Put this in `crates/mutate/src/lexer.rs`:

```rust
pub fn mask_line(line: &str) -> String {
    let b = line.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    let mask = |out: &mut Vec<u8>, j: usize| {
        if out[j].is_ascii() {
            out[j] = b' ';
        }
    };
    while i < b.len() {
        let c = b[i];
        if c == b'"' || c == b'\'' || c == b'`' {
            let quote = c;
            mask(&mut out, i);
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' && i + 1 < b.len() {
                    mask(&mut out, i);
                    mask(&mut out, i + 1);
                    i += 2;
                    continue;
                }
                let end = b[i] == quote;
                mask(&mut out, i);
                i += 1;
                if end {
                    break;
                }
            }
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            for j in i..b.len() {
                mask(&mut out, j);
            }
            break;
        } else if c == b'#' {
            for j in i..b.len() {
                mask(&mut out, j);
            }
            break;
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            mask(&mut out, i);
            mask(&mut out, i + 1);
            i += 2;
            while i < b.len() {
                if b[i] == b'*' && i + 1 < b.len() && b[i + 1] == b'/' {
                    mask(&mut out, i);
                    mask(&mut out, i + 1);
                    i += 2;
                    break;
                }
                mask(&mut out, i);
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_untouched() {
        assert_eq!(mask_line("if a == b {"), "if a == b {");
    }

    #[test]
    fn test_double_quote_string_masked() {
        let m = mask_line(r#"let s = "a == b";"#);
        assert!(!m.contains("=="));
        assert_eq!(m.len(), r#"let s = "a == b";"#.len());
        assert!(m.starts_with("let s = "));
    }

    #[test]
    fn test_line_comment_masked() {
        let m = mask_line("x = 1 // y == z");
        assert!(m.starts_with("x = 1 "));
        assert!(!m.contains("=="));
    }

    #[test]
    fn test_hash_comment_masked() {
        let m = mask_line("x = 1  # y == z");
        assert!(!m.contains("=="));
    }

    #[test]
    fn test_block_comment_masked() {
        let m = mask_line("a /* == */ == b");
        assert_eq!(m.matches("==").count(), 1);
        assert_eq!(m.len(), "a /* == */ == b".len());
    }

    #[test]
    fn test_escaped_quote_inside_string() {
        let m = mask_line(r#""he said \"==\" ok" == x"#);
        assert_eq!(m.matches("==").count(), 1);
    }

    #[test]
    fn test_multibyte_inside_string_preserved_length() {
        let src = r#"let s = "café == x";"#;
        let m = mask_line(src);
        assert_eq!(m.len(), src.len());
        assert!(!m.contains("=="));
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p ttk-mutate lexer`
Expected: 7 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/mutate/src/lexer.rs
git commit -m "feat(mutate): length-preserving string/comment masker"
```

---

## Task 4: `discover.rs` — site extraction, scoping, cap

Scans target files, masks each line, finds rule matches with longest-match-first
and word-boundary checks for word-like rules, applies `--include`/`--exclude`
globs and optional `--since` git line scope, then caps total with `--max-mutants`
(ranked by operator frequency, dropped count logged by the caller).

**Files:**
- Modify: `crates/mutate/src/discover.rs`

- [ ] **Step 1: Write the failing tests + implementation**

Put this in `crates/mutate/src/discover.rs`:

```rust
use crate::rules::Rule;
use glob::Pattern;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq)]
pub struct Site {
    pub file: PathBuf,
    pub line: usize,
    pub col: usize,
    pub from: String,
    pub to: String,
}

const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", ".venv", "vendor", "dist", "build"];

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_word_rule(rule: &Rule) -> bool {
    let f = rule.find.as_bytes();
    !f.is_empty() && is_word_byte(f[0]) && is_word_byte(f[f.len() - 1])
}

pub fn find_sites_in_line(
    file: &Path,
    line_no: usize,
    line: &str,
    sorted_rules: &[Rule],
) -> Vec<Site> {
    let masked = crate::lexer::mask_line(line);
    let m = masked.as_bytes();
    let mut sites = Vec::new();
    let mut i = 0;
    while i < m.len() {
        let mut matched = false;
        for rule in sorted_rules {
            let f = rule.find.as_bytes();
            if f.is_empty() || i + f.len() > m.len() {
                continue;
            }
            if &m[i..i + f.len()] != f {
                continue;
            }
            if is_word_rule(rule) {
                let before_ok = i == 0 || !is_word_byte(m[i - 1]);
                let after_ok = i + f.len() == m.len() || !is_word_byte(m[i + f.len()]);
                if !(before_ok && after_ok) {
                    continue;
                }
            }
            sites.push(Site {
                file: file.to_path_buf(),
                line: line_no,
                col: i,
                from: rule.find.clone(),
                to: rule.replace.clone(),
            });
            i += f.len();
            matched = true;
            break;
        }
        if !matched {
            i += 1;
        }
    }
    sites
}

pub fn changed_lines(project: &Path, since: &str) -> Result<HashMap<PathBuf, HashSet<usize>>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project)
        .args(["diff", "--unified=0", since, "--"])
        .output()
        .map_err(|e| format!("git diff failed: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "git diff --since {} failed: {}",
            since,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut map: HashMap<PathBuf, HashSet<usize>> = HashMap::new();
    let mut current: Option<PathBuf> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("+++ b/") {
            current = Some(PathBuf::from(rest));
        } else if line.starts_with("@@") {
            if let (Some(file), Some(range)) = (current.clone(), parse_hunk_new_range(line)) {
                let entry = map.entry(file).or_default();
                for n in range {
                    entry.insert(n);
                }
            }
        }
    }
    Ok(map)
}

fn parse_hunk_new_range(hunk: &str) -> Option<std::ops::RangeInclusive<usize>> {
    let plus = hunk.split('+').nth(1)?;
    let spec = plus.split(|c| c == ' ' || c == '@').next()?;
    let mut parts = spec.split(',');
    let start: usize = parts.next()?.parse().ok()?;
    let count: usize = parts.next().map(|c| c.parse().unwrap_or(1)).unwrap_or(1);
    if count == 0 {
        Some(start..=start)
    } else {
        Some(start..=start + count - 1)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn discover_sites(
    project: &Path,
    paths: &[PathBuf],
    rules: &[Rule],
    include: Option<&str>,
    exclude: Option<&str>,
    since: Option<&str>,
) -> Result<Vec<Site>, String> {
    let mut sorted = rules.to_vec();
    sorted.sort_by(|a, b| b.find.len().cmp(&a.find.len()));

    let scope = match since {
        Some(s) => Some(changed_lines(project, s)?),
        None => None,
    };
    let inc = include.map(Pattern::new).transpose().map_err(|e| format!("bad --include glob: {}", e))?;
    let exc = exclude.map(Pattern::new).transpose().map_err(|e| format!("bad --exclude glob: {}", e))?;

    let mut sites = Vec::new();
    for base in paths {
        for entry in WalkDir::new(base).follow_links(false) {
            let entry = entry.map_err(|e| format!("walk error: {}", e))?;
            let path = entry.path();
            if entry.path_is_symlink() || !path.is_file() {
                continue;
            }
            if path.components().any(|c| SKIP_DIRS.contains(&c.as_os_str().to_string_lossy().as_ref())) {
                continue;
            }
            let rel = path.strip_prefix(project).unwrap_or(path);
            let rel_str = rel.to_string_lossy();
            if let Some(p) = &inc {
                if !p.matches(&rel_str) {
                    continue;
                }
            }
            if let Some(p) = &exc {
                if p.matches(&rel_str) {
                    continue;
                }
            }
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let allowed = scope.as_ref().map(|m| m.get(rel).cloned());
            for (idx, line) in content.lines().enumerate() {
                let line_no = idx + 1;
                if let Some(Some(allowed_lines)) = &allowed {
                    if !allowed_lines.contains(&line_no) {
                        continue;
                    }
                } else if let Some(None) = &allowed {
                    continue;
                }
                let mut line_sites = find_sites_in_line(rel, line_no, line, &sorted);
                sites.append(&mut line_sites);
            }
        }
    }
    Ok(sites)
}

pub fn rank_and_cap(mut sites: Vec<Site>, max_mutants: usize) -> (Vec<Site>, usize) {
    let mut freq: HashMap<String, usize> = HashMap::new();
    for s in &sites {
        *freq.entry(s.from.clone()).or_default() += 1;
    }
    sites.sort_by(|a, b| {
        freq[&b.from]
            .cmp(&freq[&a.from])
            .then(a.file.cmp(&b.file))
            .then(a.line.cmp(&b.line))
            .then(a.col.cmp(&b.col))
    });
    if max_mutants > 0 && sites.len() > max_mutants {
        let dropped = sites.len() - max_mutants;
        sites.truncate(max_mutants);
        (sites, dropped)
    } else {
        (sites, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn sorted(rules: Vec<Rule>) -> Vec<Rule> {
        let mut r = rules;
        r.sort_by(|a, b| b.find.len().cmp(&a.find.len()));
        r
    }

    #[test]
    fn test_longest_match_wins() {
        let rules = sorted(vec![
            Rule { find: "<".into(), replace: "<=".into() },
            Rule { find: "<=".into(), replace: "<".into() },
        ]);
        let sites = find_sites_in_line(Path::new("f.rs"), 1, "a <= b", &rules);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].from, "<=");
        assert_eq!(sites[0].col, 2);
    }

    #[test]
    fn test_word_boundary_required() {
        let rules = sorted(vec![Rule { find: "true".into(), replace: "false".into() }]);
        let sites = find_sites_in_line(Path::new("f.rs"), 1, "construed truex true", &rules);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].col, "construed truex ".len());
    }

    #[test]
    fn test_no_match_inside_string() {
        let rules = sorted(vec![Rule { find: "==".into(), replace: "!=".into() }]);
        let sites = find_sites_in_line(Path::new("f.rs"), 1, r#"s = "a == b""#, &rules);
        assert!(sites.is_empty());
    }

    #[test]
    fn test_discover_walks_dir_and_finds_sites() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "fn f() { a == b; }\n").unwrap();
        let rules = vec![Rule { find: "==".into(), replace: "!=".into() }];
        let sites =
            discover_sites(dir.path(), &[dir.path().to_path_buf()], &rules, None, None, None).unwrap();
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].file, PathBuf::from("a.rs"));
        assert_eq!(sites[0].line, 1);
    }

    #[test]
    fn test_include_exclude_globs() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("keep.rs"), "a == b\n").unwrap();
        fs::write(dir.path().join("skip.txt"), "a == b\n").unwrap();
        let rules = vec![Rule { find: "==".into(), replace: "!=".into() }];
        let sites = discover_sites(
            dir.path(),
            &[dir.path().to_path_buf()],
            &rules,
            Some("*.rs"),
            None,
            None,
        )
        .unwrap();
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].file, PathBuf::from("keep.rs"));
    }

    #[test]
    fn test_skip_dirs_ignored() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        fs::write(dir.path().join("node_modules").join("x.js"), "a == b\n").unwrap();
        fs::write(dir.path().join("main.js"), "a == b\n").unwrap();
        let rules = vec![Rule { find: "==".into(), replace: "!=".into() }];
        let sites =
            discover_sites(dir.path(), &[dir.path().to_path_buf()], &rules, None, None, None).unwrap();
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].file, PathBuf::from("main.js"));
    }

    #[test]
    fn test_rank_and_cap_drops_count() {
        let mk = |from: &str, line: usize| Site {
            file: PathBuf::from("f"),
            line,
            col: 0,
            from: from.into(),
            to: "x".into(),
        };
        let sites = vec![mk("==", 1), mk("==", 2), mk("+", 3)];
        let (capped, dropped) = rank_and_cap(sites, 2);
        assert_eq!(capped.len(), 2);
        assert_eq!(dropped, 1);
        assert!(capped.iter().all(|s| s.from == "=="));
    }

    #[test]
    fn test_parse_hunk_new_range() {
        assert_eq!(parse_hunk_new_range("@@ -1,0 +2,3 @@"), Some(2..=4));
        assert_eq!(parse_hunk_new_range("@@ -5 +7 @@"), Some(7..=7));
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p ttk-mutate discover`
Expected: 8 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/mutate/src/discover.rs
git commit -m "feat(mutate): site discovery with masking, globs, git scope, cap"
```

---

## Task 5: `sandbox.rs` — RAII temp project copy with symlinked deps

Each worker gets a `Sandbox`: a temp dir holding a copy of the project source
(reusing `ttk_core::fs_utils::copy_clean_dir`), with heavy dependency dirs
symlinked in so the verdict command resolves installed deps. The `TempDir` is
dropped automatically (cleanup survives panic).

**Files:**
- Modify: `crates/mutate/src/sandbox.rs`

- [ ] **Step 1: Write the failing tests + implementation**

Put this in `crates/mutate/src/sandbox.rs`:

```rust
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use ttk_core::fs_utils::copy_clean_dir;

const LINK_DIRS: &[&str] = &["node_modules", "target", ".venv", "vendor", "dist", "build"];
const SKIP_DIRS: &[&str] = &["node_modules", "target", ".venv", "vendor", "dist", "build", ".git", ".github"];

pub struct Sandbox {
    _dir: TempDir,
    pub root: PathBuf,
}

impl Sandbox {
    pub fn new(project: &Path) -> Result<Self, String> {
        let dir = tempfile::Builder::new()
            .prefix("ttk-mut-")
            .tempdir()
            .map_err(|e| format!("cannot create sandbox tempdir: {}", e))?;
        let root = dir.path().to_path_buf();
        copy_clean_dir(project, &root, SKIP_DIRS, &[])?;
        for d in LINK_DIRS {
            let src = project.join(d);
            if src.exists() {
                let dst = root.join(d);
                symlink_dir(&src, &dst)
                    .map_err(|e| format!("symlink {} into sandbox: {}", d, e))?;
            }
        }
        Ok(Sandbox { _dir: dir, root })
    }

    pub fn path(&self, rel: &Path) -> PathBuf {
        self.root.join(rel)
    }
}

#[cfg(unix)]
fn symlink_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(windows)]
fn symlink_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(src, dst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_sandbox_copies_source() {
        let proj = tempdir().unwrap();
        fs::write(proj.path().join("main.rs"), "fn main() {}").unwrap();
        let sb = Sandbox::new(proj.path()).unwrap();
        assert!(sb.path(Path::new("main.rs")).exists());
    }

    #[test]
    fn test_sandbox_symlinks_node_modules() {
        let proj = tempdir().unwrap();
        fs::create_dir(proj.path().join("node_modules")).unwrap();
        fs::write(proj.path().join("node_modules").join("dep.js"), "x").unwrap();
        fs::write(proj.path().join("main.js"), "y").unwrap();
        let sb = Sandbox::new(proj.path()).unwrap();
        let nm = sb.path(Path::new("node_modules"));
        assert!(nm.exists());
        assert!(nm.join("dep.js").exists());
        #[cfg(unix)]
        assert!(fs::symlink_metadata(&nm).unwrap().file_type().is_symlink());
    }

    #[test]
    fn test_sandbox_cleans_up_on_drop() {
        let proj = tempdir().unwrap();
        fs::write(proj.path().join("a.rs"), "z").unwrap();
        let root;
        {
            let sb = Sandbox::new(proj.path()).unwrap();
            root = sb.root.clone();
            assert!(root.exists());
        }
        assert!(!root.exists());
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p ttk-mutate sandbox`
Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/mutate/src/sandbox.rs
git commit -m "feat(mutate): RAII sandbox with copied source and symlinked deps"
```

---

## Task 6: `runner.rs` — apply mutation, exec verdict, classify

Applies a single mutation to the sandbox copy of a file, runs the optional
`--build` gate then `--test` verdict under a wall-clock timeout, classifies the
outcome, and restores the file. `--retest` re-runs the test and only confirms a
**killed** verdict when all runs fail (conservative against flaky tests).

**Files:**
- Modify: `crates/mutate/src/runner.rs`

- [ ] **Step 1: Write the failing tests + implementation**

Put this in `crates/mutate/src/runner.rs`:

```rust
use crate::discover::Site;
use crate::sandbox::Sandbox;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Killed,
    Survived,
    Timeout,
    Unviable,
}

#[derive(Debug, Clone)]
pub struct MutationResult {
    pub site: Site,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verdict {
    Passed,
    Failed,
    TimedOut,
}

fn run_verdict(cmd: &str, cwd: &Path, timeout: Duration) -> Verdict {
    let child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(_) => return Verdict::Failed,
    };
    match child.wait_timeout(timeout) {
        Ok(Some(status)) => {
            if status.success() {
                Verdict::Passed
            } else {
                Verdict::Failed
            }
        }
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            Verdict::TimedOut
        }
        Err(_) => Verdict::Failed,
    }
}

fn apply_mutation(content: &str, site: &Site) -> Option<String> {
    let mut lines: Vec<&str> = content.split('\n').collect();
    let target = *lines.get(site.line - 1)?;
    if site.col + site.from.len() > target.len() {
        return None;
    }
    if &target[site.col..site.col + site.from.len()] != site.from {
        return None;
    }
    let mutated = format!(
        "{}{}{}",
        &target[..site.col],
        site.to,
        &target[site.col + site.from.len()..]
    );
    let owned = mutated;
    lines[site.line - 1] = &owned;
    Some(lines.join("\n"))
}

pub struct RunConfig<'a> {
    pub test: &'a str,
    pub build: Option<&'a str>,
    pub timeout: Duration,
    pub retest: usize,
}

pub fn run_site(sandbox: &Sandbox, site: &Site, cfg: &RunConfig) -> MutationResult {
    let file = sandbox.path(&site.file);
    let original = match std::fs::read_to_string(&file) {
        Ok(c) => c,
        Err(_) => return MutationResult { site: site.clone(), outcome: Outcome::Survived },
    };
    let mutated = match apply_mutation(&original, site) {
        Some(m) => m,
        None => return MutationResult { site: site.clone(), outcome: Outcome::Survived },
    };
    if std::fs::write(&file, &mutated).is_err() {
        return MutationResult { site: site.clone(), outcome: Outcome::Survived };
    }

    let outcome = classify(&sandbox.root, cfg);

    let _ = std::fs::write(&file, &original);
    MutationResult { site: site.clone(), outcome }
}

fn classify(cwd: &Path, cfg: &RunConfig) -> Outcome {
    if let Some(b) = cfg.build {
        match run_verdict(b, cwd, cfg.timeout) {
            Verdict::TimedOut => return Outcome::Timeout,
            Verdict::Failed => return Outcome::Unviable,
            Verdict::Passed => {}
        }
    }
    let runs = cfg.retest.max(1);
    let mut all_failed = true;
    for _ in 0..runs {
        match run_verdict(cfg.test, cwd, cfg.timeout) {
            Verdict::TimedOut => return Outcome::Timeout,
            Verdict::Passed => all_failed = false,
            Verdict::Failed => {}
        }
    }
    if all_failed {
        Outcome::Killed
    } else {
        Outcome::Survived
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn site(line: usize, col: usize, from: &str, to: &str) -> Site {
        Site { file: PathBuf::from("code.txt"), line, col, from: from.into(), to: to.into() }
    }

    #[test]
    fn test_apply_mutation_swaps_token() {
        let out = apply_mutation("a == b\nc", &site(1, 2, "==", "!=")).unwrap();
        assert_eq!(out, "a != b\nc");
    }

    #[test]
    fn test_apply_mutation_rejects_mismatch() {
        assert!(apply_mutation("a < b", &site(1, 2, "==", "!=")).is_none());
    }

    #[test]
    fn test_run_verdict_pass_fail() {
        let dir = tempdir().unwrap();
        assert_eq!(run_verdict("true", dir.path(), Duration::from_secs(5)), Verdict::Passed);
        assert_eq!(run_verdict("false", dir.path(), Duration::from_secs(5)), Verdict::Failed);
    }

    #[test]
    fn test_run_verdict_timeout() {
        let dir = tempdir().unwrap();
        assert_eq!(
            run_verdict("sleep 5", dir.path(), Duration::from_millis(200)),
            Verdict::TimedOut
        );
    }

    #[test]
    fn test_run_site_survived_when_test_passes() {
        let proj = tempdir().unwrap();
        std::fs::write(proj.path().join("code.txt"), "a == b\n").unwrap();
        let sb = Sandbox::new(proj.path()).unwrap();
        let cfg = RunConfig { test: "true", build: None, timeout: Duration::from_secs(5), retest: 1 };
        let r = run_site(&sb, &site(1, 2, "==", "!="), &cfg);
        assert_eq!(r.outcome, Outcome::Survived);
        assert_eq!(std::fs::read_to_string(sb.path(&PathBuf::from("code.txt"))).unwrap(), "a == b\n");
    }

    #[test]
    fn test_run_site_killed_when_test_fails() {
        let proj = tempdir().unwrap();
        std::fs::write(proj.path().join("code.txt"), "a == b\n").unwrap();
        let sb = Sandbox::new(proj.path()).unwrap();
        let cfg = RunConfig { test: "false", build: None, timeout: Duration::from_secs(5), retest: 1 };
        let r = run_site(&sb, &site(1, 2, "==", "!="), &cfg);
        assert_eq!(r.outcome, Outcome::Killed);
    }

    #[test]
    fn test_run_site_unviable_when_build_fails() {
        let proj = tempdir().unwrap();
        std::fs::write(proj.path().join("code.txt"), "a == b\n").unwrap();
        let sb = Sandbox::new(proj.path()).unwrap();
        let cfg = RunConfig {
            test: "true",
            build: Some("false"),
            timeout: Duration::from_secs(5),
            retest: 1,
        };
        let r = run_site(&sb, &site(1, 2, "==", "!="), &cfg);
        assert_eq!(r.outcome, Outcome::Unviable);
    }
}
```

Note: the `apply_mutation` borrow of `owned` requires the binding to outlive the
`join`. Implement exactly as written — `owned` is declared before assigning into
`lines`, and `lines.join` runs before `owned` drops.

- [ ] **Step 2: Run the tests**

Run: `cargo test -p ttk-mutate runner`
Expected: 7 tests pass. (`sleep`/`true`/`false` are POSIX; this targets Linux per the repo environment.)

- [ ] **Step 3: Commit**

```bash
git add crates/mutate/src/runner.rs
git commit -m "feat(mutate): mutation runner with build gate, timeout, retest"
```

---

## Task 7: `report.rs` — score + stdout + markdown

Aggregates results into counts, computes `killed / (killed + survived)`, and
renders survivors as `file:line` lines with the `from → to` change. Prominently
shows unviable/timeout counts and the no-`--build` caveat.

**Files:**
- Modify: `crates/mutate/src/report.rs`

- [ ] **Step 1: Write the failing tests + implementation**

Put this in `crates/mutate/src/report.rs`:

```rust
use crate::runner::{MutationResult, Outcome};

pub struct Report {
    pub killed: usize,
    pub survived: usize,
    pub timeout: usize,
    pub unviable: usize,
    pub survivors: Vec<MutationResult>,
    pub dropped: usize,
    pub build_used: bool,
}

impl Report {
    pub fn from_results(results: &[MutationResult], dropped: usize, build_used: bool) -> Report {
        let mut r = Report {
            killed: 0,
            survived: 0,
            timeout: 0,
            unviable: 0,
            survivors: Vec::new(),
            dropped,
            build_used,
        };
        for res in results {
            match res.outcome {
                Outcome::Killed => r.killed += 1,
                Outcome::Survived => {
                    r.survived += 1;
                    r.survivors.push(res.clone());
                }
                Outcome::Timeout => r.timeout += 1,
                Outcome::Unviable => r.unviable += 1,
            }
        }
        r
    }

    pub fn score(&self) -> Option<f64> {
        let denom = self.killed + self.survived;
        if denom == 0 {
            None
        } else {
            Some(self.killed as f64 / denom as f64)
        }
    }

    pub fn render_stdout(&self) -> String {
        let mut out = String::new();
        out.push_str("\n=== ttk mutate report ===\n");
        match self.score() {
            Some(s) => out.push_str(&format!("mutation score: {:.1}% ({}/{} killed)\n", s * 100.0, self.killed, self.killed + self.survived)),
            None => out.push_str("mutation score: n/a (no viable mutants)\n"),
        }
        out.push_str(&format!(
            "killed: {}  survived: {}  timeout: {}  unviable: {}\n",
            self.killed, self.survived, self.timeout, self.unviable
        ));
        if self.dropped > 0 {
            out.push_str(&format!("dropped (--max-mutants cap): {}\n", self.dropped));
        }
        if !self.build_used {
            out.push_str("note: no --build given; compile-broken mutants counted as killed\n");
        }
        if self.survivors.is_empty() {
            out.push_str("\nno survivors — tests caught every viable mutant\n");
        } else {
            out.push_str("\nsurvivors (test gaps):\n");
            for s in &self.survivors {
                out.push_str(&format!(
                    "  {}:{}  {} -> {}\n",
                    s.site.file.display(),
                    s.site.line,
                    s.site.from,
                    s.site.to
                ));
            }
        }
        out
    }

    pub fn render_markdown(&self) -> String {
        let mut out = String::from("# ttk mutate report\n\n");
        match self.score() {
            Some(s) => out.push_str(&format!("**Mutation score:** {:.1}% ({}/{} killed)\n\n", s * 100.0, self.killed, self.killed + self.survived)),
            None => out.push_str("**Mutation score:** n/a (no viable mutants)\n\n"),
        }
        out.push_str(&format!(
            "| killed | survived | timeout | unviable | dropped |\n|---|---|---|---|---|\n| {} | {} | {} | {} | {} |\n\n",
            self.killed, self.survived, self.timeout, self.unviable, self.dropped
        ));
        if !self.build_used {
            out.push_str("> Note: no `--build` given; compile-broken mutants counted as killed.\n\n");
        }
        out.push_str("## Survivors\n\n");
        if self.survivors.is_empty() {
            out.push_str("None — tests caught every viable mutant.\n");
        } else {
            out.push_str("| file | line | from | to |\n|---|---|---|---|\n");
            for s in &self.survivors {
                out.push_str(&format!(
                    "| {} | {} | `{}` | `{}` |\n",
                    s.site.file.display(),
                    s.site.line,
                    s.site.from,
                    s.site.to
                ));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::Site;
    use std::path::PathBuf;

    fn res(outcome: Outcome) -> MutationResult {
        MutationResult {
            site: Site { file: PathBuf::from("f.rs"), line: 1, col: 0, from: "==".into(), to: "!=".into() },
            outcome,
        }
    }

    #[test]
    fn test_counts_and_score() {
        let results = vec![res(Outcome::Killed), res(Outcome::Killed), res(Outcome::Survived)];
        let r = Report::from_results(&results, 0, false);
        assert_eq!(r.killed, 2);
        assert_eq!(r.survived, 1);
        assert!((r.score().unwrap() - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn test_score_excludes_unviable_and_timeout() {
        let results = vec![res(Outcome::Killed), res(Outcome::Unviable), res(Outcome::Timeout)];
        let r = Report::from_results(&results, 0, true);
        assert_eq!(r.score(), Some(1.0));
    }

    #[test]
    fn test_score_none_when_no_viable() {
        let results = vec![res(Outcome::Unviable)];
        let r = Report::from_results(&results, 0, true);
        assert_eq!(r.score(), None);
    }

    #[test]
    fn test_stdout_lists_survivors_and_caveat() {
        let r = Report::from_results(&[res(Outcome::Survived)], 0, false);
        let out = r.render_stdout();
        assert!(out.contains("survivors"));
        assert!(out.contains("f.rs:1"));
        assert!(out.contains("no --build given"));
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p ttk-mutate report`
Expected: 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/mutate/src/report.rs
git commit -m "feat(mutate): score math and stdout/markdown report"
```

---

## Task 8: `lib.rs` — orchestrator

Wires everything: load rules, run baseline (must be green, under a baseline
timeout), discover + cap sites, fan out across `--jobs` worker threads (one
sandbox each), aggregate, print report, write optional markdown.

**Files:**
- Modify: `crates/mutate/src/lib.rs`

- [ ] **Step 1: Replace `lib.rs` with the orchestrator**

```rust
pub mod discover;
pub mod lexer;
pub mod report;
pub mod rules;
pub mod runner;
pub mod sandbox;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use ttk_core::tlog;

use discover::{discover_sites, rank_and_cap};
use report::Report;
use runner::{run_site, MutationResult, RunConfig};
use sandbox::Sandbox;

const BASELINE_TIMEOUT: Duration = Duration::from_secs(300);

pub struct MutateArgs<'a> {
    pub paths: &'a [PathBuf],
    pub test: &'a str,
    pub build: Option<&'a str>,
    pub config: Option<&'a Path>,
    pub jobs: usize,
    pub since: Option<&'a str>,
    pub include: Option<&'a str>,
    pub exclude: Option<&'a str>,
    pub timeout: Option<u64>,
    pub max_mutants: usize,
    pub retest: usize,
    pub report: Option<&'a Path>,
}

fn run_shell(cmd: &str, cwd: &Path, timeout: Duration) -> Result<bool, String> {
    use std::process::{Command, Stdio};
    use wait_timeout::ChildExt;
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot run command '{}': {}", cmd, e))?;
    match child.wait_timeout(timeout) {
        Ok(Some(status)) => Ok(status.success()),
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!("command timed out after {:?}: {}", timeout, cmd))
        }
        Err(e) => Err(format!("command failed: {}: {}", cmd, e)),
    }
}

pub fn run(args: MutateArgs) -> Result<(), String> {
    let project = std::env::current_dir().map_err(|e| format!("cannot read cwd: {}", e))?;

    let rules = rules::load_rules(args.config)?;

    tlog("EXECUTING: baseline test run...");
    let start = Instant::now();
    let baseline_green = run_shell(args.test, &project, BASELINE_TIMEOUT)?;
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
        &project,
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
        let report = Report::from_results(&[], 0, args.build.is_some());
        print!("{}", report.render_stdout());
        return Ok(());
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

    let results: Vec<MutationResult> = std::thread::scope(|scope| {
        let handles: Vec<_> = buckets
            .into_iter()
            .map(|bucket| {
                scope.spawn(|| {
                    let sandbox = match Sandbox::new(&project) {
                        Ok(s) => s,
                        Err(e) => {
                            tlog(&format!("sandbox error (bucket skipped): {}", e));
                            return Vec::new();
                        }
                    };
                    bucket.iter().map(|site| run_site(&sandbox, site, &cfg)).collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    });

    let report = Report::from_results(&results, dropped, args.build.is_some());
    print!("{}", report.render_stdout());

    if let Some(path) = args.report {
        std::fs::write(path, report.render_markdown())
            .map_err(|e| format!("cannot write report {}: {}", path.display(), e))?;
        tlog(&format!("markdown report written to {}", path.display()));
    }

    Ok(())
}
```

- [ ] **Step 2: Verify the crate builds and all module tests still pass**

Run: `cargo test -p ttk-mutate`
Expected: all tests from Tasks 2–7 pass (28 total).

- [ ] **Step 3: Commit**

```bash
git add crates/mutate/src/lib.rs
git commit -m "feat(mutate): orchestrator wiring baseline, discover, parallel run, report"
```

---

## Task 9: CLI wiring

**Files:**
- Modify: `crates/cli/Cargo.toml`
- Modify: `crates/cli/src/main.rs`

- [ ] **Step 1: Add the crate dependency**

In `crates/cli/Cargo.toml`, under `[dependencies]`, add:

```toml
ttk-mutate = { path = "../mutate" }
```

- [ ] **Step 2: Add the `Mutate` subcommand variant**

In `crates/cli/src/main.rs`, inside `enum Commands { ... }` (after the `Mp4Optimize` variant), add:

```rust
    /// Language-agnostic mutation testing: mutate source, run your test command, report survivors
    Mutate {
        /// Files or directories to mutate
        paths: Vec<PathBuf>,
        /// Verdict command; green (exit 0) = pass
        #[arg(long)]
        test: String,
        /// Optional build/compile command; non-zero = unviable mutant
        #[arg(long)]
        build: Option<String>,
        /// Mutation rule overlay TOML (default: .ttk-mutate.toml if present)
        #[arg(long)]
        config: Option<PathBuf>,
        /// Parallel workers (default: number of CPUs)
        #[arg(long)]
        jobs: Option<usize>,
        /// Only mutate lines changed since this git ref
        #[arg(long)]
        since: Option<String>,
        /// Only mutate files matching this glob (relative path)
        #[arg(long)]
        include: Option<String>,
        /// Skip files matching this glob (relative path)
        #[arg(long)]
        exclude: Option<String>,
        /// Per-mutant timeout in seconds (default: 3x baseline)
        #[arg(long)]
        timeout: Option<u64>,
        /// Cap total mutants; rank by frequency, drop rest (0 = unlimited)
        #[arg(long, default_value_t = 0)]
        max_mutants: usize,
        /// Re-run flipped verdicts N times; killed only if all fail
        #[arg(long, default_value_t = 1)]
        retest: usize,
        /// Write a markdown report to this path
        #[arg(long)]
        report: Option<PathBuf>,
    },
```

- [ ] **Step 3: Add the dispatch arm**

In `run_command`'s `match cli.command { ... }` (after the `Mp4Optimize` arm), add:

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
        } => {
            if paths.is_empty() {
                return Err("mutate: provide at least one path to mutate".to_string());
            }
            let jobs = jobs.unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1));
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
            })
        }
```

- [ ] **Step 4: Verify the whole workspace builds**

Run: `cargo build`
Expected: clean build.

- [ ] **Step 5: Smoke-test the help**

Run: `cargo run -- mutate --help`
Expected: usage shows `--test`, `--build`, `--since`, `--max-mutants`, etc.

- [ ] **Step 6: Commit**

```bash
git add crates/cli/Cargo.toml crates/cli/src/main.rs
git commit -m "feat(cli): wire ttk mutate subcommand"
```

---

## Task 10: End-to-end integration test

Proves the full command finds a real survivor. Uses a tiny throwaway project
whose "tests" are a shell script with a deliberate gap (it checks `add` but not
`is_even`), so mutating `is_even` survives while mutating `add` is killed.

**Files:**
- Create: `crates/mutate/tests/e2e.rs`

- [ ] **Step 1: Write the integration test**

Create `crates/mutate/tests/e2e.rs`:

```rust
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

fn ttk_bin() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("target");
    p.push("debug");
    p.push("ttk");
    p
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

#[test]
fn test_mutate_reports_survivor_for_untested_code() {
    let bin = ttk_bin();
    if !bin.exists() {
        panic!("build the workspace first: cargo build (expected {})", bin.display());
    }
    let proj = tempdir().unwrap();
    let root = proj.path();

    write(
        &root.join("code.sh"),
        "add() { echo $(( $1 + $2 )); }\nis_even() { [ $(( $1 % 2 )) -eq 0 ] && echo yes || echo no; }\n",
    );
    write(
        &root.join("run_tests.sh"),
        "#!/bin/sh\n. ./code.sh\n[ \"$(add 2 3)\" = \"5\" ] || exit 1\nexit 0\n",
    );

    let report_path = root.join("report.md");
    let output = Command::new(&bin)
        .current_dir(root)
        .args([
            "mutate",
            "code.sh",
            "--test",
            "sh run_tests.sh",
            "--jobs",
            "1",
            "--report",
        ])
        .arg(&report_path)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "ttk mutate failed: {}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains("survivors"), "expected survivors in output:\n{}", stdout);

    let report = fs::read_to_string(&report_path).unwrap();
    assert!(report.contains("Mutation score"));
    assert!(report.contains("code.sh"));
}

#[test]
fn test_mutate_aborts_on_red_baseline() {
    let bin = ttk_bin();
    if !bin.exists() {
        panic!("build the workspace first: cargo build");
    }
    let proj = tempdir().unwrap();
    let root = proj.path();
    write(&root.join("code.sh"), "x() { echo 1; }\n");

    let output = Command::new(&bin)
        .current_dir(root)
        .args(["mutate", "code.sh", "--test", "false"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("baseline test command failed"));
}
```

- [ ] **Step 2: Build then run the integration test**

Run: `cargo build && cargo test -p ttk-mutate --test e2e`
Expected: both tests pass. (The `add` mutant `+ -> -` is killed by the test; the
`is_even` mutants `% ...`, `0 -> 1` survive because nothing tests `is_even`.)

- [ ] **Step 3: Commit**

```bash
git add crates/mutate/tests/e2e.rs
git commit -m "test(mutate): end-to-end survivor detection and red-baseline abort"
```

---

## Task 11: Documentation

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add a `ttk mutate` section**

In `README.md`, after the last command section, add:

````markdown
### `ttk mutate <paths>... --test "<cmd>"`

Language-agnostic mutation testing. Mutates source text (operator swaps), runs
your test command, and reports which mutants survived — i.e. where your tests are
blind.

```bash
ttk mutate src --test "cargo test"
ttk mutate src --test "pytest -q" --build "python -c 'import compileall,sys; sys.exit(0 if compileall.compile_dir(\"src\",quiet=1) else 1)'"
ttk mutate . --test "npm test" --since HEAD~5 --jobs 8
```

- `--test` (required): command that exits 0 when healthy. A mutant that keeps it
  green is a survivor (test gap).
- `--build` (optional): compile step run before the test; failure marks the
  mutant *unviable* instead of *killed*. Without it, compile-broken mutants count
  as killed.
- `--since <ref>`: only mutate lines changed since a git ref (great for CI).
- `--max-mutants <N>`, `--jobs <N>`, `--retest <N>`, `--timeout <secs>`,
  `--include`/`--exclude` globs, `--config <file>`, `--report <file.md>`.

Extend the rule table per repo with `.ttk-mutate.toml`:

```toml
[[rule]]
find = "and"
replace = "or"
```
````

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: document ttk mutate command"
```

---

## Self-Review Notes (already applied)

- **Spec coverage:** rule table + overlay (T2), masking (T3), discovery + globs +
  `--since` + cap (T4), sandbox symlink + RAII (T5), build gate + timeout +
  retest + classify (T6), honest scoring + caveat (T7), baseline + parallel
  orchestration (T8), CLI flags incl. `--build`/`--max-mutants`/`--retest` (T9),
  e2e survivor + red-baseline (T10), docs (T11). All spec sections mapped.
- **Type consistency:** `Site`, `Rule`, `Outcome`, `MutationResult`, `RunConfig`,
  `Report`, `MutateArgs`, `Sandbox` names/fields are identical across tasks.
- **Known v1 limitations (intentional, per spec):** masker is per-line (multiline
  strings/block comments spanning lines are a documented heuristic gap); unviable
  requires `--build`; structural mutations deferred to v2.
```
