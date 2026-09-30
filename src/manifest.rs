use crate::{error::Error, policy, project};
use std::{fs, path::Path};
use toml_edit::{DocumentMut, Item, Table, Value};

#[derive(Debug, PartialEq, Eq)]
pub struct Change {
    pub key: String,
    pub actual: Option<String>,
    pub expected: Option<String>,
}

fn document(path: &Path) -> Result<DocumentMut, Error> {
    let source = fs::read_to_string(path)
        .map_err(|e| Error(format!("cannot read {}: {e}", path.display())))?;
    source
        .parse::<DocumentMut>()
        .map_err(|e| Error(format!("invalid {}: {e}", path.display())))
}

fn package(doc: &DocumentMut) -> Result<&Table, Error> {
    doc.get("package")
        .and_then(Item::as_table)
        .ok_or_else(|| Error("Cargo.toml contains no [package] table.".into()))
}

fn binstall(package: &Table) -> Result<Option<&Table>, Error> {
    let Some(metadata) = package.get("metadata") else {
        return Ok(None);
    };
    let metadata = metadata
        .as_table()
        .ok_or_else(|| Error("package.metadata must be a table.".into()))?;
    let Some(binstall) = metadata.get("binstall") else {
        return Ok(None);
    };
    binstall
        .as_table()
        .map(Some)
        .ok_or_else(|| Error("package.metadata.binstall must be a table.".into()))
}

pub fn changes(
    path: &Path,
    repository: &str,
    resolved_repository: Option<&str>,
    prefix: &str,
    require_binstall: bool,
) -> Result<Vec<Change>, Error> {
    let doc = document(path)?;
    let package = package(&doc)?;
    let mut result = Vec::new();
    let actual = package
        .get("repository")
        .and_then(Item::as_str)
        .map(str::to_owned);
    let inherited_and_resolved = package
        .get("repository")
        .is_some_and(inherits_from_workspace)
        && resolved_repository.is_some_and(|resolved| {
            project::normalize_repository(resolved).is_ok_and(|value| value == repository)
        });
    if actual.as_deref() != Some(repository) && !inherited_and_resolved {
        result.push(Change {
            key: "package.repository".into(),
            actual,
            expected: Some(repository.into()),
        });
    }
    if !require_binstall {
        return Ok(result);
    }
    let metadata = binstall(package)?;
    let expected = policy::binstall_metadata(prefix);
    for (key, desired) in &expected {
        let actual = metadata
            .and_then(|table| table.get(key))
            .and_then(Item::as_str)
            .map(str::to_owned);
        if actual.as_deref() != Some(desired) {
            result.push(Change {
                key: format!("package.metadata.binstall.{key}"),
                actual,
                expected: Some(desired.clone()),
            });
        }
    }
    if let Some(table) = metadata {
        for (key, item) in table.iter() {
            if !expected.iter().any(|(known, _)| key == *known) {
                result.push(Change {
                    key: format!("package.metadata.binstall.{key}"),
                    actual: Some(item.to_string().trim().to_owned()),
                    expected: None,
                });
            }
        }
    }
    Ok(result)
}

fn inherits_from_workspace(item: &Item) -> bool {
    if let Some(table) = item.as_table() {
        return table.get("workspace").and_then(Item::as_bool) == Some(true);
    }
    item.as_inline_table()
        .and_then(|table| table.get("workspace"))
        .and_then(Value::as_bool)
        == Some(true)
}

pub fn repair(path: &Path, changes: &[Change], write: bool) -> Result<String, Error> {
    if path.is_symlink() {
        return Err(Error("Refusing to repair a symlinked Cargo.toml.".into()));
    }
    let mut doc = document(path)?;
    let package = doc["package"]
        .as_table_mut()
        .ok_or_else(|| Error("Cargo.toml contains no [package] table.".into()))?;
    for change in changes {
        if change.key == "package.repository" {
            if let Some(desired) = &change.expected {
                set_string(&mut package["repository"], desired);
            }
            continue;
        }
        let key = change
            .key
            .strip_prefix("package.metadata.binstall.")
            .ok_or_else(|| Error("Invalid metadata change key.".into()))?;
        if !package.contains_key("metadata") {
            package["metadata"] = Item::Table(Table::new());
        }
        let metadata = package["metadata"]
            .as_table_mut()
            .ok_or_else(|| Error("package.metadata must be a table.".into()))?;
        if !metadata.contains_key("binstall") {
            metadata["binstall"] = Item::Table(Table::new());
        }
        let binstall = metadata["binstall"]
            .as_table_mut()
            .ok_or_else(|| Error("package.metadata.binstall must be a table.".into()))?;
        if let Some(desired) = &change.expected {
            set_string(&mut binstall[key], desired);
        } else {
            binstall.remove(key);
        }
    }
    let rendered = doc.to_string();
    if write {
        fs::write(path, &rendered)
            .map_err(|e| Error(format!("cannot write {}: {e}", path.display())))?;
    }
    Ok(rendered)
}

fn set_string(item: &mut Item, desired: &str) {
    let mut replacement = Value::from(desired);
    if let Some(previous) = item.as_value() {
        *replacement.decor_mut() = previous.decor().clone();
    }
    *item = Item::Value(replacement);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repair_preserves_comments_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Cargo.toml");
        fs::write(&path, "# keep this comment\n[package]\nname = 'demo'\nversion = '0.1.0'\nrepository = 'https://old.example/x/y' # keep inline\n[package.metadata.binstall]\npkg-fmt = 'zip'\ncustom = 'bad'\n").unwrap();
        let issues = changes(&path, "https://example.com/x/y", None, "release-", true).unwrap();
        assert_eq!(issues.len(), 5);
        let before = fs::read_to_string(&path).unwrap();
        let preview = repair(&path, &issues, false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        assert!(preview.contains("# keep this comment"));
        assert!(preview.contains("# keep inline"));
        repair(&path, &issues, true).unwrap();
        assert!(
            changes(&path, "https://example.com/x/y", None, "release-", true)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            repair(&path, &[], true).unwrap(),
            fs::read_to_string(&path).unwrap()
        );
    }
    #[test]
    fn rejects_malformed_metadata_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Cargo.toml");
        fs::write(&path, "[package]\nname='demo'\nmetadata='oops'\n").unwrap();
        assert!(changes(&path, "https://example.com/x/y", None, "v", true)
            .unwrap_err()
            .to_string()
            .contains("package.metadata must be a table"));
        let issues = changes(&path, "https://example.com/x/y", None, "v", false).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].key, "package.repository");
    }

    #[test]
    fn accepts_repository_inherited_from_matching_workspace_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Cargo.toml");
        fs::write(
            &path,
            "[package]\nname='demo'\nversion='0.1.0'\nrepository.workspace=true\n",
        )
        .unwrap();

        let issues = changes(
            &path,
            "https://example.com/owner/demo",
            Some("https://example.com/owner/demo.git"),
            "v",
            false,
        )
        .unwrap();

        assert!(issues.is_empty());
        assert!(fs::read_to_string(path)
            .unwrap()
            .contains("repository.workspace=true"));
    }

    #[test]
    fn repairs_inherited_repository_when_resolved_value_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Cargo.toml");
        fs::write(
            &path,
            "[package]\nname='demo'\nversion='0.1.0'\nrepository.workspace=true\n",
        )
        .unwrap();

        let issues = changes(
            &path,
            "https://example.com/owner/demo",
            Some("https://example.com/owner/other"),
            "v",
            false,
        )
        .unwrap();

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].key, "package.repository");
    }
}
