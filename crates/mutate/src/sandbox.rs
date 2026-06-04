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
