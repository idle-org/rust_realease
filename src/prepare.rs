use crate::{
    config::Config,
    error::Error,
    manifest,
    project::{self, SelectedPackage},
    runner::Runner,
};
use similar::TextDiff;
use std::{
    fs,
    path::{Path, PathBuf},
};
use toml_edit::{value, Array, DocumentMut, Item, Table};

pub struct Options<'a> {
    pub tag: &'a str,
    pub image_tag: Option<&'a str>,
    pub dry_run: bool,
    pub commit: bool,
    pub create_tag: bool,
    pub push: bool,
}

pub struct PlannedFile {
    pub path: PathBuf,
    pub old: String,
    pub new: String,
}

pub struct Plan {
    pub files: Vec<PlannedFile>,
    pub generate_lock: bool,
}

pub fn plan(
    root: &Path,
    project: &SelectedPackage,
    config: &Config,
    repository: &str,
    image_tag: Option<&str>,
) -> Result<Plan, Error> {
    if let Some(image_tag) = image_tag {
        let valid = image_tag
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            && image_tag
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b));
        if !valid {
            return Err(Error(
                "--cross-image-tag must be a container image tag such as main or 0.2.5.".into(),
            ));
        }
    }
    let ignore = root.join(".gitignore");
    let cross = root.join("Cross.toml");
    let lock = root.join("Cargo.lock");
    for path in [&project.manifest, &ignore, &cross, &lock] {
        if path.is_symlink() {
            return Err(Error(
                "Repository preparation refuses symlinked managed files.".into(),
            ));
        }
    }
    let changes = manifest::changes(&project.manifest, repository, &config.tag_prefix)?;
    let manifest_old = fs::read_to_string(&project.manifest)
        .map_err(|e| Error(format!("cannot read manifest: {e}")))?;
    let manifest_new = manifest::repair(&project.manifest, &changes, false)?;
    let ignore_old = read_optional(&ignore)?;
    let mut ignore_new = ignore_old.clone();
    let missing: Vec<_> = ["/dist/", "/target/", "!/Cargo.lock"]
        .into_iter()
        .filter(|entry| !ignore_old.lines().any(|line| line == *entry))
        .collect();
    if !missing.is_empty() {
        ignore_new = format!(
            "{}\n\n# Release publisher\n{}\n",
            ignore_old.trim_end_matches('\n'),
            missing.join("\n")
        );
    }
    let cross_old = read_optional(&cross)?;
    let mut cross_doc: DocumentMut = cross_old
        .parse()
        .map_err(|e| Error(format!("invalid Cross.toml: {e}")))?;
    ensure_table(cross_doc.as_table_mut(), "build")?;
    let build = cross_doc["build"]
        .as_table_mut()
        .ok_or_else(|| Error("Cross.toml 'build' must be a table.".into()))?;
    ensure_table(build, "env")?;
    let env = build["env"]
        .as_table_mut()
        .ok_or_else(|| Error("Cross.toml 'env' must be a table.".into()))?;
    let mut passthrough = Array::new();
    if let Some(existing) = env.get("passthrough") {
        let array = existing.as_array().ok_or_else(|| {
            Error("Cross.toml build.env.passthrough must be a list of strings.".into())
        })?;
        for item in array.iter() {
            let item = item.as_str().ok_or_else(|| {
                Error("Cross.toml build.env.passthrough must be a list of strings.".into())
            })?;
            passthrough.push(item);
        }
    }
    if !passthrough
        .iter()
        .any(|item| item.as_str() == Some("RUSTUP_TOOLCHAIN"))
    {
        passthrough.push("RUSTUP_TOOLCHAIN");
    }
    env["passthrough"] = value(passthrough);
    if let Some(image_tag) = image_tag {
        ensure_table(cross_doc.as_table_mut(), "target")?;
        let targets = cross_doc["target"]
            .as_table_mut()
            .ok_or_else(|| Error("Cross.toml 'target' must be a table.".into()))?;
        for triple in &config.targets {
            ensure_table(targets, triple)?;
            let target = targets[triple]
                .as_table_mut()
                .ok_or_else(|| Error(format!("Cross.toml target {triple:?} must be a table.")))?;
            target["image"] = value(format!("ghcr.io/cross-rs/{triple}:{image_tag}"));
        }
    }
    let cross_new = cross_doc.to_string();
    let files = [
        (project.manifest.clone(), manifest_old, manifest_new),
        (ignore, ignore_old, ignore_new),
        (cross, cross_old, cross_new),
    ]
    .into_iter()
    .filter(|(_, old, new)| old != new)
    .map(|(path, old, new)| PlannedFile { path, old, new })
    .collect();
    Ok(Plan {
        files,
        generate_lock: !lock.exists(),
    })
}

