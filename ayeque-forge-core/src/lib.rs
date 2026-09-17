use std::collections::BTreeSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MANIFEST_NAME: &str = "FORGE.toml";
const LOCK_NAME: &str = "FORGE.lock";
const PROJECT_MARKER_NAME: &str = "project.toml";
const FORMAT: u32 = 2;

#[derive(Debug, Clone)]
pub struct ProjectLocation {
    project_path: PathBuf,
    project_key: String,
    workspace: PathBuf,
    workspace_sha256: String,
}

impl ProjectLocation {
    pub fn project_path(&self) -> &Path {
        &self.project_path
    }
    pub fn project_key(&self) -> &str {
        &self.project_key
    }
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }
    pub fn workspace_sha256(&self) -> &str {
        &self.workspace_sha256
    }
}

#[derive(Debug, Clone)]
pub struct Manifest {
    format: u32,
    entity: Vec<ManifestEntity>,
}

impl Manifest {
    pub fn format(&self) -> u32 {
        self.format
    }
    pub fn entities(&self) -> &[ManifestEntity] {
        &self.entity
    }
}

#[derive(Debug, Clone)]
pub struct ManifestEntity {
    id: String,
    kind: String,
    schema: String,
    git: String,
    revision: String,
    path: String,
}

impl ManifestEntity {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn kind(&self) -> &str {
        &self.kind
    }
    pub fn schema(&self) -> &str {
        &self.schema
    }
    pub fn git(&self) -> &str {
        &self.git
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn path(&self) -> &str {
        &self.path
    }
}

#[derive(Debug)]
pub struct VerifiedProject {
    storage_root: PathBuf,
    project_path: PathBuf,
    project_key: String,
    workspace: PathBuf,
    workspace_sha256: String,
    manifest_path: PathBuf,
    lock_path: PathBuf,
    entities_path: PathBuf,
    manifest_bytes: Vec<u8>,
    manifest: Manifest,
}

impl VerifiedProject {
    pub fn storage_root(&self) -> &Path {
        &self.storage_root
    }
    pub fn project_path(&self) -> &Path {
        &self.project_path
    }
    pub fn project_key(&self) -> &str {
        &self.project_key
    }
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }
    pub fn workspace_sha256(&self) -> &str {
        &self.workspace_sha256
    }
    pub fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }
    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }
    pub fn entities_path(&self) -> &Path {
        &self.entities_path
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn manifest_bytes(&self) -> &[u8] {
        &self.manifest_bytes
    }

    pub fn manifest_entity(&self, id: &str) -> Result<&ManifestEntity> {
        self.manifest
            .entities()
            .iter()
            .find(|entity| entity.id() == id)
            .ok_or_else(|| anyhow!("entity {:?} is not declared", id))
    }
}

#[derive(Debug, Clone)]
pub struct LockEntry {
    id: String,
    git: String,
    commit: String,
    path: String,
    tree: String,
}

