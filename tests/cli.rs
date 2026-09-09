use std::fs;
use std::process::Command;

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
