use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::git::{
    ensure_checkout, ensure_repository, fetch_lfs, fetch_revision, materialize_lfs, resolve_tree,
};
use crate::manifest::{Manifest, parse_manifest, validate_relative_path};
use crate::storage::{
    atomic_write, data_root, ensure_layout, remove_internal_path_if_present, sha256_hex,
    write_storage_root,
};

const MANIFEST_NAME: &str = "FORGE.toml";
const LOCK_NAME: &str = "FORGE.lock";
const FORMAT: u32 = 2;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LockFile {
    format: u32,
    manifest_sha256: String,
    project_key: String,
    workspace_sha256: String,
    entity: Vec<LockEntity>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LockEntity {
    id: String,
    git: String,
    commit: String,
    path: String,
    tree: String,
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

    let directory = directory
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", directory.display()))?;
    let storage = data_root()?;
    ensure_layout(&storage)?;
    let (project, project_key, workspace_sha) = ensure_project(&storage, &directory)?;
    let manifest_path = project.join(MANIFEST_NAME);
    if !manifest_path.exists() {
        atomic_write(&manifest_path, format!("format = {FORMAT}\n").as_bytes())?;
        println!("created {}", manifest_path.display());
    } else {
        let bytes = fs::read(&manifest_path)?;
        parse_manifest(&bytes, &manifest_path)?;
        println!("already initialized {}", manifest_path.display());
    }
    let manifest_bytes = fs::read(&manifest_path)?;
    let lock_path = project.join(LOCK_NAME);
    if !lock_path.exists() {
        let lock = LockFile {
            format: FORMAT,
            manifest_sha256: format!("sha256:{}", sha256_hex(&manifest_bytes)),
            project_key,
            workspace_sha256: workspace_sha,
            entity: Vec::new(),
        };
        write_lock(&lock_path, &lock)?;
        println!("created {}", lock_path.display());
    } else {
        println!("preserved {}", lock_path.display());
    }
    println!("project {}", project.display());
    Ok(())
}

pub fn validate() -> Result<()> {
    let (manifest_path, workspace, manifest) = current_manifest()?;
    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let lock_path = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", manifest_path.display()))?
        .join(LOCK_NAME);
    let lock_bytes = fs::read(&lock_path).with_context(|| {
        format!(
            "failed to read {}; run `ayeque-forge init` first",
            lock_path.display()
        )
    })?;
    let lock = parse_lock(&lock_bytes, &lock_path)?;
    validate_lock_matches(
        &manifest,
        &manifest_bytes,
        &lock,
        &lock_path,
        &manifest_path,
        &workspace,
    )?;
    println!("validated {}", manifest_path.display());
    println!("validated {}", lock_path.display());
    Ok(())
}

pub fn configure_storage_root(root: &Path) -> Result<()> {
    ensure!(root.is_absolute(), "storage root override must be absolute");
    let _ = write_storage_root(root)?;
    let data_root = root.to_path_buf();
    ensure_layout(&data_root)?;
    println!("configured storage {}", data_root.display());
    Ok(())
}

fn current_manifest() -> Result<(PathBuf, PathBuf, Manifest)> {
    let workspace = discover_workspace(&env::current_dir()?)?;
    let storage = data_root()?;
    let (project, _, _) = existing_project(&storage, &workspace)?;
    let manifest_path = project.join(MANIFEST_NAME);
    let bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest = parse_manifest(&bytes, &manifest_path)?;
    Ok((manifest_path, workspace, manifest))
}

fn discover_workspace(start: &Path) -> Result<PathBuf> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(start)
        .args(["rev-parse", "--show-toplevel"])
        .output();
    if let Ok(output) = output
        && output.status.success()
    {
        let value = String::from_utf8(output.stdout).context("git returned non-UTF-8 workspace")?;
        return PathBuf::from(value.trim())
            .canonicalize()
            .context("failed to resolve Git workspace");
    }
    start
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", start.display()))
}

fn workspace_identity(workspace: &Path) -> String {
    format!(
        "sha256:{}",
        sha256_hex(workspace.to_string_lossy().as_bytes())
    )
}

fn project_basename(workspace: &Path) -> Result<String> {
    workspace
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .filter(|value| {
            !value.is_empty()
                && value != "."
                && value != ".."
                && !value.contains(['/', '\0', '\n', '\r'])
        })
        .ok_or_else(|| anyhow!("workspace has no valid basename"))
}

