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
