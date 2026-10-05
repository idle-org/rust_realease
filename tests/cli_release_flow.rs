#![cfg(unix)]

use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};

use tempfile::TempDir;

fn run(command: &mut Command) -> Output {
    command.output().expect("test command should start")
}

fn git(repository: &Path, arguments: &[&str]) {
    let output = run(Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments));
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn executable(path: &Path, source: &str) {
    fs::write(path, source).expect("fake executable should be written");
    let mut permissions = fs::metadata(path)
        .expect("fake executable metadata should be readable")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("fake executable should be executable");
}

fn publisher(project: &Path, fake_bin: &Path, cross_log: &Path, arguments: &[&str]) -> Output {
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        env::var("PATH").expect("PATH should be available")
    );
    run(Command::new(env!("CARGO_BIN_EXE_rust_realease"))
        .arg(project)
        .args(arguments)
        .env("PATH", path)
        .env("CROSS_LOG", cross_log)
        .env_remove("GITEA_TOKEN"))
}

fn assert_success(output: &Output) -> String {
    assert!(
        output.status.success(),
        "publisher failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn compile_only_reuses_clean_commit_and_rebuilds_changed_or_forced_inputs() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    let project = temporary.path().join("project");
    let fake_bin = temporary.path().join("bin");
    let cross_log = temporary.path().join("cross.log");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(&fake_bin).unwrap();
    fs::write(
        project.join("Cargo.toml"),
        "[package]\n\
         name = \"demo\"\n\
         version = \"0.1.0\"\n\
         edition = \"2021\"\n\
         repository = \"https://example.com/team/demo\"\n\
         [package.metadata.binstall]\n\
         pkg-url = \"{ repo }/releases/download/v{ version }/{ name }-{ target }-v{ version }.tgz\"\n\
         pkg-fmt = \"tgz\"\n\
         bin-dir = \"{ name }-{ target }-v{ version }/{ bin }{ binary-ext }\"\n",
    )
    .unwrap();
    fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(project.join(".gitignore"), "/target/\n/dist/\n").unwrap();
    fs::write(
        project.join("rust-release.toml"),
        "[build]\ntargets = [\"x86_64-unknown-linux-musl\"]\n",
    )
    .unwrap();
    executable(&fake_bin.join("docker"), "#!/bin/sh\n[ \"$1\" = info ]\n");
    executable(&fake_bin.join("cargo-binstall"), "#!/bin/sh\nexit 0\n");
    executable(
        &fake_bin.join("cargo-cross"),
        "#!/bin/sh\n\
         set -eu\n\
         target=\n\
         target_dir=\n\
         profile=debug\n\
         binaries=\n\
         while [ \"$#\" -gt 0 ]; do\n\
           case \"$1\" in\n\
             --target) shift; target=$1 ;;\n\
             --target-dir) shift; target_dir=$1 ;;\n\
             --release) profile=release ;;\n\
             --profile) shift; profile=$1 ;;\n\
             --bin) shift; binaries=\"$binaries $1\" ;;\n\
           esac\n\
           shift\n\
         done\n\
         output_dir=$target_dir/$target/$profile\n\
         mkdir -p \"$output_dir\"\n\
         for binary in $binaries; do printf 'compiled %s' \"$binary\" >\"$output_dir/$binary\"; done\n\
         printf 'cross invoked\\n' >>\"$CROSS_LOG\"\n",
    );

    git(&project, &["init", "--quiet"]);
    git(&project, &["config", "user.name", "Release Test"]);
    git(&project, &["config", "user.email", "release@example.com"]);
    git(&project, &["config", "commit.gpgsign", "false"]);
    let lock = run(Command::new("cargo")
        .args(["generate-lockfile", "--manifest-path"])
        .arg(project.join("Cargo.toml")));
    assert!(lock.status.success());
    git(&project, &["add", "."]);
    git(&project, &["commit", "--quiet", "-m", "initial"]);
    git(&project, &["tag", "v0.1.0"]);

    let dry_run = assert_success(&publisher(&project, &fake_bin, &cross_log, &["--dry-run"]));
    assert!(dry_run.contains("Dry run complete"));
    assert!(!cross_log.exists());

    let first = assert_success(&publisher(
        &project,
        &fake_bin,
        &cross_log,
        &["--compile-only"],
    ));
    assert!(first.contains("Cross builds complete"));
    assert_eq!(fs::read_to_string(&cross_log).unwrap().lines().count(), 1);

    let second = assert_success(&publisher(
        &project,
        &fake_bin,
        &cross_log,
        &["--compile-only"],
    ));
    assert!(second.contains("Reusing completed release build"));
    assert_eq!(fs::read_to_string(&cross_log).unwrap().lines().count(), 1);

    fs::write(
        project.join("src/main.rs"),
        "fn main() { println!(\"changed\"); }\n",
    )
    .unwrap();
    let dirty_publication = publisher(&project, &fake_bin, &cross_log, &[]);
    assert!(!dirty_publication.status.success());
    assert!(String::from_utf8_lossy(&dirty_publication.stderr)
        .contains("Publication requires a clean checkout"));
    assert_success(&publisher(
        &project,
        &fake_bin,
        &cross_log,
        &["--compile-only"],
    ));
    assert_eq!(fs::read_to_string(&cross_log).unwrap().lines().count(), 2);

    git(&project, &["add", "src/main.rs"]);
    git(&project, &["commit", "--quiet", "-m", "change source"]);
    let changed = assert_success(&publisher(
        &project,
        &fake_bin,
        &cross_log,
        &["--compile-only"],
    ));
    assert!(changed.contains("Cross builds complete"));
    assert_eq!(fs::read_to_string(&cross_log).unwrap().lines().count(), 3);

    assert_success(&publisher(
        &project,
        &fake_bin,
        &cross_log,
        &["--compile-only", "--recompile"],
    ));
    assert_eq!(fs::read_to_string(&cross_log).unwrap().lines().count(), 4);
}

#[test]
fn missing_message_file_is_reported_before_project_acquisition() {
    let temporary = TempDir::new().unwrap();
    let output = run(Command::new(env!("CARGO_BIN_EXE_rust_realease"))
        .arg("missing-project")
        .args(["--message-file", "missing-notes.md"])
        .current_dir(temporary.path()));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read release message file"));
}
