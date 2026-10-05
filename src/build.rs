use crate::{config::Config, error::Error, policy, project::SelectedPackage, runner::Runner, ui};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Debug)]
pub struct TargetOutputs {
    pub target: String,
    pub binaries: Vec<(String, PathBuf)>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct BuildFingerprint {
    format_version: u8,
    commit: String,
    package: String,
    version: String,
    binaries: Vec<String>,
    targets: Vec<String>,
    profile: String,
    locked: bool,
    engine: String,
    toolchain: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct BuildCache {
    fingerprint: BuildFingerprint,
    target_dir: PathBuf,
}

pub fn prerequisites(
    root: &Path,
    config: &Config,
    needs_cross: bool,
    needs_binstall: bool,
    runner: &Runner,
) -> (Vec<String>, Option<String>) {
    let mut problems = Vec::new();
    if let Err(error) = validate_cross_config(root) {
        problems.push(error.to_string());
    }
    for tool in ["git", "cargo"]
        .into_iter()
        .chain(needs_binstall.then_some("cargo-binstall"))
    {
        if executable(tool).is_none() {
            problems.push(format!("{tool} is missing"));
        }
    }
    if needs_cross && cross_program().is_none() {
        problems.push("cross is missing (looked for cross and cargo-cross)".into());
    }
    let mut engine = None;
    if needs_cross {
        let requested = env::var("CROSS_CONTAINER_ENGINE").ok();
        let candidate = requested.or_else(|| {
            ["docker", "podman"]
                .into_iter()
                .find(|tool| executable(tool).is_some())
                .map(str::to_owned)
        });
        match candidate.as_deref() {
            Some("docker" | "podman") => {
                if let Some(candidate) = candidate {
                    if let Err(error) = runner.output(&candidate, &["info"], root) {
                        problems.push(error.to_string());
                    } else {
                        engine = Some(candidate);
                    }
                }
            }
            _ => problems.push(
                "Set CROSS_CONTAINER_ENGINE to docker or podman with a usable backend".into(),
            ),
        }
    }
    if config.locked && !root.join("Cargo.lock").is_file() {
        problems.push("Cargo.lock is missing; generate it explicitly or use --unlocked".into());
    }
    (problems, engine)
}

fn executable(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    executable_in(name, &path)
}

fn executable_in(name: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

fn cross_program() -> Option<&'static str> {
    let path = env::var_os("PATH")?;
    cross_program_in(&path)
}

fn cross_program_in(path: &std::ffi::OsStr) -> Option<&'static str> {
    env::split_paths(path).find_map(|directory| {
        ["cross", "cargo-cross"]
            .into_iter()
            .find(|name| directory.join(name).is_file())
    })
}

fn validate_cross_config(root: &Path) -> Result<(), Error> {
    let source = fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|e| Error(format!("cannot inspect root Cargo.toml: {e}")))?;
    let manifest: toml::Value =
        toml::from_str(&source).map_err(|e| Error(format!("invalid root Cargo.toml: {e}")))?;
    if let Some(cross) = manifest
        .get("workspace")
        .and_then(|value| value.get("metadata"))
        .and_then(|value| value.get("cross"))
    {
        reject_hooks(cross)?;
    }
    let path = root.join("Cross.toml");
    if path.exists() {
        let source = fs::read_to_string(&path)
            .map_err(|e| Error(format!("cannot read {}: {e}", path.display())))?;
        let cross: toml::Value = toml::from_str(&source)
            .map_err(|e| Error(format!("invalid {}: {e}", path.display())))?;
        reject_hooks(&cross)?;
    }
    Ok(())
}

fn reject_hooks(value: &toml::Value) -> Result<(), Error> {
    match value {
        toml::Value::Table(table) => {
            for (key, child) in table {
                if matches!(key.as_str(), "pre-build" | "dockerfile" | "runner") {
                    return Err(Error(format!("Repository cross configuration contains unsupported executable option {key:?}.")));
                }
                reject_hooks(child)?;
            }
        }
        toml::Value::Array(array) => {
            for child in array {
                reject_hooks(child)?;
            }
        }
        _ => (),
    }
    Ok(())
}

