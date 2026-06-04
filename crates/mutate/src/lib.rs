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

    let project_ref = &project;
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
