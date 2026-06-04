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
