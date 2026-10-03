use crate::{
    build,
    config::{self, Overrides, PackageFormat},
    error::Error,
    gitea::Gitea,
    manifest,
    model::Stage,
    policy, prepare, project,
    runner::Runner,
    ui, verify,
};
use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(about = "Build and publish Rust binary releases to Gitea", version)]
pub struct Cli {
    /// Local directory or Git URL.
    pub source: String,
    #[arg(long)]
    pub package: Option<String>,
    #[arg(long, short)]
    pub tag: Option<String>,
    #[arg(long)]
    pub tag_prefix: Option<String>,
    #[arg(long)]
    pub target: Vec<String>,
    #[arg(long)]
    pub profile: Option<String>,
    #[arg(long, conflicts_with = "unlocked")]
    pub locked: bool,
    #[arg(long)]
    pub unlocked: bool,
    #[arg(long)]
    pub dry_run: bool,
    /// Add or repair supported Cargo release metadata, including cargo-binstall metadata.
    #[arg(long)]
    pub fix_metadata: bool,
    #[arg(long)]
    pub skip_tests: bool,
    #[arg(long)]
    pub skip_binstall_test: bool,
    #[arg(long)]
    pub draft: bool,
    #[arg(long)]
    pub prerelease: bool,
    #[arg(long)]
    pub replace_assets: bool,
    /// Ignore a matching completed build and compile every release target again.
    #[arg(long)]
    pub recompile: bool,
    /// Release notes text.
    #[arg(long, short = 'm', conflicts_with = "message_file")]
    pub message: Option<String>,
    /// Read release notes from a UTF-8 file.
    #[arg(long, value_name = "PATH", conflicts_with = "message")]
    pub message_file: Option<PathBuf>,
    #[arg(long)]
    pub keep_workdir: bool,
    #[arg(long)]
    pub verbose: bool,
    #[arg(long, value_enum)]
    pub stop_after: Option<Stage>,
    /// Build and package local artifacts without creating or uploading a release.
    #[arg(
        long,
        conflicts_with_all = ["dry_run", "stop_after", "prepare_repo"]
    )]
    pub compile_only: bool,
    #[arg(long)]
    pub prepare_repo: bool,
    #[arg(long)]
    pub commit_preparation: bool,
    #[arg(long)]
    pub create_tag: bool,
    #[arg(long)]
    pub push_preparation: bool,
    #[arg(long)]
    pub cross_image_tag: Option<String>,
    #[arg(long)]
    pub repository_url: Option<String>,
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Release tgz archives, raw binaries, or both.
    #[arg(long, value_enum)]
    pub artifact_format: Option<PackageFormat>,
}