pub fn verify_source(
    root: &Path,
    project: &SelectedPackage,
    config: &Config,
    skip_tests: bool,
    runner: &Runner,
) -> Result<(), Error> {
    let mut flags = vec!["--package", project.name.as_str()];
    if config.locked {
        flags.push("--locked");
    }
    runner.output("cargo", &[&["check"], flags.as_slice()].concat(), root)?;
    if !skip_tests {
        runner.output("cargo", &[&["test"], flags.as_slice()].concat(), root)?;
    }
    Ok(())
}

pub fn cross_arguments(
    project: &SelectedPackage,
    config: &Config,
    target: &str,
    target_dir: &Path,
) -> Vec<String> {
    let mut args = vec!["build".to_owned()];
    if config.profile == "release" {
        args.push("--release".into());
    } else {
        args.extend(["--profile".into(), config.profile.clone()]);
    }
    if config.locked {
        args.push("--locked".into());
    }
    args.extend([
        "--target".into(),
        target.to_owned(),
        "--target-dir".into(),
        target_dir.display().to_string(),
        "--package".into(),
        project.name.clone(),
    ]);
    for binary in &project.binaries {
        args.extend(["--bin".into(), binary.clone()]);
    }
    args
}

pub fn build(
    root: &Path,
    project: &SelectedPackage,
    config: &Config,
    engine: &str,
    clean_commit: Option<&str>,
    recompile: bool,
    runner: &Runner,
) -> Result<Vec<TargetOutputs>, Error> {
    let target_base = project.target_directory.join("binstall-publisher");
    fs::create_dir_all(&target_base)
        .map_err(|e| Error(format!("cannot create {}: {e}", target_base.display())))?;
    let toolchain = executable("rustup").and_then(|_| {
        runner
            .text("rustup", &["show", "active-toolchain"], root)
            .ok()
            .and_then(|line| line.split_whitespace().next().map(str::to_owned))
    });
    let fingerprint = clean_commit.map(|commit| BuildFingerprint {
        format_version: 1,
        commit: commit.to_owned(),
        package: project.name.clone(),
        version: project.version.clone(),
        binaries: project.binaries.clone(),
        targets: config.targets.clone(),
        profile: config.profile.clone(),
        locked: config.locked,
        engine: engine.to_owned(),
        toolchain: toolchain.clone(),
    });
    let cache_path = target_base.join("release-build.json");
    if !recompile {
        if let Some(expected) = fingerprint.as_ref() {
            if let Some(outputs) =
                cached_outputs(&target_base, &cache_path, expected, project, config)?
            {
                ui::success(format_args!(
                    "Reusing completed release build for commit {} (use --recompile to rebuild)",
                    short_commit(&expected.commit)
                ));
                return Ok(outputs);
            }
        }
    }
    let fresh = tempfile::Builder::new()
        .prefix("build-")
        .tempdir_in(&target_base)
        .map_err(|e| Error(format!("cannot create fresh build directory: {e}")))?;
    let target_dir = fresh.keep();
    println!("Fresh build directory: {}", target_dir.display());
    let mut environment = vec![("CROSS_CONTAINER_ENGINE", engine)];
    if let Some(toolchain) = toolchain.as_deref() {
        environment.push(("RUSTUP_TOOLCHAIN", toolchain));
    }
    let profile_dir = if config.profile == "dev" {
        "debug"
    } else {
        &config.profile
    };
    let mut outputs = Vec::new();
    let cross = cross_program()
        .ok_or_else(|| Error("cross is missing (looked for cross and cargo-cross)".into()))?;
    let builds_started = Instant::now();
    for target in &config.targets {
        println!("Building {target}");
        let target_started = Instant::now();
        // Cross images can use different host libc versions. Keep their native
        // build scripts and proc macros isolated so a later image never tries
        // to execute artifacts produced by an earlier one.
        let target_build_dir = target_dir.join(target);
        let args = cross_arguments(project, config, target, &target_build_dir);
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        runner.output_with(cross, &borrowed, root, &environment, &[])?;
        let mut binaries = Vec::new();
        for name in &project.binaries {
            let binary = target_build_dir
                .join(target)
                .join(profile_dir)
                .join(policy::binary_filename(name, target));
            let metadata = fs::symlink_metadata(&binary).map_err(|_| {
                Error(format!(
                    "Build did not produce a regular binary: {}",
                    binary.display()
                ))
            })?;
            if !metadata.file_type().is_file() {
                return Err(Error(format!(
                    "Build did not produce a regular binary: {}",
                    binary.display()
                )));
            }
            binaries.push((name.clone(), binary));
        }
        outputs.push(TargetOutputs {
            target: target.clone(),
            binaries,
        });
        ui::success(format_args!(
            "Built {target} in {:.1}s",
            target_started.elapsed().as_secs_f64()
        ));
    }
    ui::success(format_args!(
        "Cross builds complete in {:.1}s",
        builds_started.elapsed().as_secs_f64()
    ));
    if let Some(fingerprint) = fingerprint {
        let cache = BuildCache {
            fingerprint,
            target_dir,
        };
        let encoded = serde_json::to_vec_pretty(&cache)
            .map_err(|e| Error(format!("cannot encode release build cache: {e}")))?;
        let mut temporary = tempfile::NamedTempFile::new_in(&target_base)
            .map_err(|e| Error(format!("cannot create release build cache: {e}")))?;
        use std::io::Write as _;
        temporary
            .write_all(&encoded)
            .map_err(|e| Error(format!("cannot write release build cache: {e}")))?;
        temporary
            .persist(&cache_path)
            .map_err(|e| Error(format!("cannot save release build cache: {e}")))?;
    }
    Ok(outputs)
}

