use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, anyhow, bail, ensure};

use crate::storage::{remove_internal_path_if_present, sha256_hex, sibling_temporary};

pub(crate) struct ResolvedTree {
    pub(crate) id: String,
    pub(crate) lfs: Vec<LfsPointer>,
}

pub(crate) struct LfsPointer {
    path: String,
    oid: String,
    size: u64,
}

pub(crate) fn ensure_repository(
    data_root: &Path,
    source_hash: &str,
    source: &str,
) -> Result<PathBuf> {
    let parent = data_root.join("git");
    fs::create_dir_all(&parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;
    let repository = parent.join(source_hash);
    if repository.exists() {
        let bare = git_output(&repository, ["rev-parse", "--is-bare-repository"])?;
        ensure!(
            String::from_utf8_lossy(&bare.stdout).trim() == "true",
            "managed Git cache is not a bare repository"
        );
        return Ok(repository);
    }

    let temporary = sibling_temporary(&repository);
    remove_internal_path_if_present(&temporary, &parent)?;
    let output = Command::new("git")
        .args([
            OsStr::new("clone"),
            OsStr::new("--mirror"),
            OsStr::new("--"),
            OsStr::new(source),
        ])
        .arg(&temporary)
        .output()
        .context("failed to execute git clone")?;
    require_success(output, "git clone --mirror")?;
    fs::rename(&temporary, &repository).with_context(|| {
        format!(
            "failed to install managed Git cache at {}",
            repository.display()
        )
    })?;
    Ok(repository)
}

pub(crate) fn fetch_revision(repository: &Path, source: &str, revision: &str) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["fetch", "--force", "--tags", "--", source, revision])
        .output()
        .context("failed to execute git fetch")?;
    require_success(output, "git fetch")?;
    let output = git_output(repository, ["rev-parse", "--verify", "FETCH_HEAD^{commit}"])?;
    let commit = String::from_utf8(output.stdout).context("git returned a non-UTF-8 commit id")?;
    Ok(commit.trim().to_owned())
}

pub(crate) fn resolve_tree(
    repository: &Path,
    commit: &str,
    entity_path: &str,
) -> Result<ResolvedTree> {
    let object = if entity_path == "." {
        format!("{commit}^{{tree}}")
    } else {
        format!("{commit}:{entity_path}")
    };
    let output = git_output(repository, ["rev-parse", "--verify", object.as_str()])?;
    let tree = String::from_utf8(output.stdout).context("git returned a non-UTF-8 tree id")?;
    let tree = tree.trim().to_owned();
    let output = git_output(repository, ["cat-file", "-t", tree.as_str()])?;
    ensure!(
        String::from_utf8_lossy(&output.stdout).trim() == "tree",
        "entity path does not resolve to a directory tree"
    );

    let output = git_output(repository, ["ls-tree", "-r", "-z", tree.as_str()])?;
    let mut lfs = Vec::new();
    for record in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| anyhow!("git ls-tree returned an invalid record"))?;
        let metadata = std::str::from_utf8(&record[..tab])
            .context("git ls-tree returned non-UTF-8 metadata")?;
        let path = std::str::from_utf8(&record[tab + 1..])
            .context("entity tree contains a non-UTF-8 path")?;
        let mut fields = metadata.split_ascii_whitespace();
        let mode = fields.next().unwrap_or_default();
        let object_type = fields.next().unwrap_or_default();
        let object = fields.next().unwrap_or_default();
        ensure!(mode != "160000", "entity tree contains a Git submodule");
        if object_type != "blob" {
            continue;
        }
        let size = git_output(repository, ["cat-file", "-s", object])?;
        let size = String::from_utf8_lossy(&size.stdout)
            .trim()
            .parse::<u64>()
            .context("git returned an invalid blob size")?;
        if size > 1024 {
            continue;
        }
        let content = git_output(repository, ["cat-file", "blob", object])?.stdout;
        if let Some((oid, size)) = parse_lfs_pointer(&content) {
            lfs.push(LfsPointer {
                path: path.to_owned(),
                oid,
                size,
            });
        }
    }
    Ok(ResolvedTree { id: tree, lfs })
}