fn read_optional(path: &Path) -> Result<String, Error> {
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(path).map_err(|e| Error(format!("cannot read {}: {e}", path.display())))
}

fn ensure_table(parent: &mut Table, key: &str) -> Result<(), Error> {
    if !parent.contains_key(key) {
        parent[key] = Item::Table(Table::new());
    }
    if !parent[key].is_table() {
        return Err(Error(format!("Cross.toml {key:?} must be a table.")));
    }
    Ok(())
}

pub fn run(
    root: &Path,
    project: &SelectedPackage,
    config: &Config,
    repository: &str,
    options: Options<'_>,
    runner: &Runner,
) -> Result<(), Error> {
    if options.push && !(options.commit && options.create_tag) {
        return Err(Error(
            "--push-preparation requires --commit-preparation and --create-tag.".into(),
        ));
    }
    if options.create_tag && !options.commit {
        return Err(Error("--create-tag requires --commit-preparation.".into()));
    }
    let plan = plan(root, project, config, repository, options.image_tag)?;
    let lock = root.join("Cargo.lock");
    let mut branch = None;
    if options.commit {
        let status = runner.text(
            "git",
            &["status", "--porcelain", "--untracked-files=normal"],
            root,
        )?;
        if !status.is_empty() {
            return Err(Error("Start --commit-preparation from a clean checkout, including the index. Review and commit earlier local edits separately.".into()));
        }
        if options.create_tag {
            let exists = !runner
                .text("git", &["tag", "--list", options.tag], root)?
                .is_empty();
            let managed = [
                &project.manifest,
                &root.join(".gitignore"),
                &root.join("Cross.toml"),
                &lock,
            ];
            let mut untracked_managed = false;
            for path in managed {
                if path.exists()
                    && runner
                        .text(
                            "git",
                            &[
                                "ls-files",
                                "--",
                                path.to_str()
                                    .ok_or_else(|| Error("Managed path is not UTF-8.".into()))?,
                            ],
                            root,
                        )?
                        .is_empty()
                {
                    untracked_managed = true;
                }
            }
            if exists && (!plan.files.is_empty() || plan.generate_lock || untracked_managed) {
                return Err(Error(format!("Tag {:?} already exists; preparation will not move it. Use a new package version or prepare without --create-tag.", options.tag)));
            }
            if exists {
                project::validate_checkout_tag(root, options.tag, runner)?;
            }
        }
        if options.push {
            branch = Some(
                runner
                    .text("git", &["symbolic-ref", "--quiet", "--short", "HEAD"], root)
                    .map_err(|_| {
                        Error("Pushing preparation requires a checked-out branch.".into())
                    })?,
            );
            let destination =
                runner.text("git", &["remote", "get-url", "--push", "origin"], root)?;
            if project::normalize_repository(&destination)? != repository {
                return Err(Error("origin push URL differs from the release repository. Fix origin explicitly before pushing.".into()));
            }
        }
    }
    for file in &plan.files {
        let label = file
            .path
            .strip_prefix(root)
            .unwrap_or(&file.path)
            .display()
            .to_string();
        let diff = TextDiff::from_lines(&file.old, &file.new)
            .unified_diff()
            .header(&label, &label)
            .to_string();
        print!("{}", runner.redact(&diff));
    }
    if plan.generate_lock {
        println!(
            "Cargo.lock: generate with cargo generate-lockfile (may download registry metadata)."
        );
    }
    if options.commit {
        println!(
            "Git: commit only the prepared files using the configured author and signing settings."
        );
    }
    if options.create_tag {
        println!(
            "Git: create or validate annotated tag {}. Existing tags are never moved.",
            options.tag
        );
    }
    if let Some(branch) = &branch {
        println!(
            "Git: atomically push branch {branch} and tag {} to origin; no force push.",
            options.tag
        );
    }
    if options.dry_run {
        println!("Preparation dry run complete; no files, commits, tags, or remote refs changed.");
        return Ok(());
    }
    for file in &plan.files {
        fs::write(&file.path, &file.new)
            .map_err(|e| Error(format!("cannot write {}: {e}", file.path.display())))?;
    }
    if plan.generate_lock {
        runner.output("cargo", &["generate-lockfile"], root)?;
    }
    if options.commit {
        let mut paths = [
            project.manifest.clone(),
            root.join(".gitignore"),
            root.join("Cross.toml"),
            lock,
        ];
        paths.sort();
        let names: Vec<&str> = paths
            .iter()
            .map(|p| {
                p.to_str()
                    .ok_or_else(|| Error("Managed path is not UTF-8.".into()))
            })
            .collect::<Result<_, _>>()?;
        let args: Vec<&str> = std::iter::once("add")
            .chain(["--force", "--"])
            .chain(names)
            .collect();
        runner.output("git", &args, root)?;
        if !runner
            .text("git", &["diff", "--cached", "--name-only"], root)?
            .is_empty()
        {
            let message = format!(
                "Prepare {} {} binary release",
                project.name, project.version
            );
            runner.output(
                "git",
                &["-c", "core.hooksPath=/dev/null", "commit", "-m", &message],
                root,
            )?;
        }
    }
    if options.create_tag {
        if runner
            .text("git", &["tag", "--list", options.tag], root)?
            .is_empty()
        {
            runner.output(
                "git",
                &[
                    "tag",
                    "-a",
                    options.tag,
                    "-m",
                    &format!("{} {}", project.name, project.version),
                ],
                root,
            )?;
        }
        project::validate_checkout_tag(root, options.tag, runner)?;
    }
    if let Some(branch) = branch {
        push_preparation(root, &branch, options.tag, repository, config, runner)?;
    }
    println!("Repository preparation complete. Review the changes, then run the publisher without --prepare-repo.");
    Ok(())
}