fn ensure_project(storage: &Path, workspace: &Path) -> Result<(PathBuf, String, String)> {
    fs::create_dir_all(storage.join("projects"))?;
    let basename = project_basename(workspace)?;
    let identity = workspace_identity(workspace);
    let base = storage.join("projects").join(&basename);
    for key in [
        basename.clone(),
        format!("{}-{}", basename, &identity[7..19]),
        format!("{}-{}", basename, &identity[7..]),
    ] {
        let project = storage.join("projects").join(&key);
        let marker = project.join("project.toml");
        if project.exists() {
            if marker.is_file() && fs::read_to_string(&marker)?.contains(&identity) {
                fs::create_dir_all(project.join("entities"))?;
                return Ok((project, key, identity));
            }
            continue;
        }
        fs::create_dir_all(project.join("entities"))?;
        atomic_write(
            &marker,
            format!("format = 1\nworkspace_sha256 = \"{}\"\n", identity).as_bytes(),
        )?;
        return Ok((project, key, identity));
    }
    let _ = base;
    bail!("could not allocate a unique Forge project key")
}

fn existing_project(storage: &Path, workspace: &Path) -> Result<(PathBuf, String, String)> {
    let basename = project_basename(workspace)?;
    let identity = workspace_identity(workspace);
    for key in [
        basename.clone(),
        format!("{}-{}", basename, &identity[7..19]),
        format!("{}-{}", basename, &identity[7..]),
    ] {
        let project = storage.join("projects").join(&key);
        if project.join("project.toml").is_file()
            && fs::read_to_string(project.join("project.toml"))?.contains(&identity)
        {
            return Ok((project, key, identity));
        }
    }
    bail!("workspace is not initialized; run `ayeque-forge init`")
}

pub fn lock() -> Result<()> {
    let workspace = discover_workspace(&env::current_dir()?)?;
    let data_root = data_root()?;
    let (project, project_key, workspace_sha) = existing_project(&data_root, &workspace)?;
    let manifest_path = project.join(MANIFEST_NAME);
    let bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let mut manifest = parse_manifest(&bytes, &manifest_path)?;
    ensure_layout(&data_root)?;

    manifest.entity.sort_by(|a, b| a.id.cmp(&b.id));
    let mut repositories = BTreeMap::<String, PathBuf>::new();
    let mut locked = Vec::with_capacity(manifest.entity.len());
    let entities = project.join("entities");
    let staged_entities = project.join(format!(".entities.staging-{}", std::process::id()));
    remove_internal_path_if_present(&staged_entities, &project)?;
    fs::create_dir_all(&staged_entities)?;

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
        let checkout_entity = checkout.join(&entity.path);
        ensure!(
            checkout_entity.is_dir(),
            "materialized entity {:?} is not a directory",
            entity.id
        );
        let materialized = staged_entities.join(&entity.id);
        fs::create_dir_all(&materialized)?;
        copy_tree(&checkout_entity, &materialized)?;
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
        project_key,
        workspace_sha256: workspace_sha,
        entity: locked,
    };
    let serialized = toml::to_string_pretty(&lock).context("failed to serialize FORGE.lock")?;
    let old_entities = project.join(format!(".entities.previous-{}", std::process::id()));
    remove_internal_path_if_present(&old_entities, &project)?;
    if entities.exists() {
        fs::rename(&entities, &old_entities)?;
    }
    if let Err(error) = fs::rename(&staged_entities, &entities) {
        if old_entities.exists() {
            let _ = fs::rename(&old_entities, &entities);
        }
        return Err(error.into());
    }
    if old_entities.exists() {
        fs::remove_dir_all(&old_entities)?;
    }
    atomic_write(&project.join(LOCK_NAME), serialized.as_bytes())?;
    for entity in &lock.entity {
        println!("{}\t{}", entity.id, entities.join(&entity.id).display());
    }
    Ok(())
}