pub fn run(mut args: Cli) -> Result<(), Error> {
    if args.compile_only {
        args.stop_after = Some(Stage::Package);
    }
    if (args.commit_preparation
        || args.create_tag
        || args.push_preparation
        || args.cross_image_tag.is_some())
        && !args.prepare_repo
    {
        return Err(Error("Preparation options require --prepare-repo.".into()));
    }
    if args.prepare_repo && args.stop_after.is_some() {
        return Err(Error(
            "--prepare-repo is a separate mode; omit --stop-after.".into(),
        ));
    }
    let release_message = match args.message_file.as_deref() {
        Some(path) => Some(std::fs::read_to_string(path).map_err(|e| {
            Error(format!(
                "cannot read release message file {}: {e}",
                path.display()
            ))
        })?),
        None => args.message.clone(),
    };
    let acquisition_runner = Runner::new("GITEA_TOKEN").with_verbose(args.verbose);
    let checkout = project::acquire_project(
        &args.source,
        if args.prepare_repo {
            None
        } else {
            args.tag.as_deref()
        },
        args.keep_workdir || args.prepare_repo,
        &acquisition_runner,
    )?;
    let config = config::load(
        args.config.as_deref(),
        &checkout.root,
        Overrides {
            targets: args.target,
            profile: args.profile,
            unlocked: args.unlocked,
            tag_prefix: args.tag_prefix,
            package_format: args.artifact_format,
        },
    )?;
    let runner = Runner::new(&config.token_env).with_verbose(args.verbose);
    let root = checkout
        .root
        .to_str()
        .ok_or_else(|| Error("Project path is not UTF-8.".into()))?;
    let selected = project::inspect(root, args.package.as_deref(), &runner)?;
    let publishing = !args.prepare_repo
        && !args.dry_run
        && args
            .stop_after
            .is_none_or(|stage| stage.order() >= Stage::Release.order());
    let tag = policy::expected_tag(&selected.version, &config.tag_prefix);
    if args.tag.as_ref().is_some_and(|provided| provided != &tag) {
        return Err(Error(format!(
            "Tag {:?} does not match package version; expected {tag:?}.",
            args.tag
        )));
    }
    ui::detail("Package", &selected.name);
    ui::detail("Version", &selected.version);
    ui::detail("Binaries", selected.binaries.join(", "));
    ui::detail("Manifest", selected.manifest.display());
    ui::detail("Tag", &tag);
    if args
        .stop_after
        .is_some_and(|stage| stage.order() <= Stage::Inspect.order())
    {
        return Ok(());
    }
    if publishing
        && !runner
            .text(
                "git",
                &["status", "--porcelain", "--untracked-files=normal"],
                &checkout.root,
            )?
            .is_empty()
    {
        return Err(Error(
            "Publication requires a clean checkout before metadata repair.".into(),
        ));
    }
    let candidate = args
        .repository_url
        .as_deref()
        .or(selected.repository.as_deref())
        .map(str::to_owned)
        .or_else(|| project::origin_url(&checkout.root, &runner))
        .ok_or_else(|| {
            Error(
                "package.repository is missing; provide --repository-url and --fix-metadata."
                    .into(),
            )
        })?;
    let normalized_candidate = project::normalize_repository(&candidate)?;
    let repository = config.resolve_repository(&candidate)?;
    if repository != normalized_candidate {
        println!("Repository fallback: {normalized_candidate} -> {repository}");
    }
    if publishing {
        if let Some(origin) = project::origin_url(&checkout.root, &runner) {
            if !config.repositories_match(&origin, &repository)? {
                return Err(Error(
                    "Git origin differs from the release repository and no configured fallback maps them together.".into(),
                ));
            }
        }
    }
    let issues = manifest::changes(
        &selected.manifest,
        &repository,
        selected.repository.as_deref(),
        &config.tag_prefix,
        config.package_format.archives(),
    )?;
    if issues.is_empty() {
        if config.package_format.archives() {
            ui::success("Binstall metadata valid");
        } else {
            ui::success("Release metadata valid");
        }
    }
    for issue in &issues {
        let actual = if issue.actual.is_some() {
            "present"
        } else {
            "missing"
        };
        println!("{}: {actual} -> {:?}", issue.key, issue.expected);
    }
    if args.prepare_repo {
        return prepare::run(
            &checkout.root,
            &selected,
            &config,
            &repository,
            prepare::Options {
                tag: &tag,
                image_tag: args.cross_image_tag.as_deref(),
                dry_run: args.dry_run,
                commit: args.commit_preparation,
                create_tag: args.create_tag,
                push: args.push_preparation,
            },
            &runner,
        );
    }
    if !issues.is_empty() && args.fix_metadata && !args.dry_run {
        manifest::repair(&selected.manifest, &issues, true)?;
        println!("Metadata repaired locally. Commit and push explicitly if desired.");
    }
    if !issues.is_empty() && !args.fix_metadata && !args.dry_run {
        return Err(Error(
            "Metadata requires repair. Rerun with --fix-metadata.".into(),
        ));
    }
    if args.stop_after == Some(Stage::Metadata) {
        return Ok(());
    }
    for target in &config.targets {
        if config.package_format.archives() {
            println!(
                "{target} archive: {} ({})",
                policy::archive_name(&selected.name, target, &selected.version),
                selected
                    .binaries
                    .iter()
                    .map(|binary| policy::binary_filename(binary, target))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if config.package_format.binaries() {
            for binary in &selected.binaries {
                println!(
                    "{target} binary: {}",
                    policy::binary_asset_name(binary, target, &selected.version)
                );
            }
        }
    }
    if config.package_format.archives() {
        for (key, value) in policy::binstall_metadata(&config.tag_prefix) {
            println!("Binstall {key}: {value}");
        }
    }
    ui::detail(
        "Release destination",
        format_args!("{repository}/releases/tag/{tag}"),
    );
    let needs_cross = args.stop_after != Some(Stage::Verify);
    let needs_binstall = config.package_format.archives()
        && !args.skip_binstall_test
        && args
            .stop_after
            .is_none_or(|stage| stage.order() >= Stage::Binstall.order());
    let publication = if publishing {
        project::validate_checkout_tag(&checkout.root, &tag, &runner)?;
        let api = Gitea::new(&repository, &config.token_env)?;
        let private = api.repository_private()?;
        if private && needs_binstall {
            return Err(Error("This repository is private; cargo-binstall cannot verify its private release downloads. Rerun with --skip-binstall-test to verify uploaded assets with Gitea authentication.".into()));
        }
        Some((api, private))
    } else {
        None
    };
    let (mut problems, engine) = build::prerequisites(
        &checkout.root,
        &config,
        needs_cross,
        needs_binstall,
        &runner,
    );
    if args.dry_run {
        if let Err(error) = project::validate_checkout_tag(&checkout.root, &tag, &runner) {
            problems.push(error.to_string());
        }
    }
    for problem in &problems {
        ui::warning(format_args!("Prerequisite: {problem}"));
    }
    if args.dry_run {
        ui::success("Dry run complete: no manifest edits, builds or release mutations");
        if !issues.is_empty() || !problems.is_empty() {
            return Err(Error(
                "Dry run found the metadata/prerequisite issues reported above.".into(),
            ));
        }
        return Ok(());
    }
    if !problems.is_empty() {
        return Err(Error(
            "Resolve the prerequisite issues above before building.".into(),
        ));
    }
    build::verify_source(&checkout.root, &selected, &config, args.skip_tests, &runner)?;
    if args.stop_after == Some(Stage::Verify) {
        return Ok(());
    }
    let engine = engine.ok_or_else(|| Error("No usable Cross container backend.".into()))?;
    let commit = project::head_commit(&checkout.root, &runner)?;
    let checkout_clean = runner
        .text(
            "git",
            &["status", "--porcelain", "--untracked-files=normal"],
            &checkout.root,
        )?
        .is_empty();
    let outputs = build::build(
        &checkout.root,
        &selected,
        &config,
        &engine,
        checkout_clean.then_some(commit.as_str()),
        args.recompile,
        &runner,
    )?;
    if args.stop_after == Some(Stage::Build) {
        return Ok(());
    }
    let (artifacts, sums) = crate::package::package(
        &checkout.root,
        &selected.name,
        &selected.version,
        &outputs,
        config.package_format,
    )?;
    ui::detail(
        "Artifacts",
        sums.parent().map_or_else(
            || sums.display().to_string(),
            |path| path.display().to_string(),
        ),
    );
    for artifact in &artifacts {
        ui::success(format_args!(
            "Packaged {}: {}",
            artifact.target,
            artifact.path.display()
        ));
    }
    if args.stop_after == Some(Stage::Package) {
        return Ok(());
    }
    let (api, private) =
        publication.ok_or_else(|| Error("Publication client was not initialized.".into()))?;
    let release = api.ensure_release(
        &tag,
        &commit,
        args.draft,
        args.prerelease,
        release_message.as_deref(),
    )?;
    ui::success(format_args!("Release ready: {tag}"));
    if args.stop_after == Some(Stage::Release) {
        return Ok(());
    }
    let mut paths: Vec<&std::path::Path> = artifacts.iter().map(|a| a.path.as_path()).collect();
    paths.push(sums.as_path());
    api.upload(release.id, &paths, args.replace_assets)?;
    if args.stop_after == Some(Stage::Upload) {
        return Ok(());
    }
    if release.draft {
        return Err(Error("Draft uploaded. Public verification requires publishing the draft; use --stop-after upload for draft workflows.".into()));
    }
    verify::verify_published(
        &repository,
        &tag,
        &artifacts,
        &sums,
        if private {
            Some(api.download_client())
        } else {
            None
        },
    )?;
    if config.package_format.archives() && !args.skip_binstall_test {
        verify::verify_binstall(&checkout.root, &selected, &config.targets, &runner)?;
    }
    ui::success("Release completed and published artifacts verified");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    fn write(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn dry_run_never_edits_and_repair_only_touches_selected_member() {
        let temp = tempfile::tempdir().unwrap();
        write(
            temp.path(),
            "config.toml",
            "[build]\ntargets=['x86_64-unknown-linux-musl']\n",
        );
        write(
            temp.path(),
            "Cargo.toml",
            "[workspace]\nmembers=['one','two']\nresolver='2'\n",
        );
        for name in ["one", "two"] {
            write(temp.path(), &format!("{name}/Cargo.toml"), &format!("# {name}\n[package]\nname='{name}'\nversion='0.1.0'\nedition='2021'\nrepository='https://example.com/owner/{name}'\n"));
            write(
                temp.path(),
                &format!("{name}/src/main.rs"),
                "fn main() {}\n",
            );
        }
        let root = temp.path().to_str().unwrap();
        let config = temp
            .path()
            .join("config.toml")
            .to_string_lossy()
            .into_owned();
        let one = temp.path().join("one/Cargo.toml");
        let two = temp.path().join("two/Cargo.toml");
        let original_one = fs::read_to_string(&one).unwrap();
        let original_two = fs::read_to_string(&two).unwrap();
        let args = Cli::parse_from([
            "rust_realease",
            root,
            "--config",
            &config,
            "--package",
            "one",
            "--fix-metadata",
            "--dry-run",
            "--stop-after",
            "metadata",
        ]);
        run(args).unwrap();
        assert_eq!(fs::read_to_string(&one).unwrap(), original_one);
        let args = Cli::parse_from([
            "rust_realease",
            root,
            "--config",
            &config,
            "--package",
            "one",
            "--fix-metadata",
            "--stop-after",
            "metadata",
        ]);
        run(args).unwrap();
        assert_ne!(fs::read_to_string(&one).unwrap(), original_one);
        assert_eq!(fs::read_to_string(&two).unwrap(), original_two);
        let args = Cli::parse_from([
            "rust_realease",
            root,
            "--config",
            &config,
            "--package",
            "one",
            "--stop-after",
            "metadata",
        ]);
        run(args).unwrap();
    }

    #[test]
    fn compile_only_is_an_explicit_package_stage_without_release_options() {
        let args = Cli::try_parse_from(["rust_realease", ".", "--compile-only"]).unwrap();
        assert!(args.compile_only);
        assert_eq!(args.stop_after, None);

        for conflicting in ["--dry-run", "--stop-after", "--prepare-repo"] {
            let mut arguments = vec!["rust_realease", ".", "--compile-only", conflicting];
            if conflicting == "--stop-after" {
                arguments.push("package");
            }
            assert!(Cli::try_parse_from(arguments).is_err());
        }
    }

    #[test]
    fn release_message_inputs_and_recompile_flag_parse() {
        let args = Cli::try_parse_from([
            "rust_realease",
            ".",
            "--recompile",
            "-m",
            "Changes in this release",
        ])
        .unwrap();
        assert!(args.recompile);
        assert_eq!(args.message.as_deref(), Some("Changes in this release"));
        assert!(Cli::try_parse_from([
            "rust_realease",
            ".",
            "--message",
            "inline",
            "--message-file",
            "notes.md",
        ])
        .is_err());
    }
}
