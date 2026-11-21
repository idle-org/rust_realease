use crate::{
    build,
    config::{self, Overrides, PackageFormat, ReleaseEndpoint, ReleaseProvider},
    error::Error,
    manifest,
    model::Stage,
    policy, prepare, project,
    release::ReleaseClient,
    runner::Runner,
    ui, verify,
};
use clap::Parser;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Parser)]
#[command(
    about = "Build and publish Rust binary releases to Gitea and GitHub",
    version
)]
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
    /// Enable Cargo features for source checks and every release backend.
    #[arg(long, value_delimiter = ',')]
    pub features: Vec<String>,
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
    /// Read one plaintext token or a JSON object of endpoint tokens from a UTF-8 file.
    #[arg(long, value_name = "PATH")]
    pub token_file: Option<PathBuf>,
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
    let supplied_release_message = match args.message_file.as_deref() {
        Some(path) => Some(std::fs::read_to_string(path).map_err(|e| {
            Error(format!(
                "cannot read release message file {}: {e}",
                path.display()
            ))
        })?),
        None => args.message.clone(),
    };
    let file_tokens = args
        .token_file
        .as_deref()
        .map(read_token_file)
        .transpose()?;
    let acquisition_runner = Runner::new("GITEA_TOKEN")
        .with_secret(file_tokens.as_ref().and_then(TokenFile::plain))
        .with_verbose(args.verbose);
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
            features: args.features,
        },
    )?;
    let runner = Runner::new(&config.token_env)
        .with_token_envs(
            config
                .release_endpoints
                .iter()
                .map(|endpoint| endpoint.token_env.clone()),
        )
        .with_secret(file_tokens.as_ref().and_then(TokenFile::plain))
        .with_verbose(args.verbose);
    let root = checkout
        .root
        .to_str()
        .ok_or_else(|| Error("Project path is not UTF-8.".into()))?;
    let selected = project::inspect(root, args.package.as_deref(), &runner)?;
    crate::bundle::plan(&config, &selected, &selected.workspace_root)?;
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
    let mut endpoints = config.release_endpoints.clone();
    if args.repository_url.is_some() && !endpoints.is_empty() {
        return Err(Error(
            "--repository-url cannot be combined with configured release.endpoints.".into(),
        ));
    }
    let repository = if let Some(primary) = endpoints.first() {
        primary.repository.clone()
    } else {
        config.resolve_repository(&candidate)?
    };
    if endpoints.is_empty() {
        let provider = if url::Url::parse(&repository)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .as_deref()
            == Some("github.com")
        {
            ReleaseProvider::Github
        } else {
            ReleaseProvider::Gitea
        };
        endpoints.push(ReleaseEndpoint {
            name: "default".into(),
            provider,
            repository: repository.clone(),
            token_env: match provider {
                ReleaseProvider::Gitea => config.token_env.clone(),
                ReleaseProvider::Github => "GITHUB_TOKEN".into(),
            },
        });
    }
    if repository != normalized_candidate {
        if config.release_endpoints.is_empty() {
            println!("Repository fallback: {normalized_candidate} -> {repository}");
        } else {
            println!("Primary release repository: {repository}");
        }
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
        let preparation_token = match file_tokens.as_ref() {
            Some(TokenFile::Plain(token)) => Some(token.as_str()),
            Some(TokenFile::Named(tokens)) => tokens.get(&endpoints[0].name).map(String::as_str),
            None => None,
        };
        return prepare::run(
            &checkout.root,
            &selected,
            &config,
            &repository,
            prepare::Options {
                tag: &tag,
                image_tag: args.cross_image_tag.as_deref(),
                token: preparation_token,
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
        if build::is_apple(target) {
            println!(
                "{target} build backend: cargo-zigbuild container {} (SDKROOT={}, MACOSX_DEPLOYMENT_TARGET={})",
                config.apple.image, config.apple.sdk_root, config.apple.deployment_target
            );
        } else {
            println!("{target} build backend: Cross");
        }
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
    for endpoint in &endpoints {
        ui::detail(
            "Release destination",
            format_args!(
                "{} ({}) {}/releases/tag/{tag}",
                endpoint.name, endpoint.provider, endpoint.repository
            ),
        );
    }
    let needs_cross = args.stop_after != Some(Stage::Verify);
    let needs_binstall = config.package_format.archives()
        && !args.skip_binstall_test
        && args
            .stop_after
            .is_none_or(|stage| stage.order() >= Stage::Binstall.order());
    let publication = if publishing {
        project::validate_checkout_tag(&checkout.root, &tag, &runner)?;
        validate_token_keys(file_tokens.as_ref(), &endpoints)?;
        let mut clients = Vec::new();
        for endpoint in &endpoints {
            let token = token_for_endpoint(file_tokens.as_ref(), endpoint, endpoints.len())?;
            let api = ReleaseClient::new(endpoint, &token)?;
            let private = api.repository_private()?;
            clients.push((endpoint.clone(), api, private));
        }
        if clients.first().is_some_and(|(_, _, private)| *private) && needs_binstall {
            return Err(Error("The primary release repository is private; cargo-binstall cannot verify its private release downloads. Rerun with --skip-binstall-test to verify uploaded assets with authentication.".into()));
        }
        Some(clients)
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
    let engine =
        engine.ok_or_else(|| Error("No usable Docker or Podman container backend.".into()))?;
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
    let (artifacts, sums) = if config.bundles.is_empty() {
        crate::package::package(
            &checkout.root,
            &selected.name,
            &selected.version,
            &outputs,
            config.package_format,
        )?
    } else {
        crate::bundle::assemble(
            &selected.workspace_root,
            &selected,
            &config,
            &outputs,
            &commit,
            !checkout_clean,
            &runner,
        )?
    };
    for artifact in &artifacts {
        if matches!(
            artifact.contents,
            crate::package::ArtifactContents::Archive { .. }
                | crate::package::ArtifactContents::Bundle { .. }
        ) {
            verify::verify_archive(&artifact.path, artifact)?;
        }
        if build::is_apple(&artifact.target) {
            verify::verify_apple_artifact(
                &artifact.path,
                artifact,
                &config.apple.deployment_target,
            )?;
        }
    }
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
    for target in &config.targets {
        if build::is_apple(target) {
            println!("{target} checks: Mach-O architecture, macOS deployment minimum {}, system dynamic dependencies, archive layout and checksum passed; runtime skipped (no compatible macOS runner)", config.apple.deployment_target);
        }
    }
    if args.stop_after == Some(Stage::Package) {
        return Ok(());
    }
    let clients =
        publication.ok_or_else(|| Error("Publication client was not initialized.".into()))?;
    let release_message =
        release_message_with_macos_workaround(supplied_release_message.as_deref(), &artifacts)?;
    let mut releases = Vec::new();
    for (endpoint, api, private) in clients {
        let release = api.ensure_release(
            &tag,
            &commit,
            args.draft,
            args.prerelease,
            release_message.as_deref(),
            supplied_release_message.is_some(),
        )?;
        ui::success(format_args!("Release ready on {}: {tag}", endpoint.name));
        releases.push((endpoint, api, private, release));
    }
    if args.stop_after == Some(Stage::Release) {
        return Ok(());
    }
    let mut paths: Vec<&std::path::Path> = artifacts.iter().map(|a| a.path.as_path()).collect();
    paths.push(sums.as_path());
    for (endpoint, api, _, release) in &releases {
        api.upload(release.id, &paths, args.replace_assets)?;
        ui::success(format_args!("Assets uploaded to {}", endpoint.name));
    }
    if args.stop_after == Some(Stage::Upload) {
        return Ok(());
    }
    if releases.iter().any(|(_, _, _, release)| release.draft) {
        return Err(Error("A draft was uploaded. Public verification requires publishing every draft; use --stop-after upload for draft workflows.".into()));
    }
    for (endpoint, api, private, release) in &releases {
        let asset_urls = api.asset_urls(release.id)?;
        let authenticated = if *private || asset_urls.is_some() {
            Some(api.download_client())
        } else {
            None
        };
        if let Some(asset_urls) = asset_urls.as_ref() {
            verify::verify_published_with_asset_urls(
                &endpoint.repository,
                &tag,
                &artifacts,
                &sums,
                authenticated,
                Some(asset_urls),
            )?;
        } else {
            verify::verify_published(&endpoint.repository, &tag, &artifacts, &sums, authenticated)?;
        }
        ui::success(format_args!(
            "Published assets verified on {}",
            endpoint.name
        ));
    }
    if config.package_format.archives() && !args.skip_binstall_test {
        verify::verify_binstall(&checkout.root, &selected, &config.targets, &runner)?;
    }
    ui::success("Release completed and published artifacts verified");
    Ok(())
}

fn release_message_with_macos_workaround(
    supplied: Option<&str>,
    artifacts: &[crate::package::Artifact],
) -> Result<Option<String>, Error> {
    const HEADING: &str = "## macOS Workaround";
    if supplied.is_some_and(|message| message.contains(HEADING)) {
        return Ok(supplied.map(str::to_owned));
    }
    let Some(workaround) = macos_workaround(artifacts)? else {
        return Ok(supplied.map(str::to_owned));
    };
    Ok(Some(
        match supplied.map(str::trim_end).filter(|s| !s.is_empty()) {
            Some(message) => format!("{message}\n\n{workaround}"),
            None => workaround,
        },
    ))
}

fn macos_workaround(artifacts: &[crate::package::Artifact]) -> Result<Option<String>, Error> {
    use crate::{bundle::Format, package::ArtifactContents};

    let apple = artifacts
        .iter()
        .filter(|artifact| build::is_apple(&artifact.target))
        .collect::<Vec<_>>();
    if apple.is_empty() {
        return Ok(None);
    }
    let has_bundle = apple
        .iter()
        .any(|artifact| matches!(artifact.contents, ArtifactContents::Bundle { .. }));
    let has_archive = apple
        .iter()
        .any(|artifact| matches!(artifact.contents, ArtifactContents::Archive { .. }));
    let selected = apple.into_iter().filter(|artifact| {
        if has_bundle {
            matches!(artifact.contents, ArtifactContents::Bundle { .. })
        } else if has_archive {
            matches!(artifact.contents, ArtifactContents::Archive { .. })
        } else {
            matches!(artifact.contents, ArtifactContents::Binary)
        }
    });
    let mut sections = Vec::new();
    for artifact in selected {
        let filename = artifact
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error("Invalid macOS artifact filename".into()))?;
        let platform = if artifact.target.starts_with("aarch64-") {
            "Apple Silicon"
        } else {
            "Intel Mac"
        };
        let commands = match &artifact.contents {
            ArtifactContents::Bundle {
                root,
                format,
                entries,
            } => archive_workaround(
                filename,
                root,
                *format,
                entries
                    .iter()
                    .filter(|entry| entry.mode & 0o111 != 0)
                    .map(|entry| entry.destination.as_str()),
            ),
            ArtifactContents::Archive { root, binaries } => archive_workaround(
                filename,
                root,
                Format::Tgz,
                binaries.iter().map(String::as_str),
            ),
            ArtifactContents::Binary => {
                let quoted = shell_quote(filename);
                format!(
                    "downloads=\"$HOME/Downloads\"\nexecutable=\"$downloads\"/{quoted}\nxattr -d com.apple.quarantine \"$executable\" 2>/dev/null || true\nchmod +x \"$executable\"\n\"$executable\""
                )
            }
        };
        sections.push(format!(
            "For **{platform}** (`{filename}`), run in Terminal:\n```sh\n{commands}\n```"
        ));
    }
    if sections.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "## macOS Workaround\n\nIf macOS blocks these downloaded, unsigned executables, remove their quarantine attribute manually after verifying and trusting the release. Download the indicated archive into `~/Downloads`; the commands extract it there, remove quarantine recursively from everything in the extracted directory, restore executable permissions, and start the first executable.\n\n{}",
        sections.join("\n\n")
    )))
}