pub fn path(id: &str) -> Result<()> {
    ensure!(!id.is_empty(), "entity id must not be empty");
    let (manifest_path, workspace, manifest) = current_manifest()?;
    let manifest_bytes = fs::read(&manifest_path)?;

    let lock_path = manifest_path.parent().unwrap().join(LOCK_NAME);
    let lock_bytes = fs::read(&lock_path).with_context(|| {
        format!(
            "failed to read {}; run `ayeque-forge lock` first",
            lock_path.display()
        )
    })?;
    let lock = parse_lock(&lock_bytes, &lock_path)?;
    let expected_digest = format!("sha256:{}", sha256_hex(&manifest_bytes));
    ensure!(
        lock.manifest_sha256 == expected_digest,
        "{} is stale; run `ayeque-forge lock`",
        lock_path.display()
    );
    ensure!(
        lock.entity.len() == manifest.entity.len(),
        "{} does not match {}; run `ayeque-forge lock`",
        lock_path.display(),
        manifest_path.display()
    );

    for locked in &lock.entity {
        let declared = manifest
            .entity
            .iter()
            .find(|entity| entity.id == locked.id)
            .ok_or_else(|| {
                anyhow!(
                    "{} contains undeclared entity {:?}; run `ayeque-forge lock`",
                    lock_path.display(),
                    locked.id
                )
            })?;
        ensure!(
            locked.git == declared.git && locked.path == declared.path,
            "locked entity {:?} does not match {}; run `ayeque-forge lock`",
            locked.id,
            manifest_path.display()
        );
    }

    let entity = lock
        .entity
        .iter()
        .find(|entity| entity.id == id)
        .ok_or_else(|| anyhow!("entity {:?} is not present in {}", id, lock_path.display()))?;
    let data_root = data_root()?;
    let materialized = resolved_entity_path(&workspace, &data_root, &manifest, entity)?;
    println!("{}", materialized.display());
    Ok(())
}

pub fn paths() -> Result<()> {
    paths_xdg()
    /*
    let current = env::current_dir().context("failed to read the current directory")?;
    let manifest_path = find_manifest(&current)?;
    let workspace = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", manifest_path.display()))?;
    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest = parse_manifest(&manifest_bytes, &manifest_path)?;
    let data_root = data_root_for(workspace, &manifest)?;

    println!("workspace	{}", workspace.display());
    println!("manifest	{}", manifest_path.display());
    println!("lock	{}", workspace.join(LOCK_NAME).display());
    println!("storage	{}", data_root.display());
    println!("git-cache	{}", data_root.join("git").display());
    println!("checkouts	{}", data_root.join("checkouts").display());

    let lock_path = workspace.join(LOCK_NAME);
    if !lock_path.is_file() {
        return Ok(());
    }
    let lock_bytes =
        fs::read(&lock_path).with_context(|| format!("failed to read {}", lock_path.display()))?;
    let lock = parse_lock(&lock_bytes, &lock_path)?;
    let expected_digest = format!("sha256:{}", sha256_hex(&manifest_bytes));
    ensure!(
        lock.manifest_sha256 == expected_digest,
        "{} is stale; run ayeque-forge lock",
        lock_path.display()
    );
    for entity in &lock.entity {
        let materialized = resolved_entity_path(workspace, &data_root, &manifest, entity)?;
        println!("entity.{}	{}", entity.id, materialized.display());
    }
    Ok(()) */
}

