use crate::{error::Error, project};
use clap::ValueEnum;
use directories::ProjectDirs;
use serde::Deserialize;
use std::{fs, path::Path};

pub const PROJECT_CONFIG_FILE: &str = "rust-release.toml";

#[derive(Debug, Clone, Deserialize, Default)]
struct FileConfig {
    build: Option<Build>,
    release: Option<Release>,
    package: Option<Package>,
    repository: Option<Repository>,
    gitea: Option<Gitea>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct Build {
    targets: Option<Vec<String>>,
    profile: Option<String>,
    locked: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct Release {
    #[serde(rename = "tag-prefix")]
    tag_prefix: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct Package {
    format: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct Repository {
    #[serde(default)]
    fallbacks: Vec<FileRepositoryFallback>,
}

#[derive(Debug, Clone, Deserialize)]
struct FileRepositoryFallback {
    origin: String,
    #[serde(rename = "fallback-url")]
    fallback_url: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct Gitea {
    #[serde(rename = "token-env")]
    token_env: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryFallback {
    pub origin: String,
    pub fallback_url: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub targets: Vec<String>,
    pub profile: String,
    pub locked: bool,
    pub tag_prefix: String,
    pub token_env: String,
    pub package_format: PackageFormat,
    pub repository_fallbacks: Vec<RepositoryFallback>,
}

impl Config {
    pub fn resolve_repository(&self, input: &str) -> Result<String, Error> {
        let normalized = project::normalize_repository(input)?;
        Ok(self
            .repository_fallbacks
            .iter()
            .rev()
            .find(|fallback| fallback.origin == normalized)
            .map_or(normalized, |fallback| fallback.fallback_url.clone()))
    }

    pub fn repositories_match(&self, left: &str, right: &str) -> Result<bool, Error> {
        Ok(self.resolve_repository(left)? == self.resolve_repository(right)?)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PackageFormat {
    Tgz,
    Binary,
    Both,
}

impl PackageFormat {
    pub fn archives(self) -> bool {
        matches!(self, Self::Tgz | Self::Both)
    }

    pub fn binaries(self) -> bool {
        matches!(self, Self::Binary | Self::Both)
    }
}

#[derive(Default)]
pub struct Overrides {
    pub targets: Vec<String>,
    pub profile: Option<String>,
    pub unlocked: bool,
    pub tag_prefix: Option<String>,
    pub package_format: Option<PackageFormat>,
}

pub fn load(
    explicit_path: Option<&Path>,
    project_root: &Path,
    overrides: Overrides,
) -> Result<Config, Error> {
    let mut sources = vec![(
        "bundled defaults".to_owned(),
        include_str!("../config.toml").to_owned(),
    )];
    let mut discovered = Vec::new();
    if let Some(project_dirs) = ProjectDirs::from("", "", "rust_realease") {
        discovered.push(project_dirs.config_dir().join("config.toml"));
    }
    discovered.push(project_root.join(PROJECT_CONFIG_FILE));
    for path in discovered {
        if path.is_file() && explicit_path != Some(path.as_path()) {
            sources.push((path.display().to_string(), read_config(&path)?));
        }
    }
    if let Some(path) = explicit_path {
        sources.push((path.display().to_string(), read_config(path)?));
    }
    parse_sources(sources, overrides)
}

fn read_config(path: &Path) -> Result<String, Error> {
    fs::read_to_string(path)
        .map_err(|e| Error(format!("cannot read config {}: {e}", path.display())))
}

fn parse_sources(sources: Vec<(String, String)>, overrides: Overrides) -> Result<Config, Error> {
    let mut merged = FileConfig::default();
    for (label, source) in sources {
        let file: FileConfig =
            toml::from_str(&source).map_err(|e| Error(format!("invalid config {label}: {e}")))?;
        merged.merge(file);
    }
    finish(merged, overrides)
}

impl FileConfig {
    fn merge(&mut self, later: Self) {
        merge_build(&mut self.build, later.build);
        merge_release(&mut self.release, later.release);
        merge_package(&mut self.package, later.package);
        merge_repository(&mut self.repository, later.repository);
        merge_gitea(&mut self.gitea, later.gitea);
    }
}

fn merge_build(current: &mut Option<Build>, later: Option<Build>) {
    if let Some(later) = later {
        let current = current.get_or_insert_with(Build::default);
        if later.targets.is_some() {
            current.targets = later.targets;
        }
        if later.profile.is_some() {
            current.profile = later.profile;
        }
        if later.locked.is_some() {
            current.locked = later.locked;
        }
    }
}

fn merge_release(current: &mut Option<Release>, later: Option<Release>) {
    if let Some(later) = later {
        let current = current.get_or_insert_with(Release::default);
        if later.tag_prefix.is_some() {
            current.tag_prefix = later.tag_prefix;
        }
    }
}

fn merge_package(current: &mut Option<Package>, later: Option<Package>) {
    if let Some(later) = later {
        let current = current.get_or_insert_with(Package::default);
        if later.format.is_some() {
            current.format = later.format;
        }
    }
}

fn merge_repository(current: &mut Option<Repository>, later: Option<Repository>) {
    if let Some(later) = later {
        current
            .get_or_insert_with(Repository::default)
            .fallbacks
            .extend(later.fallbacks);
    }
}

fn merge_gitea(current: &mut Option<Gitea>, later: Option<Gitea>) {
    if let Some(later) = later {
        let current = current.get_or_insert_with(Gitea::default);
        if later.token_env.is_some() {
            current.token_env = later.token_env;
        }
    }
}

fn finish(file: FileConfig, overrides: Overrides) -> Result<Config, Error> {
    let build = file.build.unwrap_or_default();
    let release = file.release.unwrap_or_default();
    let package = file.package.unwrap_or_default();
    let repository = file.repository.unwrap_or_default();
    let gitea = file.gitea.unwrap_or_default();
    let package_format = match package.format.as_deref().unwrap_or("tgz") {
        "tgz" => PackageFormat::Tgz,
        "binary" => PackageFormat::Binary,
        "both" => PackageFormat::Both,
        _ => {
            return Err(Error(
                "package.format must be \"tgz\", \"binary\", or \"both\".".into(),
            ))
        }
    };
    let mut repository_fallbacks = Vec::new();
    for fallback in repository.fallbacks {
        let origin = project::normalize_repository(&fallback.origin)
            .map_err(|e| Error(format!("invalid repository fallback origin: {e}")))?;
        let fallback_url = project::normalize_repository(&fallback.fallback_url)
            .map_err(|e| Error(format!("invalid repository fallback-url: {e}")))?;
        if !fallback_url.starts_with("https://") {
            return Err(Error("repository fallback-url must use HTTPS.".into()));
        }
        repository_fallbacks.retain(|existing: &RepositoryFallback| existing.origin != origin);
        repository_fallbacks.push(RepositoryFallback {
            origin,
            fallback_url,
        });
    }
    let mut config = Config {
        targets: if overrides.targets.is_empty() {
            build.targets.unwrap_or_else(|| {
                [
                    "x86_64-unknown-linux-musl",
                    "aarch64-unknown-linux-musl",
                    "x86_64-pc-windows-gnu",
                ]
                .map(str::to_owned)
                .to_vec()
            })
        } else {
            overrides.targets
        },
        profile: overrides
            .profile
            .or(build.profile)
            .unwrap_or_else(|| "release".into()),
        locked: if overrides.unlocked {
            false
        } else {
            build.locked.unwrap_or(true)
        },
        tag_prefix: overrides
            .tag_prefix
            .or(release.tag_prefix)
            .unwrap_or_else(|| "v".into()),
        token_env: gitea.token_env.unwrap_or_else(|| "GITEA_TOKEN".into()),
        package_format: overrides.package_format.unwrap_or(package_format),
        repository_fallbacks,
    };
    if config.targets.is_empty() || config.targets.iter().any(|target| !valid_target(target)) {
        return Err(Error(
            "build.targets must be a nonempty list of Rust target triples.".into(),
        ));
    }
    let mut unique = Vec::new();
    for target in config.targets {
        if !unique.contains(&target) {
            unique.push(target);
        }
    }
    config.targets = unique;
    if !valid_profile(&config.profile) {
        return Err(Error("build.profile must be a Cargo profile name.".into()));
    }
    if !config
        .tag_prefix
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(Error(
            "release.tag-prefix must contain only letters, digits, dots, underscores or hyphens."
                .into(),
        ));
    }
    if !valid_env(&config.token_env) {
        return Err(Error(
            "gitea.token-env must be an environment variable name.".into(),
        ));
    }
    Ok(config)
}

fn valid_target(s: &str) -> bool {
    s.split('-').count() >= 3
        && s.split('-').all(|part| {
            !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
}

fn valid_profile(s: &str) -> bool {
    s.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

fn valid_env(s: &str) -> bool {
    s.as_bytes()
        .first()
        .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str, overrides: Overrides) -> Result<Config, Error> {
        parse_sources(vec![("test".into(), source.into())], overrides)
    }

    #[test]
    fn overrides_and_validation() {
        let config = parse("[build]\ntargets=['x86_64-unknown-linux-musl']\nlocked=true\n[release]\ntag-prefix='v'", Overrides { targets: vec!["aarch64-unknown-linux-musl".into()], unlocked: true, tag_prefix: Some(String::new()), ..Overrides::default() }).unwrap();
        assert_eq!(config.targets, ["aarch64-unknown-linux-musl"]);
        assert!(!config.locked);
        assert_eq!(config.tag_prefix, "");
        assert!(parse("[build]\ntargets=['bad']", Overrides::default()).is_err());
        assert!(parse("[package]\nformat='zip'", Overrides::default()).is_err());
        assert_eq!(
            parse("[package]\nformat='both'", Overrides::default())
                .unwrap()
                .package_format,
            PackageFormat::Both
        );
    }

    #[test]
    fn layered_config_and_repository_fallbacks() {
        let config = parse_sources(
            vec![
                ("default".into(), "[build]\ntargets=['x86_64-unknown-linux-musl']\nprofile='release'\n[repository]\nfallbacks=[]".into()),
                ("user".into(), "[build]\nprofile='small'\n[[repository.fallbacks]]\norigin='http://git.dandaedre.com/idle-org/john-slop.git'\nfallback-url='https://old.example/gitea/idle-org/john-slop'".into()),
                ("project".into(), "[[repository.fallbacks]]\norigin='http://git.dandaedre.com/idle-org/john-slop'\nfallback-url='https://proxy.dandaedre.com/gitea/idle-org/john-slop.git'".into()),
            ],
            Overrides::default(),
        )
        .unwrap();
        assert_eq!(config.profile, "small");
        assert_eq!(config.repository_fallbacks.len(), 1);
        assert_eq!(
            config
                .resolve_repository("http://git.dandaedre.com/idle-org/john-slop.git")
                .unwrap(),
            "https://proxy.dandaedre.com/gitea/idle-org/john-slop"
        );
        assert!(config
            .repositories_match(
                "http://git.dandaedre.com/idle-org/john-slop.git",
                "https://proxy.dandaedre.com/gitea/idle-org/john-slop"
            )
            .unwrap());
    }

    #[test]
    fn fallback_requires_https() {
        let result = parse("[[repository.fallbacks]]\norigin='https://git.example/owner/repo'\nfallback-url='http://proxy.example/owner/repo'", Overrides::default());
        assert!(result.unwrap_err().to_string().contains("must use HTTPS"));
    }

    #[test]
    fn project_config_is_loaded_after_bundled_config() {
        let project = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join(PROJECT_CONFIG_FILE),
            "[build]\nprofile='project-profile'\n",
        )
        .unwrap();
        let config = load(None, project.path(), Overrides::default()).unwrap();
        assert_eq!(config.profile, "project-profile");
        assert_eq!(config.targets.len(), 3);
    }
}
