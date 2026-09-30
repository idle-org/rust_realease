use crate::{config::Config, error::Error, policy, project::SelectedPackage, runner::Runner};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct TargetOutputs {
    pub target: String,
    pub binaries: Vec<(String, PathBuf)>,
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
        .chain(needs_cross.then_some("cross"))
        .chain(needs_binstall.then_some("cargo-binstall"))
    {
        if executable(tool).is_none() {
            problems.push(format!("{tool} is missing"));
        }
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
    env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
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
    runner: &Runner,
) -> Result<Vec<TargetOutputs>, Error> {
    let target_base = project.target_directory.join("binstall-publisher");
    fs::create_dir_all(&target_base)
        .map_err(|e| Error(format!("cannot create {}: {e}", target_base.display())))?;
    let fresh = tempfile::Builder::new()
        .prefix("build-")
        .tempdir_in(&target_base)
        .map_err(|e| Error(format!("cannot create fresh build directory: {e}")))?;
    let target_dir = fresh.keep();
    println!("Fresh build directory: {}", target_dir.display());
    let toolchain = executable("rustup")
        .and_then(|_| {
            runner
                .text("rustup", &["show", "active-toolchain"], root)
                .ok()
        })
        .and_then(|line| line.split_whitespace().next().map(str::to_owned));
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
    for target in &config.targets {
        println!("Building {target}");
        let args = cross_arguments(project, config, target, &target_dir);
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        runner.output_with("cross", &borrowed, root, &environment, &[])?;
        let mut binaries = Vec::new();
        for name in &project.binaries {
            let binary = target_dir
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
    }
    Ok(outputs)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cross_command_scopes_package_and_uses_fresh_target_path() {
        let project = SelectedPackage {
            name: "cli".into(),
            version: "0.1.0".into(),
            binaries: vec!["cli".into(), "helper".into()],
            manifest: PathBuf::from("Cargo.toml"),
            repository: None,
            target_directory: PathBuf::from("target"),
        };
        let config = Config {
            targets: vec!["x86_64-pc-windows-gnu".into()],
            profile: "release".into(),
            locked: true,
            tag_prefix: "v".into(),
            token_env: "GITEA_TOKEN".into(),
            package_format: crate::config::PackageFormat::Tgz,
        };
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
    fn cross_hooks_are_rejected() {
        let doc: toml::Value =
            toml::from_str("[target.x86_64-unknown-linux-musl]\npre-build=['echo unsafe']")
                .unwrap();
        assert!(reject_hooks(&doc).is_err());
    }
}