fn paths_xdg() -> Result<()> {
    let (manifest_path, workspace, manifest) = current_manifest()?;
    let manifest_bytes = fs::read(&manifest_path)?;
    let data_root = data_root()?;
    let project = manifest_path.parent().unwrap();
    println!("workspace\t{}", workspace.display());
    println!("project\t{}", project.display());
    println!("manifest\t{}", manifest_path.display());
    println!("lock\t{}", project.join(LOCK_NAME).display());
    println!("storage\t{}", data_root.display());
    println!("git-cache\t{}", data_root.join("git").display());
    println!("projects\t{}", data_root.join("projects").display());
    println!("entities\t{}", project.join("entities").display());
    let lock_path = project.join(LOCK_NAME);
    let lock_bytes = fs::read(&lock_path)?;
    let lock = parse_lock(&lock_bytes, &lock_path)?;
    ensure!(
        lock.manifest_sha256 == format!("sha256:{}", sha256_hex(&manifest_bytes)),
        "{} is stale; run ayeque-forge lock",
        lock_path.display()
    );
    for entity in &lock.entity {
        let materialized = data_root
            .join("projects")
            .join(&lock.project_key)
            .join("entities")
            .join(&entity.id);
        ensure!(
            materialized.is_dir(),
            "locked entity {:?} is not materialized",
            entity.id
        );
        println!("entity.{}\t{}", entity.id, materialized.display());
    }
    let _ = manifest;
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    for entry in
        fs::read_dir(source).with_context(|| format!("failed to read {}", source.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read {}", source.display()))?;
        let source_path = entry.path();
        if source_path.file_name().is_some_and(|name| name == ".git") {
            continue;
        }
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", source_path.display()))?;
        if file_type.is_dir() {
            fs::create_dir_all(&destination_path)
                .with_context(|| format!("failed to create {}", destination_path.display()))?;
            copy_tree(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn resolved_entity_path(
    workspace: &Path,
    data_root: &Path,
    manifest: &Manifest,
    entity: &LockEntity,
) -> Result<PathBuf> {
    let declared = manifest
        .entity
        .iter()
        .find(|declared| declared.id == entity.id)
        .ok_or_else(|| anyhow!("entity {:?} is not declared", entity.id))?;
    let project_key = lock_project_key(data_root, workspace)?;
    let path = data_root
        .join("projects")
        .join(project_key)
        .join("entities")
        .join(&declared.id);
    ensure!(
        path.is_dir(),
        "entity {:?} is not materialized; run ayeque-forge lock",
        entity.id
    );
    Ok(path)
}

fn lock_project_key(data_root: &Path, workspace: &Path) -> Result<String> {
    let (project, key, _) = existing_project(data_root, workspace)?;
    let _ = project;
    Ok(key)
}

fn write_lock(path: &Path, lock: &LockFile) -> Result<()> {
    let serialized = toml::to_string_pretty(lock).context("failed to serialize FORGE.lock")?;
    atomic_write(path, serialized.as_bytes())
}

fn validate_lock_matches(
    manifest: &Manifest,
    manifest_bytes: &[u8],
    lock: &LockFile,
    lock_path: &Path,
    manifest_path: &Path,
    workspace: &Path,
) -> Result<()> {
    let basename = project_basename(workspace)?;
    ensure!(
        lock.project_key == basename || lock.project_key.starts_with(&format!("{}-", basename)),
        "{} belongs to another project",
        lock_path.display()
    );
    ensure!(
        lock.workspace_sha256 == workspace_identity(workspace),
        "{} belongs to another workspace",
        lock_path.display()
    );
    let expected_digest = format!("sha256:{}", sha256_hex(manifest_bytes));
    ensure!(
        lock.manifest_sha256 == expected_digest,
        "{} is stale; run `ayeque-forge lock`",
        lock_path.display()
    );
    ensure!(
        lock.entity.len() == manifest.entity.len(),
        "{} does not match {}; run `ayeque-forge lock`",
        lock_path.display(),
        manifest_path.display()
    );
    for locked in &lock.entity {
        let declared = manifest
            .entity
            .iter()
            .find(|entity| entity.id == locked.id)
            .ok_or_else(|| {
                anyhow!(
                    "{} contains undeclared entity {:?}; run `ayeque-forge lock`",
                    lock_path.display(),
                    locked.id
                )
            })?;
        ensure!(
            locked.git == declared.git && locked.path == declared.path,
            "locked entity {:?} does not match {}; run `ayeque-forge lock`",
            locked.id,
            manifest_path.display()
        );
    }
    Ok(())
}

fn parse_lock(bytes: &[u8], path: &Path) -> Result<LockFile> {
    let source =
        std::str::from_utf8(bytes).with_context(|| format!("{} is not UTF-8", path.display()))?;
    let lock: LockFile =
        toml::from_str(source).with_context(|| format!("failed to parse {}", path.display()))?;
    ensure!(
        lock.format == FORMAT,
        "unsupported FORGE.lock format {}",
        lock.format
    );
    let mut ids = std::collections::BTreeSet::new();
    for entity in &lock.entity {
        ensure!(!entity.id.is_empty(), "locked entity id must not be empty");
        ensure!(
            ids.insert(entity.id.as_str()),
            "duplicate locked entity id {:?}",
            entity.id
        );
        validate_object_id(&entity.commit, "commit")?;
        validate_object_id(&entity.tree, "tree")?;
        validate_relative_path(&entity.path)?;
    }
    Ok(lock)
}

fn validate_object_id(value: &str, name: &str) -> Result<()> {
    ensure!(
        matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "locked entity {name} is not a Git object id"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_serialization_is_stable() {
        let lock = LockFile {
            format: 2,
            manifest_sha256: "sha256:abc".into(),
            project_key: "project".into(),
            workspace_sha256: "sha256:workspace".into(),
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
            "format = 2\nmanifest_sha256 = \"sha256:abc\"\nproject_key = \"project\"\nworkspace_sha256 = \"sha256:workspace\"\n\n[[entity]]\nid = \"a\"\ngit = \"/repo\"\ncommit = \"commit\"\npath = \"entity\"\ntree = \"tree\"\n"
        );
    }
}