pub(crate) fn ensure_checkout(
    data_root: &Path,
    source_hash: &str,
    commit: &str,
    repository: &Path,
) -> Result<PathBuf> {
    let parent = data_root
        .join("git")
        .join(".staging-checkouts")
        .join(source_hash);
    fs::create_dir_all(&parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;
    let checkout = parent.join(commit);
    if checkout.exists() && checkout_is_valid(&checkout, commit)? {
        return Ok(checkout);
    }
    remove_internal_path_if_present(&checkout, &parent)?;

    let temporary = sibling_temporary(&checkout);
    remove_internal_path_if_present(&temporary, &parent)?;
    let output = Command::new("git")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .args(["clone", "--no-checkout", "--"])
        .arg(repository)
        .arg(&temporary)
        .output()
        .context("failed to execute git clone for checkout")?;
    require_success(output, "git clone --no-checkout")?;
    let output = Command::new("git")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .arg("-C")
        .arg(&temporary)
        .args(["checkout", "--detach", commit])
        .output()
        .context("failed to execute git checkout")?;
    require_success(output, "git checkout --detach")?;
    fs::rename(&temporary, &checkout)
        .with_context(|| format!("failed to install checkout at {}", checkout.display()))?;
    Ok(checkout)
}

fn checkout_is_valid(checkout: &Path, expected_commit: &str) -> Result<bool> {
    if !checkout.is_dir() {
        return Ok(false);
    }
    let head = match git_output(checkout, ["rev-parse", "HEAD"]) {
        Ok(output) => output,
        Err(_) => return Ok(false),
    };
    if String::from_utf8_lossy(&head.stdout).trim() != expected_commit {
        return Ok(false);
    }
    let status = git_output(checkout, ["status", "--porcelain"])?;
    Ok(status.stdout.is_empty())
}

pub(crate) fn materialize_lfs(
    checkout: &Path,
    entity_path: &str,
    pointers: &[LfsPointer],
) -> Result<()> {
    if !pointers.is_empty() {
        git_output(checkout, ["lfs", "install", "--local"])?;
    }
    for pointer in pointers {
        let relative = entity_relative_path(entity_path, &pointer.path);
        let include = format!("--include={}", relative.to_string_lossy());
        let pull = git_output(checkout, ["lfs", "pull", include.as_str(), "--exclude="])?;
        let relative_argument = relative.to_string_lossy();
        let checkout_output = git_output(
            checkout,
            ["lfs", "checkout", "--", relative_argument.as_ref()],
        )?;

        let materialized = checkout.join(&relative);
        let bytes = fs::read(&materialized).with_context(|| {
            format!(
                "failed to read materialized LFS file {}",
                materialized.display()
            )
        })?;
        ensure!(
            bytes.len() as u64 == pointer.size,
            "LFS file {} has the wrong size; pull: {}; checkout: {}",
            relative.display(),
            String::from_utf8_lossy(&pull.stdout).trim(),
            String::from_utf8_lossy(&checkout_output.stdout).trim()
        );
        ensure!(
            sha256_hex(&bytes) == pointer.oid,
            "LFS file {} has the wrong SHA-256",
            relative.display()
        );
    }
    Ok(())
}

pub(crate) fn fetch_lfs(
    repository: &Path,
    commit: &str,
    entity_path: &str,
    pointers: &[LfsPointer],
) -> Result<()> {
    for pointer in pointers {
        let relative = entity_relative_path(entity_path, &pointer.path);
        let include = format!("--include={}", relative.to_string_lossy());
        git_output(
            repository,
            [
                "lfs",
                "fetch",
                include.as_str(),
                "--exclude=",
                "origin",
                commit,
            ],
        )?;
    }
    Ok(())
}

fn entity_relative_path(entity_path: &str, pointer_path: &str) -> PathBuf {
    if entity_path == "." {
        PathBuf::from(pointer_path)
    } else {
        Path::new(entity_path).join(pointer_path)
    }
}

fn parse_lfs_pointer(bytes: &[u8]) -> Option<(String, u64)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    if lines.next()? != "version https://git-lfs.github.com/spec/v1" {
        return None;
    }
    let oid = lines.next()?.strip_prefix("oid sha256:")?;
    if oid.len() != 64 || !oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let size = lines.next()?.strip_prefix("size ")?.parse().ok()?;
    if lines.next().is_some() {
        return None;
    }
    Some((oid.to_ascii_lowercase(), size))
}

fn git_output<I, S>(repository: &Path, args: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .context("failed to execute git")?;
    require_success(output, "git command")
}

fn require_success(output: Output, operation: &str) -> Result<Output> {
    if output.status.success() {
        return Ok(output);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!("{operation} failed: {}", stderr.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_strict_lfs_pointer() {
        let pointer = b"version https://git-lfs.github.com/spec/v1\noid sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\nsize 42\n";
        assert_eq!(
            parse_lfs_pointer(pointer),
            Some((
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
                42
            ))
        );
        assert!(parse_lfs_pointer(b"ordinary file").is_none());
    }
}
