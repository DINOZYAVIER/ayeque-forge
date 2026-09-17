use std::fs;
use std::process::Command;

use ayeque_forge_core::{LockEntry, resolve_entity, resolve_project_at, serialize_lock};

fn forge() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ayeque-forge"))
}

#[test]
fn init_keeps_workspace_empty_and_creates_xdg_project() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let output = forge()
        .args(["init", temp.path().to_str().unwrap()])
        .env("XDG_DATA_HOME", &data)
        .env("HOME", temp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!temp.path().join("FORGE.toml").exists());
    let project = data
        .join("ayeque-forge/projects")
        .join(temp.path().file_name().unwrap());
    assert!(project.join("FORGE.toml").is_file());
    assert!(project.join("FORGE.lock").is_file());
    assert!(project.join("project.toml").is_file());
    assert_eq!(
        fs::read_to_string(project.join("FORGE.toml")).unwrap(),
        "format = 2\n"
    );
}

#[test]
fn config_storage_root_is_global_and_absolute() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("forge");
    let output = forge()
        .args(["config", "storage-root", root.to_str().unwrap()])
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .env("HOME", temp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.join("git").is_dir());
    assert!(root.join("projects").is_dir());
    assert!(
        temp.path()
            .join("config/ayeque-forge/config.toml")
            .is_file()
    );
}

#[test]
fn cli_and_core_resolve_the_same_verified_entity_path() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    let data_home = temp.path().join("data");
    let config_home = temp.path().join("config");
    fs::create_dir(&workspace).unwrap();
    let environment = |command: &mut Command| {
        command
            .env("XDG_DATA_HOME", &data_home)
            .env("XDG_CONFIG_HOME", &config_home)
            .env("HOME", temp.path())
            .current_dir(&workspace);
    };

    let mut init = forge();
    init.arg("init").arg(&workspace);
    environment(&mut init);
    let output = init.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let storage = data_home.join("ayeque-forge");
    let project = resolve_project_at(&storage, &workspace).unwrap();
    fs::write(
        project.manifest_path(),
        "format = 2\n[[entity]]\nid = \"agent\"\nkind = \"agent\"\nschema = \"1\"\ngit = \"/repo\"\nrevision = \"main\"\n",
    )
    .unwrap();
    let project = resolve_project_at(&storage, &workspace).unwrap();
    fs::create_dir_all(project.entities_path().join("agent")).unwrap();
    let entry = LockEntry::new(
        "agent".into(),
        "/repo".into(),
        "0123456789012345678901234567890123456789".into(),
        ".".into(),
        "abcdefabcdefabcdefabcdefabcdefabcdefabcd".into(),
    )
    .unwrap();
    fs::write(
        project.lock_path(),
        serialize_lock(&project, vec![entry]).unwrap(),
    )
    .unwrap();

    let core_entity = resolve_entity(&project, "agent").unwrap();
    let mut path = forge();
    path.arg("path").arg("agent");
    environment(&mut path);
    let output = path.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        core_entity.materialized_path().to_str().unwrap()
    );
}
