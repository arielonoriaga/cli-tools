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
