use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MANIFEST_NAME: &str = "FORGE.toml";
const LOCK_NAME: &str = "FORGE.lock";
const FORMAT: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u32,
    #[serde(default)]
    entity: Vec<ManifestEntity>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestEntity {
    id: String,
    kind: String,
    schema: String,
    git: String,
    revision: String,
    path: String,
}

#[derive(Debug, Serialize)]
struct LockFile {
    format: u32,
    manifest_sha256: String,
    entity: Vec<LockEntity>,
}

#[derive(Debug, Serialize)]
struct LockEntity {
    id: String,
    git: String,
    commit: String,
    path: String,
    tree: String,
}

#[derive(Debug)]
struct ResolvedTree {
    id: String,
    lfs: Vec<LfsPointer>,
}

#[derive(Debug)]
struct LfsPointer {
    path: String,
    oid: String,
    size: u64,
}

pub fn init(path: Option<&Path>) -> Result<()> {
    let directory = match path {
        Some(path) => path.to_path_buf(),
        None => env::current_dir().context("failed to read the current directory")?,
    };
    ensure!(
        directory.is_dir(),
        "{} is not an existing directory",
        directory.display()
    );

    let manifest_path = directory.join(MANIFEST_NAME);
    if manifest_path.exists() {
        let bytes = fs::read(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        parse_manifest(&bytes, &manifest_path)?;
        println!("validated {}", manifest_path.display());
    } else {
        atomic_write(&manifest_path, b"format = 1\n")?;
        println!("created {}", manifest_path.display());
    }
    Ok(())
}

pub fn lock() -> Result<()> {
    let current = env::current_dir().context("failed to read the current directory")?;
    let manifest_path = find_manifest(&current)?;
    let workspace = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", manifest_path.display()))?;
    let bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let mut manifest = parse_manifest(&bytes, &manifest_path)?;
    let data_root = data_root()?;

    // Stable traversal also makes the serialized lock independent of manifest order.
    manifest.entity.sort_by(|a, b| a.id.cmp(&b.id));
    let mut repositories = BTreeMap::<String, PathBuf>::new();
    let mut locked = Vec::with_capacity(manifest.entity.len());

    for entity in manifest.entity {
        let source_hash = sha256_hex(entity.git.as_bytes());
        let repository = match repositories.get(&entity.git) {
            Some(path) => path.clone(),
            None => {
                let path = ensure_repository(&data_root, &source_hash, &entity.git)?;
                repositories.insert(entity.git.clone(), path.clone());
                path
            }
        };
        let commit = fetch_revision(&repository, &entity.git, &entity.revision)
            .with_context(|| format!("failed to resolve entity {:?}", entity.id))?;
        let tree = resolve_tree(&repository, &commit, &entity.path)
            .with_context(|| format!("invalid tree for entity {:?}", entity.id))?;
        fetch_lfs(&repository, &commit, &entity.path, &tree.lfs)?;
        let checkout = ensure_checkout(&data_root, &source_hash, &commit, &repository)?;
        materialize_lfs(&checkout, &entity.path, &tree.lfs)?;
        let materialized = checkout.join(&entity.path);
        ensure!(
            materialized.is_dir(),
            "materialized entity {:?} is not a directory",
            entity.id
        );

        println!("{}\t{}", entity.id, materialized.display());
        locked.push(LockEntity {
            id: entity.id,
            git: entity.git,
            commit,
            path: entity.path,
            tree: tree.id,
        });
    }

    let lock = LockFile {
        format: FORMAT,
        manifest_sha256: format!("sha256:{}", sha256_hex(&bytes)),
        entity: locked,
    };
    let serialized = toml::to_string_pretty(&lock).context("failed to serialize FORGE.lock")?;
    atomic_write(&workspace.join(LOCK_NAME), serialized.as_bytes())?;
    Ok(())
}

fn parse_manifest(bytes: &[u8], path: &Path) -> Result<Manifest> {
    let source =
        std::str::from_utf8(bytes).with_context(|| format!("{} is not UTF-8", path.display()))?;
    let manifest: Manifest =
        toml::from_str(source).with_context(|| format!("failed to parse {}", path.display()))?;
    ensure!(
        manifest.format == FORMAT,
        "unsupported FORGE.toml format {}",
        manifest.format
    );

    let mut ids = BTreeSet::new();
    for entity in &manifest.entity {
        validate_entity(entity)?;
        ensure!(
            ids.insert(entity.id.as_str()),
            "duplicate entity id {:?}",
            entity.id
        );
    }
    Ok(manifest)
}

fn validate_entity(entity: &ManifestEntity) -> Result<()> {
    for (name, value) in [
        ("id", entity.id.as_str()),
        ("kind", entity.kind.as_str()),
        ("schema", entity.schema.as_str()),
        ("git", entity.git.as_str()),
        ("revision", entity.revision.as_str()),
        ("path", entity.path.as_str()),
    ] {
        ensure!(!value.is_empty(), "entity {} must not be empty", name);
        ensure!(
            !value.contains(['\0', '\n', '\r']),
            "entity {} contains an invalid character",
            name
        );
    }
    ensure!(
        !entity.git.starts_with('-'),
        "entity git must not start with '-'"
    );
    ensure!(
        !entity.revision.starts_with('-'),
        "entity revision must not start with '-'"
    );
    validate_git_source(&entity.git)?;
    validate_relative_path(&entity.path)?;
    Ok(())
}

fn validate_git_source(source: &str) -> Result<()> {
    if Path::new(source).is_absolute() {
        return Ok(());
    }
    if source.contains("://") {
        return Ok(());
    }
    let colon = source.find(':');
    let slash = source.find('/');
    if colon.is_some_and(|colon| slash.is_none_or(|slash| colon < slash)) {
        return Ok(());
    }
    bail!("entity git must be a URL, scp-like SSH source, or absolute local path")
}

fn validate_relative_path(value: &str) -> Result<()> {
    let path = Path::new(value);
    ensure!(!path.is_absolute(), "entity path must be relative");
    let mut saw_normal = false;
    for component in path.components() {
        match component {
            Component::Normal(_) => saw_normal = true,
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("entity path must not contain '..' or a root component")
            }
        }
    }
    ensure!(saw_normal || value == ".", "entity path must not be empty");
    Ok(())
}