fn archive_workaround<'a>(
    filename: &str,
    root: &str,
    format: crate::bundle::Format,
    executables: impl Iterator<Item = &'a str>,
) -> String {
    let filename = shell_quote(filename);
    let root = shell_quote(root);
    let executables = executables.collect::<Vec<_>>();
    let extraction = match format {
        crate::bundle::Format::Tgz => "tar -xzf \"$archive\" -C \"$downloads\"",
        crate::bundle::Format::Zip => "ditto -x -k \"$archive\" \"$downloads\"",
    };
    let paths = executables
        .iter()
        .map(|path| format!("\"$bundle\"/{}", shell_quote(path)))
        .collect::<Vec<_>>();
    let chmod = if paths.is_empty() {
        String::new()
    } else {
        format!("\nchmod +x {}", paths.join(" "))
    };
    let launch = paths
        .iter()
        .zip(executables.iter())
        .find(|(_, executable)| {
            executable
                .rsplit('/')
                .next()
                .is_some_and(|name| name.ends_with("-gui"))
        })
        .map(|(path, _)| path)
        .or_else(|| paths.first())
        .map_or_else(String::new, |path| format!("\n{path}"));
    format!(
        "downloads=\"$HOME/Downloads\"\narchive=\"$downloads\"/{filename}\n{extraction}\nbundle=\"$downloads\"/{root}\nxattr -rd com.apple.quarantine \"$bundle\"{chmod}{launch}"
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[derive(Debug)]
enum TokenFile {
    Plain(String),
    Named(BTreeMap<String, String>),
}

impl TokenFile {
    fn plain(&self) -> Option<&str> {
        match self {
            Self::Plain(token) => Some(token),
            Self::Named(_) => None,
        }
    }
}

fn read_token_file(path: &std::path::Path) -> Result<TokenFile, Error> {
    if !path.is_file() {
        return Err(Error(format!(
            "token file is not a regular file: {}",
            path.display()
        )));
    }
    let source = std::fs::read_to_string(path)
        .map_err(|e| Error(format!("cannot read token file {}: {e}", path.display())))?;
    let trimmed = source.trim_end_matches(['\r', '\n']);
    if trimmed.trim_start().starts_with('{') {
        let tokens: BTreeMap<String, String> = serde_json::from_str(trimmed)
            .map_err(|e| Error(format!("invalid JSON token file {}: {e}", path.display())))?;
        if tokens.is_empty()
            || tokens
                .values()
                .any(|token| token.is_empty() || token.contains(['\r', '\n']))
        {
            return Err(Error(format!(
                "JSON token file must map endpoint names to nonempty single-line tokens: {}",
                path.display()
            )));
        }
        return Ok(TokenFile::Named(tokens));
    }
    let mut token = source;
    while token.ends_with(['\r', '\n']) {
        token.pop();
    }
    if token.is_empty() {
        return Err(Error(format!("token file is empty: {}", path.display())));
    }
    if token.contains(['\r', '\n']) {
        return Err(Error(format!(
            "plaintext token file must contain exactly one token: {}",
            path.display()
        )));
    }
    Ok(TokenFile::Plain(token))
}

fn token_for_endpoint(
    file: Option<&TokenFile>,
    endpoint: &ReleaseEndpoint,
    endpoint_count: usize,
) -> Result<String, Error> {
    match file {
        Some(TokenFile::Plain(token)) if endpoint_count == 1 => Ok(token.clone()),
        Some(TokenFile::Plain(_)) => Err(Error(
            "A plaintext token file can only be used with one release endpoint; use a JSON object keyed by endpoint name."
                .into(),
        )),
        Some(TokenFile::Named(tokens)) => tokens.get(&endpoint.name).cloned().ok_or_else(|| {
            Error(format!(
                "Token file has no token for release endpoint {:?}.",
                endpoint.name
            ))
        }),
        None => std::env::var(&endpoint.token_env)
            .ok()
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                Error(format!(
                    "Set {} before publishing endpoint {:?}.",
                    endpoint.token_env, endpoint.name
                ))
            }),
    }
}

