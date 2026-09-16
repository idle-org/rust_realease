use crate::{
    build,
    config::{self, Overrides},
    error::Error,
    manifest,
    model::Stage,
    policy, project,
    runner::Runner,
};
use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(about = "Inspect and prepare Rust release inputs", version)]
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
    #[arg(long)]
    pub keep_workdir: bool,
    #[arg(long)]
    pub verbose: bool,
    #[arg(long, value_enum)]
    pub stop_after: Option<Stage>,
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
}

pub fn run(args: Cli) -> Result<(), Error> {
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
    let config = config::load(
        args.config.as_deref(),
        Overrides {
            targets: args.target,
            profile: args.profile,
            unlocked: args.unlocked,
            tag_prefix: args.tag_prefix,
        },
    )?;
    let runner = Runner::new(&config.token_env).with_verbose(args.verbose);
    let checkout = project::acquire_project(
        &args.source,
        args.tag.as_deref(),
        args.keep_workdir || args.prepare_repo,
        &runner,
    )?;
    let root = checkout
        .root
        .to_str()
        .ok_or_else(|| Error("Project path is not UTF-8.".into()))?;
    let selected = project::inspect(root, args.package.as_deref(), &runner)?;
    let tag = policy::expected_tag(&selected.version, &config.tag_prefix);
    if args.tag.as_ref().is_some_and(|provided| provided != &tag) {
        return Err(Error(format!(
            "Tag {:?} does not match package version; expected {tag:?}.",
            args.tag
        )));
    }
    println!("Package: {}", selected.name);
    println!("Version: {}", selected.version);
    println!("Binary: {}", selected.binary);
    println!("Manifest: {}", selected.manifest.display());
    println!("Tag: {tag}");
    if args
        .stop_after
        .is_some_and(|stage| stage.order() <= Stage::Inspect.order())
    {
        return Ok(());
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
    let repository = project::normalize_repository(&candidate)?;
    let issues = manifest::changes(&selected.manifest, &repository, &config.tag_prefix)?;
    if issues.is_empty() {
        println!("Binstall metadata: valid");
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
        return Err(Error(
            "Rust repository preparation is still being ported; use release.sh for preparation."
                .into(),
        ));
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
        println!(
            "{target}: {} ({})",
            policy::archive_name(&selected.name, target, &selected.version),
            policy::binary_filename(&selected.binary, target)
        );
    }
    for (key, value) in policy::binstall_metadata(&config.tag_prefix) {
        println!("Binstall {key}: {value}");
    }
    println!("Release destination: {repository}/releases/tag/{tag}");
    let needs_cross = args.stop_after != Some(Stage::Verify);
    let needs_binstall = !args.skip_binstall_test
        && args
            .stop_after
            .is_none_or(|stage| stage.order() >= Stage::Binstall.order());
    let (problems, engine) = build::prerequisites(
        &checkout.root,
        &config,
        needs_cross,
        needs_binstall,
        &runner,
    );
    for problem in &problems {
        println!("Prerequisite: {problem}");
    }
    if args.dry_run {
        println!("Dry run complete: no manifest edits, builds or release mutations.");
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
    let outputs = build::build(&checkout.root, &selected, &config, &engine, &runner)?;
    if args.stop_after == Some(Stage::Build) {
        return Ok(());
    }
    let (artifacts, sums) = crate::package::package(
        &checkout.root,
        &selected.name,
        &selected.version,
        &selected.binary,
        &outputs,
    )?;
    println!(
        "Artifacts: {}",
        sums.parent().map_or_else(
            || sums.display().to_string(),
            |path| path.display().to_string()
        )
    );
    for artifact in &artifacts {
        println!(
            "Packaged {}: {}/{}",
            artifact.target, artifact.root, artifact.binary
        );
    }
    if args.stop_after == Some(Stage::Package) {
        return Ok(());
    }
    Err(Error(
        "Rust Gitea publication is still being ported; use release.sh for publication.".into(),
    ))
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
}
