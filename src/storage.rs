use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, ensure};
use sha2::{Digest, Sha256};

pub(crate) fn data_root() -> Result<PathBuf> {
    if let Some(value) = env::var_os("XDG_DATA_HOME") {
        let path = PathBuf::from(value);
        ensure!(path.is_absolute(), "XDG_DATA_HOME must be an absolute path");
        return Ok(path.join("ayeque-forge"));
    }
    let home =
        env::var_os("HOME").ok_or_else(|| anyhow!("neither XDG_DATA_HOME nor HOME is set"))?;
    let home = PathBuf::from(home);
    ensure!(home.is_absolute(), "HOME must be an absolute path");
    Ok(home.join(".local/share/ayeque-forge"))
}

pub(crate) fn ensure_layout(root: &Path) -> Result<()> {
    for directory in [root, &root.join("git"), &root.join("checkouts")] {
        fs::create_dir_all(directory)
            .with_context(|| format!("failed to create {}", directory.display()))?;
    }
    Ok(())
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent", path.display()))?;
    let temporary = sibling_temporary(path);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .with_context(|| format!("failed to create {}", temporary.display()))?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)
            .with_context(|| format!("failed to write {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync {}", temporary.display()))?;
        drop(file);
        fs::rename(&temporary, path)
            .with_context(|| format!("failed to atomically replace {}", path.display()))?;
        let directory = fs::File::open(parent)
            .with_context(|| format!("failed to open {} for sync", parent.display()))?;
        directory
            .sync_all()
            .with_context(|| format!("failed to sync {}", parent.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn sibling_temporary(path: &Path) -> PathBuf {
    let mut name = OsString::from(".");
    name.push(path.file_name().unwrap_or_else(|| OsStr::new("forge")));
    name.push(format!(".tmp-{}", std::process::id()));
    path.with_file_name(name)
}

pub(crate) fn remove_internal_path_if_present(path: &Path, required_parent: &Path) -> Result<()> {
    ensure!(
        path.parent() == Some(required_parent),
        "refusing to remove an unmanaged path"
    );
    if path.is_dir() {
        fs::remove_dir_all(path)
            .with_context(|| format!("failed to remove stale {}", path.display()))?;
    } else if path.exists() {
        fs::remove_file(path)
            .with_context(|| format!("failed to remove stale {}", path.display()))?;
    }
    Ok(())
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("value");
        fs::write(&path, "old").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
    }
}
