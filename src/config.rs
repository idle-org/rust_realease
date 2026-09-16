use crate::error::Error;
use serde::Deserialize;
use std::{fs, path::Path};

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    build: Option<Build>,
    release: Option<Release>,
    package: Option<Package>,
    gitea: Option<Gitea>,
}
#[derive(Debug, Deserialize, Default)]
struct Build {
    targets: Option<Vec<String>>,
    profile: Option<String>,
    locked: Option<bool>,
}
#[derive(Debug, Deserialize, Default)]
struct Release {
    #[serde(rename = "tag-prefix")]
    tag_prefix: Option<String>,
}
#[derive(Debug, Deserialize, Default)]
struct Package {
    format: Option<String>,
}
#[derive(Debug, Deserialize, Default)]
struct Gitea {
    #[serde(rename = "token-env")]
    token_env: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub targets: Vec<String>,
    pub profile: String,
    pub locked: bool,
    pub tag_prefix: String,
    pub token_env: String,
}
#[derive(Default)]
pub struct Overrides {
    pub targets: Vec<String>,
    pub profile: Option<String>,
    pub unlocked: bool,
    pub tag_prefix: Option<String>,
}
pub fn load(path: &Path, overrides: Overrides) -> Result<Config, Error> {
    let source = fs::read_to_string(path)
        .map_err(|e| Error(format!("cannot read config {}: {e}", path.display())))?;
    parse(&source, overrides)
}
fn parse(source: &str, overrides: Overrides) -> Result<Config, Error> {
    let file: FileConfig =
        toml::from_str(source).map_err(|e| Error(format!("invalid config: {e}")))?;
    let build = file.build.unwrap_or_default();
    let release = file.release.unwrap_or_default();
    let package = file.package.unwrap_or_default();
    let gitea = file.gitea.unwrap_or_default();
    if package.format.as_deref().unwrap_or("tgz") != "tgz" {
        return Err(Error("Only package.format = \"tgz\" is supported.".into()));
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
    #[test]
    fn overrides_and_validation() {
        let config = parse("[build]\ntargets=['x86_64-unknown-linux-musl']\nlocked=true\n[release]\ntag-prefix='v'", Overrides { targets: vec!["aarch64-unknown-linux-musl".into()], unlocked: true, tag_prefix: Some(String::new()), ..Overrides::default() }).unwrap();
        assert_eq!(config.targets, ["aarch64-unknown-linux-musl"]);
        assert!(!config.locked);
        assert_eq!(config.tag_prefix, "");
        assert!(parse("[build]\ntargets=['bad']", Overrides::default()).is_err());
        assert!(parse("[package]\nformat='zip'", Overrides::default()).is_err());
    }
}
