use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn forge() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ayeque-forge"))
}

fn git(directory: &Path, args: &[&str]) -> Output {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn create_source(directory: &Path) {
    fs::create_dir_all(directory.join("one")).unwrap();
    fs::create_dir_all(directory.join("two")).unwrap();
    fs::write(directory.join("one/value.txt"), "one\n").unwrap();
    fs::write(directory.join("two/value.txt"), "two\n").unwrap();
    git(directory, &["init", "--initial-branch=main"]);
    git(directory, &["config", "user.name", "Forge Test"]);
    git(
        directory,
        &["config", "user.email", "forge@example.invalid"],
    );
    git(directory, &["add", "."]);
    git(directory, &["commit", "-m", "fixture"]);
}

#[test]
fn init_is_idempotent_outside_git() {
    let temp = tempfile::tempdir().unwrap();
    for expected in ["created", "validated"] {
        let output = forge().arg("init").arg(temp.path()).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains(expected));
    }
    assert_eq!(
        fs::read(temp.path().join("FORGE.toml")).unwrap(),
        b"format = 1\n"
    );
}

#[test]
fn init_uses_the_exact_directory_and_preserves_an_invalid_manifest() {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "--initial-branch=main"]);
    let nested = temp.path().join("nested/workspace");
    fs::create_dir_all(&nested).unwrap();

    let output = forge().arg("init").arg(&nested).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!temp.path().join("FORGE.toml").exists());
    assert_eq!(
        fs::read(nested.join("FORGE.toml")).unwrap(),
        b"format = 1\n"
    );

    let invalid = b"format = 2\n";
    fs::write(nested.join("FORGE.toml"), invalid).unwrap();
    let failed = forge().arg("init").arg(&nested).output().unwrap();
    assert!(!failed.status.success());
    assert_eq!(fs::read(nested.join("FORGE.toml")).unwrap(), invalid);
}

#[test]
fn lock_materializes_two_entities_and_preserves_old_lock_on_failure() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let workspace = temp.path().join("workspace");
    let nested = workspace.join("nested");
    let data = temp.path().join("data");
    fs::create_dir_all(&nested).unwrap();
    create_source(&source);

    let manifest = format!(
        r#"format = 1

[[entity]]
id = "second"
kind = "test"
schema = "1"
git = "{}"
revision = "main"
path = "two"

[[entity]]
id = "first"
kind = "test"
schema = "1"
git = "{}"
revision = "main"
path = "one"
"#,
        source.display(),
        source.display()
    );
    fs::write(workspace.join("FORGE.toml"), &manifest).unwrap();
    let output = forge()
        .arg("lock")
        .current_dir(&nested)
        .env("XDG_DATA_HOME", &data)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lock = fs::read(workspace.join("FORGE.lock")).unwrap();
    let text = String::from_utf8(lock.clone()).unwrap();
    assert!(text.find("id = \"first\"").unwrap() < text.find("id = \"second\"").unwrap());
    assert_eq!(text.matches("commit = ").count(), 2);
    assert_eq!(text.matches("tree = ").count(), 2);

    fs::write(
        workspace.join("FORGE.toml"),
        manifest.replace("path = \"two\"", "path = \"missing\""),
    )
    .unwrap();
    let failed = forge()
        .arg("lock")
        .current_dir(&nested)
        .env("XDG_DATA_HOME", &data)
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert_eq!(fs::read(workspace.join("FORGE.lock")).unwrap(), lock);
}

#[test]
fn lock_reresolves_a_moving_branch() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    create_source(&source);
    fs::write(
        workspace.join("FORGE.toml"),
        format!(
            r#"format = 1

[[entity]]
id = "moving"
kind = "test"
schema = "1"
git = "{}"
revision = "main"
path = "one"
"#,
            source.display()
        ),
    )
    .unwrap();
    let data = temp.path().join("data");
    let first = forge()
        .arg("lock")
        .current_dir(&workspace)
        .env("XDG_DATA_HOME", &data)
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_lock = fs::read_to_string(workspace.join("FORGE.lock")).unwrap();

    fs::write(source.join("one/value.txt"), "changed\n").unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "advance main"]);
    let second = forge()
        .arg("lock")
        .current_dir(&workspace)
        .env("XDG_DATA_HOME", &data)
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_lock = fs::read_to_string(workspace.join("FORGE.lock")).unwrap();
    assert_ne!(first_lock, second_lock);
    assert!(
        second_lock.contains(
            String::from_utf8(git(&source, &["rev-parse", "HEAD"]).stdout)
                .unwrap()
                .trim()
        )
    );
}

#[test]
fn lock_materializes_lfs_and_rejects_a_missing_object() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(source.join("entity")).unwrap();
    git(&source, &["init", "--initial-branch=main"]);
    git(&source, &["config", "user.name", "Forge Test"]);
    git(&source, &["config", "user.email", "forge@example.invalid"]);
    git(&source, &["lfs", "install", "--local"]);
    git(&source, &["lfs", "track", "*.bin"]);
    let payload = b"an LFS payload used by the Forge integration test\n";
    fs::write(source.join("entity/payload.bin"), payload).unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "LFS fixture"]);

    let manifest = format!(
        r#"format = 1

[[entity]]
id = "lfs"
kind = "test"
schema = "1"
git = "{}"
revision = "main"
path = "entity"
"#,
        source.display()
    );
    let first_workspace = temp.path().join("first-workspace");
    fs::create_dir(&first_workspace).unwrap();
    fs::write(first_workspace.join("FORGE.toml"), &manifest).unwrap();
    let first = forge()
        .arg("lock")
        .current_dir(&first_workspace)
        .env("XDG_DATA_HOME", temp.path().join("first-data"))
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let path = String::from_utf8(first.stdout)
        .unwrap()
        .split_once('\t')
        .unwrap()
        .1
        .trim()
        .to_owned();
    assert_eq!(
        fs::read(Path::new(&path).join("payload.bin")).unwrap(),
        payload
    );

    fs::remove_dir_all(source.join(".git/lfs/objects")).unwrap();
    let second_workspace = temp.path().join("second-workspace");
    fs::create_dir(&second_workspace).unwrap();
    fs::write(second_workspace.join("FORGE.toml"), manifest).unwrap();
    let second = forge()
        .arg("lock")
        .current_dir(&second_workspace)
        .env("XDG_DATA_HOME", temp.path().join("second-data"))
        .output()
        .unwrap();
    assert!(!second.status.success());
    assert!(!second_workspace.join("FORGE.lock").exists());
}
