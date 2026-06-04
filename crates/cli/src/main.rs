use chrono::{DateTime, Local, NaiveDate};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ttk", version, about = "Terminal Tool Kit")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Delete files by index (even or odd)
    Deleter {
        directory: PathBuf,
        /// Delete even-indexed files (default: odd)
        #[arg(short, long)]
        even: bool,
    },
    /// Rename files sequentially (file0.txt, file1.txt, ...)
    Rename {
        new_name: String,
        #[arg(short, long, default_value = ".")]
        directory: PathBuf,
    },
    /// Optimize images in a directory
    Optimize {
        directory: PathBuf,
        #[arg(short, long, default_value = "80")]
        quality: u8,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Keep original file (default: true). Use --keep-original=false to overwrite
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        keep_original: bool,
    },
    /// Clone a repo and optionally reset history
    CloneRepo {
        repo_url: String,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Reset git history after clone (default: true). Use --reset=false to keep history
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        reset: bool,
    },
    /// Summarize commits across repos in a date range
    GitDigest {
        dir: PathBuf,
        /// Date ISO 8601, e.g. 2026-05-01
        #[arg(long)]
        since: Option<String>,
        /// Date ISO 8601, e.g. 2026-05-28
        #[arg(long)]
        until: Option<String>,
        /// Shorthand: 24h | 7d | 30d
        #[arg(long)]
        last: Option<String>,
        /// Output directory for markdown file
        #[arg(long)]
        output_dir: Option<PathBuf>,
    },
    /// Generate MD5 and SHA1 checksums for all files in a folder
    Sign {
        folder: PathBuf,
        prefix: String,
    },
    /// Copy a directory tree, stripping build artifacts and checksums
    CopyClean {
        source: PathBuf,
        dest: PathBuf,
    },
    /// Copy-clean, sign, and zip a source directory
    BuildAndSign {
        source: PathBuf,
        prefix: String,
    },
    /// Compress MP3 files to M4A via ffmpeg
    Mp3Compress {
        #[arg(default_value = ".")]
        input: PathBuf,
        output_dir: Option<PathBuf>,
        #[arg(long)]
        stereo: bool,
    },
    /// Optimize MP4 files via ffmpeg
    Mp4Optimize {
        #[arg(default_value = ".")]
        input: PathBuf,
        output_dir: Option<PathBuf>,
        /// Quality preset: high | medium | low | web
        #[arg(long, default_value = "web")]
        quality: String,
    },
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
}

fn parse_since_date(s: &str) -> Result<DateTime<Local>, String> {
    parse_date_at(s, 0, 0, 0)
}

fn parse_until_date(s: &str) -> Result<DateTime<Local>, String> {
    parse_date_at(s, 23, 59, 59)
}

fn parse_date_at(s: &str, h: u32, m: u32, sec: u32) -> Result<DateTime<Local>, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| format!("invalid date '{}', use YYYY-MM-DD", s))?
        .and_hms_opt(h, m, sec)
        .unwrap()
        .and_local_timezone(Local)
        .earliest()
        .ok_or_else(|| format!("ambiguous local time for '{}'", s))
}

fn run_command(cli: Cli) -> Result<(), String> {
    match cli.command {
        Commands::Deleter { directory, even } => {
            ttk_deleter::run(&directory, even)
        }
        Commands::Rename { new_name, directory } => {
            ttk_rename::run(&directory, &new_name)
        }
        Commands::Optimize { directory, quality, output, keep_original } => {
            ttk_optimize::run(ttk_optimize::OptimizeArgs {
                input: &directory,
                output: output.as_deref(),
                quality,
                keep_original,
            })
        }
        Commands::CloneRepo { repo_url, output, reset } => {
            ttk_clone_repo::run(ttk_clone_repo::CloneArgs {
                url: &repo_url,
                output: output.as_deref(),
                reset_history: reset,
            })
        }
        Commands::GitDigest { dir, since, until, last, output_dir } => {
            ttk_git_digest::run(ttk_git_digest::GitDigestArgs {
                dir,
                since: since.as_deref().map(parse_since_date).transpose()?,
                until: until.as_deref().map(parse_until_date).transpose()?,
                last,
                output_dir,
            })
        }
        Commands::Sign { folder, prefix } => {
            ttk_sign::run(ttk_sign::SignArgs {
                folder: &folder,
                prefix: &prefix,
            })
        }
        Commands::CopyClean { source, dest } => {
            ttk_copy_clean::run(ttk_copy_clean::CopyCleanArgs {
                source: &source,
                dest: &dest,
            })
        }
        Commands::BuildAndSign { source, prefix } => {
            ttk_build_and_sign::run(ttk_build_and_sign::BuildAndSignArgs {
                source: &source,
                prefix: &prefix,
            })
        }
        Commands::Mp3Compress { input, output_dir, stereo } => {
            ttk_mp3_compress::run(ttk_mp3_compress::Mp3CompressArgs {
                input: &input,
                output_dir: output_dir.as_deref(),
                stereo,
            })
        }
        Commands::Mp4Optimize { input, output_dir, quality } => {
            let q = ttk_mp4_optimize::Quality::parse(&quality)?;
            ttk_mp4_optimize::run(ttk_mp4_optimize::Mp4OptimizeArgs {
                input: &input,
                output_dir: output_dir.as_deref(),
                quality: q,
            })
        }
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
    }
}

fn main() {
    if let Err(e) = run_command(Cli::parse()) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}