impl LockEntry {
    pub fn new(
        id: String,
        git: String,
        commit: String,
        path: String,
        tree: String,
    ) -> Result<Self> {
        validate_entity_id(&id)?;
        ensure!(!git.is_empty(), "locked entity git must not be empty");
        ensure!(
            !git.contains(['\0', '\n', '\r']),
            "locked entity git contains an invalid character"
        );
        validate_object_id(&commit, "commit")?;
        validate_relative_path(&path)?;
        validate_object_id(&tree, "tree")?;
        Ok(Self {
            id,
            git,
            commit,
            path,
            tree,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn git(&self) -> &str {
        &self.git
    }
    pub fn commit(&self) -> &str {
        &self.commit
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn tree(&self) -> &str {
        &self.tree
    }
}

#[derive(Debug)]
pub struct VerifiedLock {
    manifest_sha256: String,
    project_key: String,
    workspace_sha256: String,
    entities: Vec<LockEntry>,
}

impl VerifiedLock {
    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }
    pub fn project_key(&self) -> &str {
        &self.project_key
    }
    pub fn workspace_sha256(&self) -> &str {
        &self.workspace_sha256
    }
    pub fn entities(&self) -> &[LockEntry] {
        &self.entities
    }
}

#[derive(Debug)]
pub struct VerifiedEntity {
    id: String,
    kind: String,
    schema: String,
    git: String,
    commit: String,
    source_path: String,
    tree: String,
    materialized_path: PathBuf,
}

impl VerifiedEntity {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn kind(&self) -> &str {
        &self.kind
    }
    pub fn schema(&self) -> &str {
        &self.schema
    }
    pub fn git(&self) -> &str {
        &self.git
    }
    pub fn commit(&self) -> &str {
        &self.commit
    }
    pub fn source_path(&self) -> &str {
        &self.source_path
    }
    pub fn tree(&self) -> &str {
        &self.tree
    }
    pub fn materialized_path(&self) -> &Path {
        &self.materialized_path
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    format: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    entity: Vec<ManifestEntityFile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ManifestEntityFile {
    id: String,
    kind: String,
    schema: String,
    git: String,
    revision: String,
    #[serde(
        default = "default_entity_path",
        skip_serializing_if = "is_current_path"
    )]
    path: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LockFile {
    format: u32,
    manifest_sha256: String,
    project_key: String,
    workspace_sha256: String,
    entity: Vec<LockEntryFile>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LockEntryFile {
    id: String,
    git: String,
    commit: String,
    path: String,
    tree: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectMarker {
    format: u32,
    workspace_sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GlobalConfig {
    format: u32,
    storage_root: String,
}

pub fn config_path() -> Result<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| anyhow!("neither XDG_CONFIG_HOME nor HOME is set"))?;
    ensure!(
        base.is_absolute(),
        "XDG_CONFIG_HOME or HOME must be absolute"
    );
    Ok(base.join("ayeque-forge/config.toml"))
}

pub fn data_root() -> Result<PathBuf> {
    let config = config_path()?;
    if config.is_file() {
        let bytes = fs::read(&config)?;
        let config: GlobalConfig =
            toml::from_slice(&bytes).context("invalid ayeque-forge config")?;
        ensure!(config.format == 1, "unsupported ayeque-forge config format");
        let root = PathBuf::from(config.storage_root);
        ensure!(
            root.is_absolute(),
            "configured storage_root must be absolute"
        );
        return Ok(root);
    }
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

pub fn write_storage_root(root: &Path) -> Result<PathBuf> {
    ensure!(root.is_absolute(), "storage root override must be absolute");
    let path = config_path()?;
    let config = GlobalConfig {
        format: 1,
        storage_root: root.to_string_lossy().into_owned(),
    };
    let bytes = toml::to_string_pretty(&config).context("failed to serialize config")?;
    atomic_write(&path, bytes.as_bytes())?;
    Ok(path)
}

pub fn ensure_layout(root: &Path) -> Result<()> {
    for directory in [root, &root.join("git"), &root.join("projects")] {
        fs::create_dir_all(directory)
            .with_context(|| format!("failed to create {}", directory.display()))?;
    }
    Ok(())
}

pub fn discover_workspace(start: &Path) -> Result<PathBuf> {
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

pub fn workspace_identity(workspace: &Path) -> String {
    format!("sha256:{}", sha256_hex(&path_bytes(workspace)))
}

pub fn initialize_project(storage_root: &Path, start: &Path) -> Result<ProjectLocation> {
    ensure!(storage_root.is_absolute(), "storage root must be absolute");
    let workspace = discover_workspace(start)?;
    ensure!(
        workspace.is_dir(),
        "{} is not an existing directory",
        workspace.display()
    );
    ensure_layout(storage_root)?;
    let basename = project_basename(&workspace)?;
    let identity = workspace_identity(&workspace);
    for key in [
        basename.clone(),
        format!("{}-{}", basename, &identity[7..19]),
        format!("{}-{}", basename, &identity[7..]),
    ] {
        let project = storage_root.join("projects").join(&key);
        let marker = project.join(PROJECT_MARKER_NAME);
        if project.exists() {
            if marker.is_file() && marker_matches(&marker, &identity)? {
                fs::create_dir_all(project.join("entities"))?;
                return Ok(ProjectLocation {
                    project_path: project,
                    project_key: key,
                    workspace: workspace.clone(),
                    workspace_sha256: identity.clone(),
                });
            }
            continue;
        }
        fs::create_dir_all(project.join("entities"))?;
        atomic_write(
            &marker,
            format!("format = 1\nworkspace_sha256 = \"{}\"\n", identity).as_bytes(),
        )?;
        return Ok(ProjectLocation {
            project_path: project,
            project_key: key,
            workspace: workspace.clone(),
            workspace_sha256: identity.clone(),
        });
    }
    bail!("could not allocate a unique Forge project key")
}

pub fn resolve_project(workspace: &Path) -> Result<VerifiedProject> {
    resolve_project_at(&data_root()?, workspace)
}

pub fn resolve_project_at(storage_root: &Path, start: &Path) -> Result<VerifiedProject> {
    ensure!(storage_root.is_absolute(), "storage root must be absolute");
    let workspace = discover_workspace(start)?;
    let basename = project_basename(&workspace)?;
    let identity = workspace_identity(&workspace);
    for key in [
        basename.clone(),
        format!("{}-{}", basename, &identity[7..19]),
        format!("{}-{}", basename, &identity[7..]),
    ] {
        let project_path = storage_root.join("projects").join(&key);
        let marker_path = project_path.join(PROJECT_MARKER_NAME);
        if !marker_path.is_file() {
            continue;
        }
        if !marker_matches(&marker_path, &identity)? {
            continue;
        }
        let manifest_path = project_path.join(MANIFEST_NAME);
        let manifest_bytes = fs::read(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let manifest = parse_manifest(&manifest_bytes, &manifest_path)?;
        return Ok(VerifiedProject {
            storage_root: storage_root.to_owned(),
            project_path: project_path.clone(),
            project_key: key,
            workspace,
            workspace_sha256: identity,
            manifest_path,
            lock_path: project_path.join(LOCK_NAME),
            entities_path: project_path.join("entities"),
            manifest_bytes,
            manifest,
        });
    }
    bail!("workspace is not initialized; run `ayeque-forge init`")
}

pub fn parse_manifest(bytes: &[u8], path: &Path) -> Result<Manifest> {
    let source =
        std::str::from_utf8(bytes).with_context(|| format!("{} is not UTF-8", path.display()))?;
    let manifest: ManifestFile =
        toml::from_str(source).with_context(|| format!("failed to parse {}", path.display()))?;
    ensure!(
        manifest.format == FORMAT,
        "unsupported FORGE.toml format {}",
        manifest.format
    );
    let mut ids = BTreeSet::new();
    let mut entities = Vec::with_capacity(manifest.entity.len());
    for entity in manifest.entity {
        validate_manifest_entity(&entity)?;
        ensure!(
            ids.insert(entity.id.clone()),
            "duplicate entity id {:?}",
            entity.id
        );
        entities.push(ManifestEntity {
            id: entity.id,
            kind: entity.kind,
            schema: entity.schema,
            git: entity.git,
            revision: entity.revision,
            path: entity.path,
        });
    }
    Ok(Manifest {
        format: manifest.format,
        entity: entities,
    })
}

pub fn verify_lock(project: &VerifiedProject) -> Result<VerifiedLock> {
    ensure!(
        marker_matches(
            &project.project_path.join(PROJECT_MARKER_NAME),
            &project.workspace_sha256
        )?,
        "{} belongs to another workspace",
        project.project_path.display()
    );
    let current_manifest = fs::read(&project.manifest_path)
        .with_context(|| format!("failed to read {}", project.manifest_path.display()))?;
    ensure!(
        current_manifest == project.manifest_bytes,
        "{} is stale; run `ayeque-forge lock`",
        project.manifest_path.display()
    );
    let bytes = fs::read(&project.lock_path).with_context(|| {
        format!(
            "failed to read {}; run `ayeque-forge init` first",
            project.lock_path.display()
        )
    })?;
    let source = std::str::from_utf8(&bytes)
        .with_context(|| format!("{} is not UTF-8", project.lock_path.display()))?;
    let lock: LockFile = toml::from_str(source)
        .with_context(|| format!("failed to parse {}", project.lock_path.display()))?;
    ensure!(
        lock.format == FORMAT,
        "unsupported FORGE.lock format {}",
        lock.format
    );
    ensure!(
        lock.project_key == project.project_key,
        "{} belongs to another project",
        project.lock_path.display()
    );
    ensure!(
        lock.workspace_sha256 == project.workspace_sha256,
        "{} belongs to another workspace",
        project.lock_path.display()
    );
    let expected_digest = format!("sha256:{}", sha256_hex(&project.manifest_bytes));
    ensure!(
        lock.manifest_sha256 == expected_digest,
        "{} is stale; run `ayeque-forge lock`",
        project.lock_path.display()
    );

    let mut ids = BTreeSet::new();
    let mut entities = Vec::with_capacity(lock.entity.len());
    for entity in lock.entity {
        validate_entity_id(&entity.id)?;
        ensure!(
            ids.insert(entity.id.clone()),
            "duplicate locked entity id {:?}",
            entity.id
        );
        let entry = LockEntry::new(
            entity.id,
            entity.git,
            entity.commit,
            entity.path,
            entity.tree,
        )?;
        let declared = project.manifest_entity(entry.id()).with_context(|| {
            format!(
                "{} contains undeclared entity {:?}; run `ayeque-forge lock`",
                project.lock_path.display(),
                entry.id()
            )
        })?;
        ensure!(
            entry.git() == declared.git() && entry.path() == declared.path(),
            "locked entity {:?} does not match {}; run `ayeque-forge lock`",
            entry.id(),
            project.manifest_path.display()
        );
        entities.push(entry);
    }
    ensure!(
        entities.len() == project.manifest.entities().len(),
        "{} does not match {}; run `ayeque-forge lock`",
        project.lock_path.display(),
        project.manifest_path.display()
    );
    ensure!(
        project.entities_path.is_dir(),
        "{} is not a materialized entities directory; run `ayeque-forge lock`",
        project.entities_path.display()
    );
    for entity in &entities {
        let path = project.entities_path.join(entity.id());
        ensure!(
            path.is_dir(),
            "entity {:?} is not materialized; run `ayeque-forge lock`",
            entity.id()
        );
    }
    for entry in fs::read_dir(&project.entities_path)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow!("materialized entity has a non-UTF-8 name"))?;
        ensure!(
            ids.contains(&name),
            "materialized entity {:?} is not declared; run `ayeque-forge lock`",
            name
        );
        ensure!(
            entry.file_type()?.is_dir(),
            "materialized entity {:?} is not a directory; run `ayeque-forge lock`",
            name
        );
    }
    Ok(VerifiedLock {
        manifest_sha256: lock.manifest_sha256,
        project_key: lock.project_key,
        workspace_sha256: lock.workspace_sha256,
        entities,
    })
}

pub fn resolve_entity(project: &VerifiedProject, entity_id: &str) -> Result<VerifiedEntity> {
    let lock = verify_lock(project)?;
    let locked = lock
        .entities
        .iter()
        .find(|entity| entity.id() == entity_id)
        .ok_or_else(|| {
            anyhow!(
                "entity {:?} is not present in {}",
                entity_id,
                project.lock_path.display()
            )
        })?;
    let declared = project.manifest_entity(entity_id)?;
    let materialized_path = project.entities_path.join(entity_id);
    ensure!(
        materialized_path.is_dir(),
        "entity {:?} is not materialized; run `ayeque-forge lock`",
        entity_id
    );
    Ok(VerifiedEntity {
        id: declared.id.clone(),
        kind: declared.kind.clone(),
        schema: declared.schema.clone(),
        git: locked.git.clone(),
        commit: locked.commit.clone(),
        source_path: locked.path.clone(),
        tree: locked.tree.clone(),
        materialized_path,
    })
}

pub fn serialize_lock(project: &VerifiedProject, entities: Vec<LockEntry>) -> Result<Vec<u8>> {
    let mut ids = BTreeSet::new();
    for entity in &entities {
        ensure!(
            ids.insert(entity.id()),
            "duplicate locked entity id {:?}",
            entity.id()
        );
        let declared = project.manifest_entity(entity.id())?;
        ensure!(
            entity.git() == declared.git() && entity.path() == declared.path(),
            "locked entity {:?} does not match {}",
            entity.id(),
            project.manifest_path.display()
        );
    }
    ensure!(
        entities.len() == project.manifest.entities().len(),
        "lock entity count does not match {}",
        project.manifest_path.display()
    );
    let lock = LockFile {
        format: FORMAT,
        manifest_sha256: format!("sha256:{}", sha256_hex(&project.manifest_bytes)),
        project_key: project.project_key.clone(),
        workspace_sha256: project.workspace_sha256.clone(),
        entity: entities
            .into_iter()
            .map(|entity| LockEntryFile {
                id: entity.id,
                git: entity.git,
                commit: entity.commit,
                path: entity.path,
                tree: entity.tree,
            })
            .collect(),
    };
    Ok(toml::to_string_pretty(&lock)
        .context("failed to serialize FORGE.lock")?
        .into_bytes())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

pub fn validate_relative_path(value: &str) -> Result<()> {
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

fn validate_manifest_entity(entity: &ManifestEntityFile) -> Result<()> {
    validate_entity_id(&entity.id)?;
    for (name, value) in [
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
    validate_relative_path(&entity.path)
}

fn validate_entity_id(value: &str) -> Result<()> {
    ensure!(!value.is_empty(), "entity id must not be empty");
    ensure!(
        !value.contains(['/', '\\', '\0', '\n', '\r']),
        "entity id must be a safe path component"
    );
    ensure!(
        value != "." && value != "..",
        "entity id must be a safe path component"
    );
    ensure!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':')),
        "entity id contains an invalid character"
    );
    Ok(())
}

fn validate_git_source(source: &str) -> Result<()> {
    if Path::new(source).is_absolute() || source.contains("://") {
        return Ok(());
    }
    let colon = source.find(':');
    let slash = source.find('/');
    if colon.is_some_and(|colon| slash.is_none_or(|slash| colon < slash)) {
        return Ok(());
    }
    bail!("entity git must be a URL, scp-like SSH source, or absolute local path")
}

fn validate_object_id(value: &str, name: &str) -> Result<()> {
    ensure!(
        matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "locked entity {name} is not a Git object id"
    );
    Ok(())
}

fn default_entity_path() -> String {
    ".".into()
}
fn is_current_path(path: &str) -> bool {
    path == "."
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

fn marker_matches(path: &Path, identity: &str) -> Result<bool> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let source =
        std::str::from_utf8(&bytes).with_context(|| format!("{} is not UTF-8", path.display()))?;
    let marker: ProjectMarker =
        toml::from_str(source).with_context(|| format!("failed to parse {}", path.display()))?;
    ensure!(
        marker.format == 1,
        "unsupported project marker format {}",
        marker.format
    );
    Ok(marker.workspace_sha256 == identity)
}

fn path_bytes(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().as_bytes().to_vec()
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent", path.display()))?;
    fs::create_dir_all(parent)?;
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
        fs::File::open(parent)
            .with_context(|| format!("failed to open {} for sync", parent.display()))?
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn manifest_parser_is_strict_and_rejects_unsafe_ids() {
        assert!(parse_manifest(b"format = 2\nextra = true\n", Path::new("FORGE.toml")).is_err());
        assert!(parse_manifest(b"format = 2\n[[entity]]\nid = \"../escape\"\nkind = \"agent\"\nschema = \"1\"\ngit = \"/repo\"\nrevision = \"main\"\n", Path::new("FORGE.toml")).is_err());
        assert!(
            LockEntry::new(
                "../escape".into(),
                "/repo".into(),
                "0123456789012345678901234567890123456789".into(),
                ".".into(),
                "abcdefabcdefabcdefabcdefabcdefabcdefabcd".into()
            )
            .is_err()
        );
    }

    #[test]
    fn public_api_rejects_stale_lock_and_materialized_entity() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let storage = temp.path().join("data");
        fs::create_dir(&workspace).unwrap();
        let location = initialize_project(&storage, &workspace).unwrap();
        fs::write(location.project_path().join(MANIFEST_NAME), "format = 2\n[[entity]]\nid = \"agent\"\nkind = \"agent\"\nschema = \"1\"\ngit = \"/repo\"\nrevision = \"main\"\n").unwrap();
        let project = resolve_project_at(&storage, &workspace).unwrap();
        let manifest_bytes = fs::read(project.manifest_path()).unwrap();
        let entry = LockEntry::new(
            "agent".into(),
            "/repo".into(),
            "0123456789012345678901234567890123456789".into(),
            ".".into(),
            "abcdefabcdefabcdefabcdefabcdefabcdefabcd".into(),
        )
        .unwrap();
        fs::create_dir_all(project.entities_path().join("agent")).unwrap();
        fs::write(
            project.lock_path(),
            serialize_lock(&project, vec![entry]).unwrap(),
        )
        .unwrap();
        assert_eq!(resolve_entity(&project, "agent").unwrap().id(), "agent");
        fs::write(project.manifest_path(), "format = 2\n").unwrap();
        assert!(verify_lock(&project).is_err());
        fs::write(project.manifest_path(), manifest_bytes).unwrap();
        fs::write(
            project.project_path().join(PROJECT_MARKER_NAME),
            "format = 1\nworkspace_sha256 = \"sha256:stale\"\n",
        )
        .unwrap();
        assert!(verify_lock(&project).is_err());
        fs::write(
            project.project_path().join(PROJECT_MARKER_NAME),
            format!(
                "format = 1\nworkspace_sha256 = \"{}\"\n",
                workspace_identity(&workspace)
            ),
        )
        .unwrap();
        fs::remove_dir_all(project.entities_path().join("agent")).unwrap();
        assert!(verify_lock(&project).is_err());
        fs::write(project.lock_path(), "format = 2\nmanifest_sha256 = \"sha256:stale\"\nproject_key = \"workspace\"\nworkspace_sha256 = \"sha256:stale\"\n").unwrap();
        assert!(verify_lock(&project).is_err());
    }
}
