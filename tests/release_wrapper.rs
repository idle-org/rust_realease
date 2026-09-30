#![cfg(unix)]

use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};

use tempfile::TempDir;

const DEVELOPMENT_ORIGIN: &str = "git@git.dandaedre.com:idle-org/rust_realease.git";
const RELEASE_CONFIG: &str = "[[repository.fallbacks]]\n\
origin = \"git@git.dandaedre.com:idle-org/rust_realease.git\"\n\
fallback-url = \"https://proxy.dandaedre.com/gitea/idle-org/rust_realease\"\n";

fn run(command: &mut Command) -> Output {
    command.output().expect("test command should start")
}

fn git(repository: &Path, arguments: &[&str]) -> Output {
    run(Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments))
}

#[test]
fn release_wrapper_passes_the_token_and_preserves_the_ssh_origin() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    let repository = temporary.path().join("repository");
    let fake_bin = temporary.path().join("bin");
    fs::create_dir_all(&repository).expect("repository directory should be created");
    fs::create_dir_all(&fake_bin).expect("fake binary directory should be created");

    for name in ["release.sh", "rust-release.toml"] {
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join(name),
            repository.join(name),
        )
        .expect("release file should be copied");
    }
    fs::write(
        repository.join("Cargo.toml"),
        "[package]\nname = \"rust_realease\"\nversion = \"0.1.3\"\n",
    )
    .expect("manifest should be written");
    fs::write(repository.join("token.txt"), "test-token\n").expect("token file should be written");

    let fake_release = fake_bin.join("rust_realease");
    fs::write(
        &fake_release,
        "#!/bin/sh\n\
         [ \"$GITEA_TOKEN\" = test-token ] || exit 91\n\
         git -C \"$1\" remote get-url origin >\"$OBSERVED_ORIGIN\"\n\
         cat \"$1/rust-release.toml\" >\"$OBSERVED_CONFIG\"\n\
         printf '%s\\n' \"$@\" >\"$OBSERVED_ARGS\"\n",
    )
    .expect("fake release executable should be written");
    for path in [&fake_release, &repository.join("release.sh")] {
        let mut permissions = fs::metadata(path)
            .expect("script metadata should be readable")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("script should be executable");
    }

    assert!(git(&repository, &["init", "--quiet"]).status.success());
    assert!(git(
        &repository,
        &["remote", "add", "origin", DEVELOPMENT_ORIGIN]
    )
    .status
    .success());

    let observed_origin = temporary.path().join("observed-origin.txt");
    let observed_config = temporary.path().join("observed-config.toml");
    let observed_args = temporary.path().join("observed-args.txt");
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        env::var("PATH").expect("PATH should be available")
    );
    let output = run(Command::new(repository.join("release.sh"))
        .arg(repository.join("token.txt"))
        .arg("--dry-run")
        .env("PATH", path)
        .env("OBSERVED_ORIGIN", &observed_origin)
        .env("OBSERVED_CONFIG", &observed_config)
        .env("OBSERVED_ARGS", &observed_args));

    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(&observed_origin)
            .expect("fake release should record the origin")
            .trim(),
        DEVELOPMENT_ORIGIN
    );
    assert_eq!(
        fs::read_to_string(&observed_config).expect("fake release should record the configuration"),
        RELEASE_CONFIG
    );
    let arguments =
        fs::read_to_string(&observed_args).expect("fake release should record publisher arguments");
    assert!(arguments.lines().any(|argument| argument == "both"));
    assert!(arguments.lines().any(|argument| argument == "--dry-run"));
    assert!(!arguments.lines().any(|argument| argument == "--prerelease"));
}