fn validate_token_keys(
    file: Option<&TokenFile>,
    endpoints: &[ReleaseEndpoint],
) -> Result<(), Error> {
    let Some(TokenFile::Named(tokens)) = file else {
        return Ok(());
    };
    if let Some(unknown) = tokens
        .keys()
        .find(|name| !endpoints.iter().any(|endpoint| &endpoint.name == *name))
    {
        return Err(Error(format!(
            "Token file contains unknown release endpoint {unknown:?}."
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bundle::{Entry, Format},
        package::{Artifact, ArtifactContents},
    };
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

    #[test]
    fn macos_workaround_uses_actual_bundle_and_quarantines_its_complete_tree() {
        let artifacts = vec![
            Artifact {
                target: "aarch64-apple-darwin".into(),
                path: "dist/pptx-reporter-aarch64-apple-darwin".into(),
                contents: ArtifactContents::Binary,
                sha256: "0".repeat(64),
            },
            Artifact {
                target: "aarch64-apple-darwin".into(),
                path: "dist/pptx-reporter-portable-aarch64-apple-darwin-v0.1.2.tgz".into(),
                contents: ArtifactContents::Bundle {
                    root: "pptx-reporter-portable-aarch64-apple-darwin-v0.1.2".into(),
                    format: Format::Tgz,
                    entries: vec![
                        Entry {
                            destination: "pptx-reporter".into(),
                            sha256: "1".repeat(64),
                            mode: 0o755,
                        },
                        Entry {
                            destination: "pptx-reporter-gui".into(),
                            sha256: "2".repeat(64),
                            mode: 0o755,
                        },
                        Entry {
                            destination: "resources/font.ttf".into(),
                            sha256: "3".repeat(64),
                            mode: 0o644,
                        },
                    ],
                },
                sha256: "4".repeat(64),
            },
        ];
        let message = release_message_with_macos_workaround(Some("Release notes"), &artifacts)
            .unwrap()
            .unwrap();
        assert!(message.starts_with("Release notes\n\n## macOS Workaround"));
        assert!(message.contains("tar -xzf \"$archive\" -C \"$downloads\""));
        assert!(message.contains("xattr -rd com.apple.quarantine \"$bundle\""));
        assert!(message
            .contains("chmod +x \"$bundle\"/'pptx-reporter' \"$bundle\"/'pptx-reporter-gui'"));
        assert!(message.contains("\n\"$bundle\"/'pptx-reporter-gui'\n```"));
        assert!(!message.contains("executable=\"$downloads\""));
    }

    #[test]
    fn macos_workaround_is_not_duplicated_or_added_to_other_targets() {
        let linux = Artifact {
            target: "x86_64-unknown-linux-gnu".into(),
            path: "dist/demo.tgz".into(),
            contents: ArtifactContents::Archive {
                root: "demo".into(),
                binaries: vec!["demo".into()],
            },
            sha256: "0".repeat(64),
        };
        assert_eq!(
            release_message_with_macos_workaround(Some("Notes"), &[linux]).unwrap(),
            Some("Notes".into())
        );
        let existing = "Notes\n\n## macOS Workaround\n\nCustom instructions";
        assert_eq!(
            release_message_with_macos_workaround(Some(existing), &[]).unwrap(),
            Some(existing.into())
        );
    }

    #[test]
    fn token_file_accepts_one_token_and_trailing_newlines() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("token");
        fs::write(&path, "secret-token\r\n").unwrap();
        assert_eq!(
            read_token_file(&path).unwrap().plain(),
            Some("secret-token")
        );

        fs::write(&path, "first\nsecond\n").unwrap();
        assert!(read_token_file(&path).is_err());
        fs::write(&path, "\n").unwrap();
        assert!(read_token_file(&path).is_err());
    }

    #[test]
    fn token_file_accepts_json_tokens_keyed_by_endpoint() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tokens.json");
        fs::write(&path, r#"{"github":"github-token","mirror":"gitea-token"}"#).unwrap();
        let TokenFile::Named(tokens) = read_token_file(&path).unwrap() else {
            panic!("expected named tokens");
        };
        assert_eq!(
            tokens.get("github").map(String::as_str),
            Some("github-token")
        );
        assert_eq!(
            tokens.get("mirror").map(String::as_str),
            Some("gitea-token")
        );
    }

    #[test]
    fn token_file_option_parses_as_a_path() {
        let args =
            Cli::try_parse_from(["rust_realease", ".", "--token-file", "secrets/gitea-token"])
                .unwrap();
        assert_eq!(
            args.token_file.as_deref(),
            Some(Path::new("secrets/gitea-token"))
        );
    }

    fn endpoint(name: &str, token_env: &str) -> ReleaseEndpoint {
        ReleaseEndpoint {
            name: name.into(),
            provider: crate::config::ReleaseProvider::Gitea,
            repository: "https://example.com/team/demo".into(),
            token_env: token_env.into(),
        }
    }

    #[test]
    fn endpoint_tokens_are_selected_and_validated() {
        let primary = endpoint("primary", "RUST_RELEASE_TEST_PRIMARY_TOKEN");
        let mirror = endpoint("mirror", "RUST_RELEASE_TEST_MIRROR_TOKEN");
        let plain = TokenFile::Plain("plain-secret".into());
        assert_eq!(
            token_for_endpoint(Some(&plain), &primary, 1).unwrap(),
            "plain-secret"
        );
        assert!(token_for_endpoint(Some(&plain), &primary, 2).is_err());

        let named = TokenFile::Named(BTreeMap::from([
            ("primary".into(), "one".into()),
            ("mirror".into(), "two".into()),
        ]));
        assert_eq!(token_for_endpoint(Some(&named), &mirror, 2).unwrap(), "two");
        assert!(token_for_endpoint(Some(&TokenFile::Named(BTreeMap::new())), &primary, 1).is_err());
        assert!(validate_token_keys(Some(&named), &[primary.clone(), mirror]).is_ok());
        assert!(validate_token_keys(Some(&named), &[primary]).is_err());
        assert!(validate_token_keys(None, &[]).is_ok());
    }

    #[test]
    fn token_file_rejects_non_files_and_invalid_json_values() {
        let temp = tempfile::tempdir().unwrap();
        assert!(read_token_file(temp.path()).is_err());
        let path = temp.path().join("tokens.json");
        for invalid in [
            "{",
            "{}",
            r#"{"primary":""}"#,
            "{\"primary\":\"line\\nbreak\"}",
        ] {
            fs::write(&path, invalid).unwrap();
            assert!(read_token_file(&path).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn macos_workaround_covers_zip_archive_binary_and_shell_escaping() {
        let bundle = Artifact {
            target: "x86_64-apple-darwin".into(),
            path: "dist/demo portable.zip".into(),
            contents: ArtifactContents::Bundle {
                root: "demo's bundle".into(),
                format: Format::Zip,
                entries: vec![],
            },
            sha256: "0".repeat(64),
        };
        let archive = Artifact {
            target: "aarch64-apple-darwin".into(),
            path: "dist/demo.tgz".into(),
            contents: ArtifactContents::Archive {
                root: "demo".into(),
                binaries: vec!["demo".into()],
            },
            sha256: "1".repeat(64),
        };
        let binary = Artifact {
            target: "aarch64-apple-darwin".into(),
            path: "dist/demo-bin".into(),
            contents: ArtifactContents::Binary,
            sha256: "2".repeat(64),
        };
        let message = release_message_with_macos_workaround(None, &[bundle])
            .unwrap()
            .unwrap();
        assert!(message.contains("ditto -x -k"));
        assert!(message.contains("demo'\"'\"'s bundle"));

        let message = release_message_with_macos_workaround(None, &[archive])
            .unwrap()
            .unwrap();
        assert!(message.contains("tar -xzf"));

        let message = release_message_with_macos_workaround(None, &[binary])
            .unwrap()
            .unwrap();
        assert!(message.contains("xattr -d com.apple.quarantine \"$executable\""));
    }
}
