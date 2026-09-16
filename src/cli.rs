use crate::{
    config::{self, Overrides},
    error::Error,
    model::Stage,
    policy, project,
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
    #[arg(long, default_value = "config.toml")]
    pub config: PathBuf,
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
        &args.config,
        Overrides {
            targets: args.target,
            profile: args.profile,
            unlocked: args.unlocked,
            tag_prefix: args.tag_prefix,
        },
    )?;
    let selected = project::inspect(&args.source, args.package.as_deref())?;
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
    let _ = (
        &config.profile,
        config.locked,
        &config.token_env,
        &selected.repository,
    );
    Err(Error(
        "Rust release stages after inspect are still being ported; use release.sh for publication."
            .into(),
    ))
}
