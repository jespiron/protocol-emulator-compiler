//! A project is a directory with a `main.spt`. Build output goes to `target/` beside it.
use crate::AppResult;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const SOURCE: &str = "main.spt";

pub(crate) struct Project {
    pub source: PathBuf,
    pub text: String,
}

impl Project {
    /// The given file, or else the nearest `main.spt` in this or a parent directory.
    pub(crate) fn find(file: Option<&Path>) -> AppResult<Self> {
        let source = match file {
            Some(file) => file.to_owned(),
            None => std::env::current_dir()?
                .ancestors()
                .map(|dir| dir.join(SOURCE))
                .find(|path| path.is_file())
                .ok_or("no main.spt here or in any parent directory; start one with `spatial new <name>`")?,
        };
        let text = fs::read_to_string(&source)
            .map_err(|e| format!("cannot read {}: {e}", source.display()))?;
        Ok(Self { source, text })
    }

    pub(crate) fn target(&self) -> PathBuf {
        self.source
            .parent()
            .unwrap_or(Path::new("."))
            .join("target")
    }

    /// The source path as the user would write it, for error messages.
    pub(crate) fn display_path(&self) -> String {
        let cwd = std::env::current_dir().unwrap_or_default();
        self.source
            .strip_prefix(&cwd)
            .unwrap_or(&self.source)
            .display()
            .to_string()
    }
}

/// Create `name/main.spt`; never overwrites an existing directory.
pub(crate) fn new(name: &Path) -> AppResult<()> {
    fs::create_dir(name).map_err(|e| format!("cannot create {}: {e}", name.display()))?;
    fs::write(name.join(SOURCE), include_str!("../examples/loopback.spt"))?;
    fs::write(name.join(".gitignore"), "/target/\n")?;
    Ok(())
}
