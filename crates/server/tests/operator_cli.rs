use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use entity::{user, user_token};
use sea_orm::{ActiveModelTrait, ConnectionTrait, Database, EntityTrait, PaginatorTrait, Set};
use serde_json::Value;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("console-operator-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rustdesk-console"));
        command.current_dir(&self.0).args(args);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("RUSTDESK_API_") {
                command.env_remove(key);
            }
        }
        command
    }

    fn config(&self, yaml: &str) {
        fs::create_dir_all(self.0.join("conf")).unwrap();
        fs::write(self.0.join("conf/config.yaml"), yaml).unwrap();
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn json(output: Output, exit_code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(exit_code),
        "{}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    value
}

#[test]
fn version_validation_and_empty_status_have_no_local_side_effects() {
    let directory = TestDirectory::new();
    let run = |args: &[&str]| directory.command(args).output().unwrap();

    let version = json(run(&["version", "--config", "/missing", "--json"]), 0);
    assert_eq!(version["version"], env!("CARGO_PKG_VERSION"));
    assert!(version["database_schema"].is_u64());
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);

    directory.config("admin:\n  password: validation-test-secret\nrustdesk:\n  key-file: absent\n");
    let valid = json(run(&["config", "validate", "--json"]), 0);
    assert_eq!(valid["valid"], true);
    assert_eq!(valid["warnings"][0]["field"], "rustdesk.key-file");
    let status = json(
        run(&[
            "database",
            "status",
            "--config",
            "conf/config.yaml",
            "--json",
        ]),
        0,
    );
    assert_eq!(status["state"], "empty");
    assert_eq!(status["safe_to_start"], false);
    assert!(!directory.0.join("data").exists());
    assert!(!directory.0.join("runtime").exists());

    directory.config("gin:\n  api-addr: invalid-address-secret\n");
    let invalid = json(run(&["config", "validate", "--json"]), 3);
    assert_eq!(invalid["errors"][0]["field"], "gin.api-addr");
    assert!(!invalid.to_string().contains("invalid-address-secret"));
    assert!(!valid.to_string().contains("validation-test-secret"));
}

#[tokio::test]
async fn status_and_migration_preserve_a_future_database() {
    let directory = TestDirectory::new();
    directory.config("{}");
    let run = |args: &[&str]| directory.command(args).output().unwrap();
    let path = directory.0.join("data/rustdeskapi.db");
    fs::create_dir(path.parent().unwrap()).unwrap();
    let db = Database::connect(format!("sqlite://{}?mode=rwc", path.display()))
        .await
        .unwrap();
    let target = json(run(&["version", "--json"]), 0)["database_schema"]
        .as_u64()
        .unwrap();
    db.execute_unprepared("CREATE TABLE versions (version INTEGER NOT NULL)")
        .await
        .unwrap();
    db.execute_unprepared(&format!("INSERT INTO versions VALUES ({})", target + 1))
        .await
        .unwrap();
    db.close().await.unwrap();
    let before = fs::read(&path).unwrap();

    let status = json(run(&["database", "status", "--json"]), 5);
    assert_eq!(status["state"], "newer_than_binary");
    assert_eq!(status["installed_schema"], target + 1);
    assert_eq!(status["safe_to_start"], false);
    assert!(!directory
        .0
        .join("data/rustdeskapi.db.migrate.lock")
        .exists());
    let migrate = json(run(&["database", "migrate", "--json"]), 5);
    assert_eq!(migrate["state"], "newer_than_binary");
    directory.config(
        "gin:
  api-addr: invalid-to-prevent-listener
",
    );
    let startup = run(&[]);
    assert_eq!(startup.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&startup.stdout).contains("is newer than the supported schema"));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!directory.0.join("runtime").exists());
}

#[test]
fn migration_is_schema_only_and_explicit() {
    let directory = TestDirectory::new();
    directory.config("admin:\n  password: this-must-not-seed\n");
    let first = directory
        .command(&["database", "migrate", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let second = directory
        .command(&["database", "migrate", "--json"])
        .output()
        .unwrap();
    let first = first.wait_with_output().unwrap();
    let outcomes = [json(first, 0), json(second, 0)];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| outcome["migrated"] == true)
            .count(),
        1
    );
    let status = json(
        directory
            .command(&["database", "status", "--json"])
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(status["state"], "current");
    assert_eq!(status["safe_to_start"], true);
    assert!(!directory.0.join("runtime").exists());
}

#[test]
fn recovery_rejects_invalid_input_before_opening_the_database() {
    let directory = TestDirectory::new();
    directory.config("{}");
    let mut child = directory
        .command(&["reset-admin-pwd", "--password-stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"private-test-password\nsecond-line\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("private-test-password"));
    assert!(!directory.0.join("data").exists());
    assert!(!directory.0.join("runtime").exists());
}

#[cfg(unix)]
#[test]
fn recovery_rejects_symlink_and_public_password_file() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let directory = TestDirectory::new();
    directory.config("{}");
    let path = directory.0.join("secret");
    fs::write(&path, "private-test-password\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    let output = directory
        .command(&["reset-pwd", "1", "--password-file", "secret"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("private-test-password"));

    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&path, directory.0.join("link")).unwrap();
    let output = directory
        .command(&["reset-admin-pwd", "--password-file", "link"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!directory.0.join("data").exists());
}

#[tokio::test]
async fn recovery_preserves_positional_usage_and_revokes_sessions() {
    let directory = TestDirectory::new();
    directory.config("{}");
    let run = |args: &[&str]| directory.command(args).output().unwrap();
    json(run(&["database", "migrate", "--json"]), 0);
    let path = directory.0.join("data/rustdeskapi.db");
    let db = Database::connect(format!("sqlite://{}?mode=rw", path.display()))
        .await
        .unwrap();
    let account = user::ActiveModel {
        username: Set("admin".to_owned()),
        is_admin: Set(Some(true)),
        tfa_secret: Set("dummy-factor".to_owned()),
        tfa_enabled: Set(true),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let token = user_token::ActiveModel {
        user_id: Set(account.id),
        token: Set("dummy-session".to_owned()),
        ..Default::default()
    };
    token.clone().insert(&db).await.unwrap();
    let mut child = directory
        .command(&["reset-admin-pwd", "--password-stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b" stdin-test-password \r\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("stdin-test-password"));
    let account = user::Entity::find_by_id(account.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert!(bcrypt::verify(" stdin-test-password ", &account.password).unwrap());
    assert_eq!(user_token::Entity::find().count(&db).await.unwrap(), 0);

    let output = run(&["reset-pwd", &account.id.to_string(), "legacy-test-password"]);
    assert!(output.status.success());
    let account = user::Entity::find_by_id(account.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert!(bcrypt::verify("legacy-test-password", &account.password).unwrap());
    assert!(account.tfa_enabled);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.0.join("secret"))
            .unwrap();
        file.write_all(b"file-test-password\n").unwrap();
        drop(file);

        let output = run(&[
            "reset-pwd",
            &account.id.to_string(),
            "--password-file",
            "secret",
        ]);
        assert!(output.status.success());
        let updated = user::Entity::find_by_id(account.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(bcrypt::verify("file-test-password", &updated.password).unwrap());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("file-test-password"));
    }

    token.insert(&db).await.unwrap();
    let output = run(&["reset-totp", &account.id.to_string()]);
    assert!(output.status.success());
    let account = user::Entity::find_by_id(account.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert!(!account.tfa_enabled);
    assert!(account.tfa_secret.is_empty());
    assert_eq!(user_token::Entity::find().count(&db).await.unwrap(), 0);
}