fn cached_outputs(
    target_base: &Path,
    cache_path: &Path,
    expected: &BuildFingerprint,
    project: &SelectedPackage,
    config: &Config,
) -> Result<Option<Vec<TargetOutputs>>, Error> {
    match fs::symlink_metadata(cache_path) {
        Ok(metadata) if metadata.file_type().is_file() => (),
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error(format!(
                "cannot inspect release build cache {}: {error}",
                cache_path.display()
            )))
        }
    }
    let encoded =
        fs::read(cache_path).map_err(|e| Error(format!("cannot read release build cache: {e}")))?;
    let cache: BuildCache = match serde_json::from_slice(&encoded) {
        Ok(cache) => cache,
        Err(_) => return Ok(None),
    };
    if &cache.fingerprint != expected {
        return Ok(None);
    }
    let base = fs::canonicalize(target_base)
        .map_err(|e| Error(format!("cannot resolve build cache directory: {e}")))?;
    let target_dir = match fs::canonicalize(&cache.target_dir) {
        Ok(path) if path.starts_with(&base) => path,
        _ => return Ok(None),
    };
    let profile_dir = if config.profile == "dev" {
        "debug"
    } else {
        &config.profile
    };
    let mut outputs = Vec::new();
    for target in &config.targets {
        let mut binaries = Vec::new();
        for name in &project.binaries {
            let binary = target_dir
                .join(target)
                .join(target)
                .join(profile_dir)
                .join(policy::binary_filename(name, target));
            let resolved = match fs::canonicalize(&binary) {
                Ok(path) if path.starts_with(&base) => path,
                _ => return Ok(None),
            };
            match fs::symlink_metadata(&binary) {
                Ok(metadata) if metadata.file_type().is_file() => {
                    binaries.push((name.clone(), resolved));
                }
                _ => return Ok(None),
            }
        }
        outputs.push(TargetOutputs {
            target: target.clone(),
            binaries,
        });
    }
    Ok(Some(outputs))
}

