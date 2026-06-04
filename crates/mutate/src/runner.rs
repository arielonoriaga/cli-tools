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
    let lines: Vec<&str> = content.split('\n').collect();
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
    let mut result_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    result_lines[site.line - 1] = mutated;
    Some(result_lines.join("\n"))
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