fn find_manifest(start: &Path) -> Result<PathBuf> {
    for directory in start.ancestors() {
        let candidate = directory.join(MANIFEST_NAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    bail!(
        "no {} found in {} or its parents",
        MANIFEST_NAME,
        start.display()
    )
}

fn data_root() -> Result<PathBuf> {
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

fn ensure_repository(data_root: &Path, source_hash: &str, source: &str) -> Result<PathBuf> {
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

fn fetch_revision(repository: &Path, source: &str, revision: &str) -> Result<String> {
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

fn resolve_tree(repository: &Path, commit: &str, entity_path: &str) -> Result<ResolvedTree> {
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

fn ensure_checkout(
    data_root: &Path,
    source_hash: &str,
    commit: &str,
    repository: &Path,
) -> Result<PathBuf> {
    let parent = data_root.join("checkouts").join(source_hash);
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

fn materialize_lfs(checkout: &Path, entity_path: &str, pointers: &[LfsPointer]) -> Result<()> {
    if !pointers.is_empty() {
        let output = git_output(checkout, ["lfs", "install", "--local"])?;
        require_success(output, "git lfs install --local")?;
    }
    for pointer in pointers {
        let relative = if entity_path == "." {
            PathBuf::from(&pointer.path)
        } else {
            Path::new(entity_path).join(&pointer.path)
        };
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

fn fetch_lfs(
    repository: &Path,
    commit: &str,
    entity_path: &str,
    pointers: &[LfsPointer],
) -> Result<()> {
    for pointer in pointers {
        let relative = if entity_path == "." {
            PathBuf::from(&pointer.path)
        } else {
            Path::new(entity_path).join(&pointer.path)
        };
        let include = format!("--include={}", relative.to_string_lossy());
        let output = git_output(
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
        require_success(output, "git lfs fetch")?;
    }
    Ok(())
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
    if output.status.success() {
        Ok(output)
    } else {
        require_success(output, "git command")
    }
}

fn require_success(output: Output, operation: &str) -> Result<Output> {
    if output.status.success() {
        return Ok(output);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!("{operation} failed: {}", stderr.trim())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
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

fn sibling_temporary(path: &Path) -> PathBuf {
    let mut name = OsString::from(".");
    name.push(path.file_name().unwrap_or_else(|| OsStr::new("forge")));
    name.push(format!(".tmp-{}", std::process::id()));
    path.with_file_name(name)
}

fn remove_internal_path_if_present(path: &Path, required_parent: &Path) -> Result<()> {
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

fn sha256_hex(bytes: &[u8]) -> String {
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
    fn empty_manifest_is_valid() {
        let manifest = parse_manifest(b"format = 1\n", Path::new("FORGE.toml")).unwrap();
        assert!(manifest.entity.is_empty());
    }

    #[test]
    fn manifest_rejects_unknown_fields() {
        let error =
            parse_manifest(b"format = 1\nextra = true\n", Path::new("FORGE.toml")).unwrap_err();
        assert!(error.to_string().contains("failed to parse"));
    }

    #[test]
    fn manifest_rejects_duplicate_ids() {
        let source = br#"
format = 1

[[entity]]
id = "same"
kind = "one"
schema = "1"
git = "/tmp/repo"
revision = "main"
path = "first"

[[entity]]
id = "same"
kind = "two"
schema = "1"
git = "/tmp/repo"
revision = "main"
path = "second"
"#;
        let error = parse_manifest(source, Path::new("FORGE.toml")).unwrap_err();
        assert!(error.to_string().contains("duplicate entity id"));
    }

    #[test]
    fn path_validation_rejects_parent_components() {
        assert!(validate_relative_path("a/../b").is_err());
        assert!(validate_relative_path("/absolute").is_err());
        assert!(validate_relative_path("entity").is_ok());
        assert!(validate_relative_path(".").is_ok());
    }

    #[test]
    fn lock_serialization_is_stable() {
        let lock = LockFile {
            format: 1,
            manifest_sha256: "sha256:abc".into(),
            entity: vec![LockEntity {
                id: "a".into(),
                git: "/repo".into(),
                commit: "commit".into(),
                path: "entity".into(),
                tree: "tree".into(),
            }],
        };
        assert_eq!(
            toml::to_string_pretty(&lock).unwrap(),
            "format = 1\nmanifest_sha256 = \"sha256:abc\"\n\n[[entity]]\nid = \"a\"\ngit = \"/repo\"\ncommit = \"commit\"\npath = \"entity\"\ntree = \"tree\"\n"
        );
    }

    #[test]
    fn atomic_write_replaces_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("value");
        fs::write(&path, "old").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
    }

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