fn short_commit(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_command_honors_path_order_and_falls_back_to_cargo_cross() {
        let first = tempfile::tempdir().unwrap();
        let later = tempfile::tempdir().unwrap();
        let path = env::join_paths([first.path(), later.path()]).unwrap();

        fs::write(first.path().join("cargo-cross"), "").unwrap();
        fs::write(later.path().join("cross"), "").unwrap();
        assert_eq!(cross_program_in(&path), Some("cargo-cross"));

        fs::write(first.path().join("cross"), "").unwrap();
        assert_eq!(cross_program_in(&path), Some("cross"));
    }

    fn test_project(target_directory: PathBuf) -> SelectedPackage {
        SelectedPackage {
            name: "cli".into(),
            version: "0.1.0".into(),
            binaries: vec!["cli".into(), "helper".into()],
            manifest: PathBuf::from("Cargo.toml"),
            repository: None,
            target_directory,
        }
    }

    fn test_config() -> Config {
        Config {
            targets: vec!["x86_64-pc-windows-gnu".into()],
            profile: "release".into(),
            locked: true,
            tag_prefix: "v".into(),
            token_env: "GITEA_TOKEN".into(),
            package_format: crate::config::PackageFormat::Tgz,
            repository_fallbacks: Vec::new(),
        }
    }

    #[test]
    fn cross_command_scopes_package_and_uses_fresh_target_path() {
        let project = test_project(PathBuf::from("target"));
        let config = test_config();
        let args = cross_arguments(
            &project,
            &config,
            "x86_64-pc-windows-gnu",
            Path::new("/tmp/fresh"),
        );
        assert_eq!(
            args,
            [
                "build",
                "--release",
                "--locked",
                "--target",
                "x86_64-pc-windows-gnu",
                "--target-dir",
                "/tmp/fresh",
                "--package",
                "cli",
                "--bin",
                "cli",
                "--bin",
                "helper"
            ]
        );
    }

    #[test]
    fn completed_clean_build_is_reused_only_for_the_same_fingerprint() {
        let temporary = tempfile::tempdir().unwrap();
        let target_base = temporary.path().join("binstall-publisher");
        let target_dir = target_base.join("build-complete");
        let project = test_project(temporary.path().to_owned());
        let config = test_config();
        for binary in &project.binaries {
            let path = target_dir
                .join(&config.targets[0])
                .join(&config.targets[0])
                .join("release")
                .join(policy::binary_filename(binary, &config.targets[0]));
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, binary).unwrap();
        }
        let fingerprint = BuildFingerprint {
            format_version: 1,
            commit: "0123456789abcdef".into(),
            package: project.name.clone(),
            version: project.version.clone(),
            binaries: project.binaries.clone(),
            targets: config.targets.clone(),
            profile: config.profile.clone(),
            locked: config.locked,
            engine: "podman".into(),
            toolchain: Some("stable-x86_64-unknown-linux-gnu".into()),
        };
        let cache_path = target_base.join("release-build.json");
        fs::write(
            &cache_path,
            serde_json::to_vec(&BuildCache {
                fingerprint: fingerprint.clone(),
                target_dir,
            })
            .unwrap(),
        )
        .unwrap();
        let outputs = cached_outputs(&target_base, &cache_path, &fingerprint, &project, &config)
            .unwrap()
            .unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].binaries.len(), 2);

        let different = BuildFingerprint {
            commit: "different".into(),
            ..fingerprint
        };
        assert!(
            cached_outputs(&target_base, &cache_path, &different, &project, &config)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn cross_hooks_are_rejected() {
        let doc: toml::Value =
            toml::from_str("[target.x86_64-unknown-linux-musl]\npre-build=['echo unsafe']")
                .unwrap();
        assert!(reject_hooks(&doc).is_err());
    }

    #[test]
    fn malformed_cross_configuration_and_missing_lockfile_are_reported() {
        let temp = tempfile::tempdir().unwrap();
        assert!(validate_cross_config(temp.path())
            .unwrap_err()
            .to_string()
            .contains("cannot inspect"));

        fs::write(temp.path().join("Cargo.toml"), "not valid toml = [").unwrap();
        assert!(validate_cross_config(temp.path())
            .unwrap_err()
            .to_string()
            .contains("invalid root"));

        fs::write(
            temp.path().join("Cargo.toml"),
            "[workspace.metadata.cross.target.demo]\nrunner = 'unsafe'\n",
        )
        .unwrap();
        assert!(validate_cross_config(temp.path())
            .unwrap_err()
            .to_string()
            .contains("runner"));

        fs::write(temp.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(temp.path().join("Cross.toml"), "invalid = [").unwrap();
        assert!(validate_cross_config(temp.path())
            .unwrap_err()
            .to_string()
            .contains("invalid"));
        fs::remove_file(temp.path().join("Cross.toml")).unwrap();

        let config = test_config();
        let (problems, engine) = prerequisites(
            temp.path(),
            &config,
            false,
            false,
            &Runner::new("TEST_TOKEN"),
        );
        assert!(engine.is_none());
        assert!(problems
            .iter()
            .any(|problem| problem.contains("Cargo.lock")));

        let mut dev = test_config();
        dev.profile = "dev".into();
        dev.locked = false;
        let args = cross_arguments(
            &test_project(PathBuf::from("target")),
            &dev,
            "demo-target",
            Path::new("target-dir"),
        );
        assert!(args.windows(2).any(|pair| pair == ["--profile", "dev"]));
        assert!(!args.contains(&"--locked".into()));
    }
}
