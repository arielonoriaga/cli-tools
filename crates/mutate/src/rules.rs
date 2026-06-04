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
