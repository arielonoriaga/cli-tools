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