fn push_preparation(
    root: &Path,
    branch: &str,
    tag: &str,
    repository: &str,
    config: &Config,
    runner: &Runner,
) -> Result<(), Error> {
    let origin = runner.text("git", &["remote", "get-url", "--push", "origin"], root)?;
    if project::normalize_repository(&origin)? != repository {
        return Err(Error("origin push URL differs from the release repository. Fix origin explicitly before pushing.".into()));
    }
    let branch_ref = format!("HEAD:refs/heads/{branch}");
    let tag_ref = format!("refs/tags/{tag}");
    if origin.starts_with("https://") {
        if let Ok(token) = std::env::var(&config.token_env) {
            if !token.is_empty() {
                let helper = tempfile::Builder::new()
                    .prefix("rust-release-askpass-")
                    .tempdir()
                    .map_err(|e| Error(format!("cannot create askpass helper: {e}")))?;
                let script = helper.path().join("askpass");
                fs::write(&script, "#!/bin/sh\ncase \"$1\" in *sername*) printf 'token\\n';; *) printf '%s\\n' \"$PUBLISHER_GIT_TOKEN\";; esac\n").map_err(|e| Error(format!("cannot write askpass helper: {e}")))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&script, fs::Permissions::from_mode(0o700))
                        .map_err(|e| Error(format!("cannot chmod askpass helper: {e}")))?;
                }
                let helper_path = script
                    .to_str()
                    .ok_or_else(|| Error("Askpass helper path is not UTF-8.".into()))?;
                return runner
                    .output_with(
                        "git",
                        &[
                            "-c",
                            "credential.helper=",
                            "-c",
                            "core.hooksPath=/dev/null",
                            "push",
                            "--atomic",
                            "origin",
                            &branch_ref,
                            &tag_ref,
                        ],
                        root,
                        &[
                            ("GIT_ASKPASS", helper_path),
                            ("GIT_TERMINAL_PROMPT", "0"),
                            ("PUBLISHER_GIT_TOKEN", &token),
                            ("GIT_TRACE", "0"),
                            ("GIT_TRACE_CURL", "0"),
                            ("GIT_TRACE2", "0"),
                        ],
                        &[],
                    )
                    .map(|_| ());
            }
        }
    }
    runner.output_with(
        "git",
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "push",
            "--atomic",
            "origin",
            &branch_ref,
            &tag_ref,
        ],
        root,
        &[("GIT_TERMINAL_PROMPT", "0")],
        &[],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, SelectedPackage, Config, Runner) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("Cargo.toml"), "# Keep this comment\n[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\nrepository='https://example.org/team/demo'\n").unwrap();
        fs::write(root.join(".gitignore"), "Cargo.lock\n").unwrap();
        let runner = Runner::new("GITEA_TOKEN");
        runner
            .output("git", &["init", "-q", "-b", "main"], root)
            .unwrap();
        for (key, val) in [
            ("user.name", "Test"),
            ("user.email", "test@example.invalid"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
        ] {
            runner.output("git", &["config", key, val], root).unwrap();
        }
        runner
            .output(
                "git",
                &["add", "Cargo.toml", ".gitignore", "src/main.rs"],
                root,
            )
            .unwrap();
        runner
            .output("git", &["commit", "-qm", "initial"], root)
            .unwrap();
        let project = project::inspect(root.to_str().unwrap(), None, &runner).unwrap();
        let config = Config {
            targets: vec!["x86_64-unknown-linux-musl".into()],
            profile: "release".into(),
            locked: true,
            tag_prefix: "v".into(),
            token_env: "GITEA_TOKEN".into(),
        };
        (temp, project, config, runner)
    }

    fn options<'a>(dry_run: bool, commit: bool, create_tag: bool) -> Options<'a> {
        Options {
            tag: "v0.1.0",
            image_tag: Some("main"),
            dry_run,
            commit,
            create_tag,
            push: false,
        }
    }

    #[test]
    fn preview_then_signed_policy_commit_and_annotated_tag() {
        let (temp, project, config, runner) = fixture();
        let root = temp.path();
        let before = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        let head = runner.text("git", &["rev-parse", "HEAD"], root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let hook = root.join(".git/hooks/pre-commit");
            fs::write(&hook, "#!/bin/sh\ntouch hook-ran\n").unwrap();
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        }
        run(
            root,
            &project,
            &config,
            "https://example.org/team/demo",
            options(true, true, true),
            &runner,
        )
        .unwrap();
        assert_eq!(fs::read_to_string(root.join("Cargo.toml")).unwrap(), before);
        assert!(!root.join("Cargo.lock").exists());
        assert!(!root.join("Cross.toml").exists());
        assert_eq!(
            runner.text("git", &["rev-parse", "HEAD"], root).unwrap(),
            head
        );
        run(
            root,
            &project,
            &config,
            "https://example.org/team/demo",
            options(false, true, true),
            &runner,
        )
        .unwrap();
        let prepared = runner.text("git", &["rev-parse", "HEAD"], root).unwrap();
        assert_ne!(prepared, head);
        assert_eq!(
            runner
                .text("git", &["rev-parse", "v0.1.0^{commit}"], root)
                .unwrap(),
            prepared
        );
        assert_eq!(
            runner
                .text("git", &["cat-file", "-t", "v0.1.0"], root)
                .unwrap(),
            "tag"
        );
        assert!(runner
            .text("git", &["status", "--porcelain"], root)
            .unwrap()
            .is_empty());
        assert!(fs::read_to_string(root.join("Cargo.toml"))
            .unwrap()
            .contains("# Keep this comment"));
        assert!(root.join("Cargo.lock").exists());
        assert!(!root.join("hook-ran").exists());
        run(
            root,
            &project,
            &config,
            "https://example.org/team/demo",
            options(false, true, true),
            &runner,
        )
        .unwrap();
        assert_eq!(
            runner.text("git", &["rev-parse", "HEAD"], root).unwrap(),
            prepared
        );
    }

    #[test]
    fn rejects_dirty_inputs_and_existing_tag_before_writes() {
        let (temp, project, config, runner) = fixture();
        let root = temp.path();
        fs::write(root.join("unrelated.txt"), "work").unwrap();
        assert!(run(
            root,
            &project,
            &config,
            "https://example.org/team/demo",
            options(false, true, false),
            &runner
        )
        .unwrap_err()
        .to_string()
        .contains("clean checkout"));
        assert!(!root.join("Cross.toml").exists());
        fs::remove_file(root.join("unrelated.txt")).unwrap();
        runner.output("git", &["tag", "v0.1.0"], root).unwrap();
        assert!(run(
            root,
            &project,
            &config,
            "https://example.org/team/demo",
            options(false, true, true),
            &runner
        )
        .unwrap_err()
        .to_string()
        .contains("will not move"));
        assert!(!root.join("Cross.toml").exists());
    }

    #[test]
    fn preserves_custom_cross_settings_and_rejects_bad_image_tag() {
        let (temp, project, config, _runner) = fixture();
        let root = temp.path();
        fs::write(root.join("Cross.toml"), "# custom\n[build.env]\npassthrough=['MY_VAR']\n[target.x86_64-unknown-linux-musl]\nimage='my/custom:image'\n").unwrap();
        let planned = plan(
            root,
            &project,
            &config,
            "https://example.org/team/demo",
            None,
        )
        .unwrap();
        let cross = planned
            .files
            .iter()
            .find(|f| f.path.ends_with("Cross.toml"))
            .unwrap();
        assert!(cross.new.contains("# custom"));
        assert!(cross.new.contains("my/custom:image"));
        assert!(cross.new.contains("RUSTUP_TOOLCHAIN"));
        assert!(plan(
            root,
            &project,
            &config,
            "https://example.org/team/demo",
            Some("bad/tag")
        )
        .is_err());
    }

    #[test]
    fn push_destination_mismatch_is_caught_before_writes() {
        let (temp, project, config, runner) = fixture();
        let root = temp.path();
        runner
            .output(
                "git",
                &[
                    "remote",
                    "add",
                    "origin",
                    "https://other.invalid/team/demo.git",
                ],
                root,
            )
            .unwrap();
        let before = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        let options = Options {
            tag: "v0.1.0",
            image_tag: None,
            dry_run: true,
            commit: true,
            create_tag: true,
            push: true,
        };
        assert!(run(
            root,
            &project,
            &config,
            "https://example.org/team/demo",
            options,
            &runner
        )
        .unwrap_err()
        .to_string()
        .contains("differs"));
        assert_eq!(fs::read_to_string(root.join("Cargo.toml")).unwrap(), before);
        assert!(!root.join("Cross.toml").exists());
    }
}
