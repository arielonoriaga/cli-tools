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

    let mut killed = 0;
    let mut survived = 0;
    let mut timeout = 0;
    let mut unviable = 0;
    let mut survivors = Vec::new();

    for entry in &f.outcomes {
        match entry.summary.as_str() {
            "CaughtMutant" => killed += 1,
            "MissedMutant" => {
                survived += 1;
                let mutant = &entry.scenario["Mutant"];
                let file = mutant["file"].as_str().unwrap_or("(detail unavailable)").to_string();
                let line = mutant["span"]["start"]["line"].as_u64().unwrap_or(0) as usize;
                let desc = mutant["name"].as_str().unwrap_or("(detail unavailable)").to_string();
                survivors.push(MutationResult {
                    site: Site { file: PathBuf::from(file), line, col: 0, from: desc, to: String::new() },
                    outcome: Outcome::Survived,
                });
            }
            "Timeout" => timeout += 1,
            "Unviable" => unviable += 1,
            _ => {}
        }
    }

    Ok(Report {
        killed,
        survived,
        timeout,
        unviable,
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
        if p == Path::new(".") {
            continue;
        }
        let glob = if p.is_dir() {
            format!("{}/**", p.display())
        } else {
            p.display().to_string()
        };
        cmd.arg("-f").arg(&glob);
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
    let status = cmd
        .status()
        .map_err(|e| format!("failed to launch cargo-mutants: {}", e))?;
    match status.code() {
        Some(0) | Some(2) => {}
        Some(4) => {
            return Err(
                "cargo-mutants baseline failed (unmutated tree does not build or pass its own tests) — fix it before mutating; see output above"
                    .to_string(),
            );
        }
        other => {
            return Err(format!(
                "cargo-mutants did not complete (exit {:?}); run may be incomplete — see output above",
                other
            ));
        }
    }

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

    #[test]
    fn test_counts_from_tally_not_toplevel_fields() {
        let json = r#"{
            "caught": 999, "missed": 999, "timeout": 999, "unviable": 999,
            "outcomes": [
                { "summary": "CaughtMutant", "scenario": { "Mutant": { "name": "x", "file": "a.rs", "span": { "start": { "line": 1 } } } } },
                { "summary": "MissedMutant", "scenario": { "Mutant": { "name": "y", "file": "a.rs", "span": { "start": { "line": 2 } } } } }
            ]
        }"#;
        let r = parse_outcomes(json).unwrap();
        assert_eq!(r.killed, 1);
        assert_eq!(r.survived, 1);
        assert_eq!(r.timeout, 0);
        assert_eq!(r.unviable, 0);
    }
}
