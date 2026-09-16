use crate::error::Error;
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug)]
pub struct SelectedPackage {
    pub name: String,
    pub version: String,
    pub binary: String,
    pub manifest: PathBuf,
    pub repository: Option<String>,
}
#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    workspace_root: PathBuf,
}
#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    manifest_path: PathBuf,
    repository: Option<String>,
    targets: Vec<Target>,
}
#[derive(Deserialize)]
struct Target {
    name: String,
    kind: Vec<String>,
}

pub fn inspect(source: &str, package_name: Option<&str>) -> Result<SelectedPackage, Error> {
    let root = Path::new(source);
    if !root.is_dir() {
        return Err(Error(
            "Rust inspect currently requires a local project directory.".into(),
        ));
    }
    let manifest = root.join("Cargo.toml");
    let source_text = fs::read_to_string(&manifest)
        .map_err(|e| Error(format!("cannot read {}: {e}", manifest.display())))?;
    let source_doc: toml::Value = toml::from_str(&source_text)
        .map_err(|e| Error(format!("invalid {}: {e}", manifest.display())))?;
    let output = Command::new("cargo")
        .args(["metadata", "--format-version=1", "--no-deps"])
        .current_dir(root)
        .output()
        .map_err(|e| Error(format!("cannot run cargo metadata: {e}")))?;
    if !output.status.success() {
        return Err(Error(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let metadata: Metadata = serde_json::from_slice(&output.stdout)
        .map_err(|e| Error(format!("invalid cargo metadata output: {e}")))?;
    let members: Vec<_> = metadata
        .packages
        .into_iter()
        .filter(|p| metadata.workspace_members.contains(&p.id))
        .collect();
    let discovered = members
        .iter()
        .map(|p| p.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let selected = match package_name {
        Some(name) => members
            .into_iter()
            .find(|p| p.name == name)
            .ok_or_else(|| {
                Error(format!(
                    "Workspace package {name:?} was not found uniquely; discovered: {discovered}"
                ))
            })?,
        None if source_doc
            .get("package")
            .and_then(toml::Value::as_table)
            .is_some()
            && members.len() == 1 =>
        {
            members
                .into_iter()
                .next()
                .ok_or_else(|| Error("No workspace package found.".into()))?
        }
        None => {
            return Err(Error(format!(
                "Workspace selection requires --package NAME; discovered: {discovered}"
            )))
        }
    };
    let binaries: Vec<_> = selected
        .targets
        .iter()
        .filter(|t| t.kind.iter().any(|kind| kind == "bin"))
        .collect();
    if binaries.len() != 1 {
        return Err(Error(format!(
            "Package {:?} must have exactly one binary; discovered: {}",
            selected.name,
            binaries
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    for value in [&selected.name, &selected.version, &binaries[0].name] {
        if !safe_component(value) {
            return Err(Error("Unsafe package, version or binary filename.".into()));
        }
    }
    let workspace_root = metadata
        .workspace_root
        .canonicalize()
        .map_err(|e| Error(format!("invalid workspace root: {e}")))?;
    let member_manifest = selected
        .manifest_path
        .canonicalize()
        .map_err(|e| Error(format!("invalid member manifest: {e}")))?;
    if !member_manifest.starts_with(&workspace_root) {
        return Err(Error(
            "Selected package manifest is outside the workspace root.".into(),
        ));
    }
    let member_text = fs::read_to_string(&member_manifest)
        .map_err(|e| Error(format!("cannot read {}: {e}", member_manifest.display())))?;
    let member_doc: toml::Value = toml::from_str(&member_text)
        .map_err(|e| Error(format!("invalid {}: {e}", member_manifest.display())))?;
    if member_doc
        .get("package")
        .and_then(toml::Value::as_table)
        .is_none()
    {
        return Err(Error(
            "Selected Cargo.toml contains no [package] table.".into(),
        ));
    }
    Ok(SelectedPackage {
        name: selected.name,
        version: selected.version,
        binary: binaries[0].name.clone(),
        manifest: member_manifest,
        repository: selected.repository,
    })
}
fn safe_component(value: &str) -> bool {
    value
        .as_bytes()
        .first()
        .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.+-".contains(&b))
}

pub fn origin_url(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(root)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn validate_checkout_tag(root: &Path, tag: &str) -> Result<(), Error> {
    let tagged = Command::new("git")
        .args([
            "rev-parse",
            "--verify",
            &format!("refs/tags/{tag}^{{commit}}"),
        ])
        .current_dir(root)
        .output()
        .map_err(|e| Error(format!("cannot validate Git tag: {e}")))?;
    if !tagged.status.success() {
        return Err(Error(format!(
            "Git tag {tag:?} does not exist. Create it explicitly first."
        )));
    }
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .map_err(|e| Error(format!("cannot inspect Git HEAD: {e}")))?;
    if !head.status.success() || head.stdout != tagged.stdout {
        return Err(Error(format!(
            "Git tag {tag:?} does not point to this checkout. Check it out explicitly."
        )));
    }
    Ok(())
}

pub fn normalize_repository(input: &str) -> Result<String, Error> {
    let candidate = if let Some((user_host, path)) = input.split_once(':') {
        if !input.contains("://") && user_host.contains('@') {
            let (user, host) = user_host.split_once('@').unwrap_or(("", ""));
            if user.is_empty() || host.is_empty() || path.is_empty() {
                return Err(Error(
                    "Repository must be an HTTP(S) or SSH Git URL; use --repository-url.".into(),
                ));
            }
            format!("https://{host}/{path}")
        } else {
            input.to_owned()
        }
    } else {
        input.to_owned()
    };
    let parsed = url::Url::parse(&candidate).map_err(|_| {
        Error("Repository must be an HTTP(S) or SSH Git URL; use --repository-url.".into())
    })?;
    if !matches!(parsed.scheme(), "http" | "https" | "ssh") || parsed.host_str().is_none() {
        return Err(Error(
            "Repository must be an HTTP(S) or SSH Git URL; use --repository-url.".into(),
        ));
    }
    if parsed.password().is_some()
        || (parsed.scheme() != "ssh" && !parsed.username().is_empty())
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(Error(
            "Repository URLs must not contain credentials, query strings or fragments.".into(),
        ));
    }
    let parts: Vec<_> = parsed
        .path_segments()
        .ok_or_else(|| Error("Repository URL must end in owner/repository.".into()))?
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() < 2
        || parts.iter().any(|part| {
            *part == "."
                || *part == ".."
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        })
    {
        return Err(Error("Repository URL must end in owner/repository.".into()));
    }
    let name = parts[parts.len() - 1]
        .strip_suffix(".git")
        .unwrap_or(parts[parts.len() - 1]);
    if name.is_empty() || name == "." || name == ".." {
        return Err(Error("Repository name is empty.".into()));
    }
    let host = parsed.host_str().unwrap_or_default();
    let authority = if let Some(port) = parsed.port() {
        format!("{host}:{port}")
    } else {
        host.to_owned()
    };
    let scheme = if parsed.scheme() == "ssh" {
        "https"
    } else {
        parsed.scheme()
    };
    Ok(format!(
        "{scheme}://{authority}/{}{}{}",
        parts[..parts.len() - 1].join("/"),
        "/",
        name
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, path: &str, content: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn standalone_project_and_workspace_selection() {
        let temp = tempfile::tempdir().unwrap();
        write(
            temp.path(),
            "Cargo.toml",
            "[package]\nname='solo'\nversion='0.1.0'\nedition='2021'\n",
        );
        write(temp.path(), "src/main.rs", "fn main() {}\n");
        let selected = inspect(temp.path().to_str().unwrap(), None).unwrap();
        assert_eq!(selected.name, "solo");
        assert_eq!(selected.binary, "solo");

        let workspace = tempfile::tempdir().unwrap();
        write(
            workspace.path(),
            "Cargo.toml",
            "[workspace]\nmembers=['one','two']\nresolver='2'\n",
        );
        for name in ["one", "two"] {
            write(
                workspace.path(),
                &format!("{name}/Cargo.toml"),
                &format!("[package]\nname='{name}'\nversion='0.2.0'\nedition='2021'\n"),
            );
            write(
                workspace.path(),
                &format!("{name}/src/main.rs"),
                "fn main() {}\n",
            );
        }
        let root = workspace.path().to_str().unwrap();
        assert!(inspect(root, None)
            .unwrap_err()
            .to_string()
            .contains("--package"));
        assert!(inspect(root, Some("missing"))
            .unwrap_err()
            .to_string()
            .contains("not found"));
        assert_eq!(inspect(root, Some("two")).unwrap().binary, "two");
    }

    #[test]
    fn rejects_library_and_multiple_binaries() {
        let temp = tempfile::tempdir().unwrap();
        write(
            temp.path(),
            "Cargo.toml",
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
        );
        write(temp.path(), "src/lib.rs", "pub fn hello() {}\n");
        let root = temp.path().to_str().unwrap();
        assert!(inspect(root, None)
            .unwrap_err()
            .to_string()
            .contains("exactly one binary"));
        write(temp.path(), "src/main.rs", "fn main() {}\n");
        write(temp.path(), "src/bin/extra.rs", "fn main() {}\n");
        assert!(inspect(root, None)
            .unwrap_err()
            .to_string()
            .contains("exactly one binary"));
    }

    #[test]
    fn repository_url_normalization_rejects_credentials() {
        assert_eq!(
            normalize_repository("git@example.com:owner/demo.git").unwrap(),
            "https://example.com/owner/demo"
        );
        assert_eq!(
            normalize_repository("ssh://git@example.com/owner/demo.git").unwrap(),
            "https://example.com/owner/demo"
        );
        assert_eq!(
            normalize_repository("https://example.com/gitea/owner/demo.git").unwrap(),
            "https://example.com/gitea/owner/demo"
        );
        for value in [
            "https://token@example.com/owner/demo",
            "https://example.com/owner/demo?token=secret",
            "https://example.com/owner/../demo",
        ] {
            assert!(normalize_repository(value).is_err(), "{value}");
        }
    }
}
