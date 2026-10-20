//! Host-side portable bundles; preparation is deliberately never cached.
use crate::{
    build::TargetOutputs,
    config::Config,
    error::Error,
    package::{self, Artifact, ArtifactContents},
    policy,
    project::SelectedPackage,
    runner::Runner,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Bundle {
    pub name: String,
    #[serde(default)]
    pub targets: Vec<String>,
    pub formats: BTreeMap<String, Format>,
    pub archive_name: String,
    pub root: String,
    pub cargo_binaries: Selection,
    #[serde(default)]
    pub files: Vec<Inclusion>,
    pub prepare: Option<Hook>,
    pub smoke_test: Option<Hook>,
}
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Tgz,
    Zip,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Selection {
    All(String),
    Names(Vec<String>),
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Inclusion {
    source: String,
    destination: String,
    #[serde(default)]
    workspace_relative: bool,
    #[serde(default)]
    kind: Kind,
}
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    File,
    Executable,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Hook {
    pub command: Vec<String>,
    #[serde(default)]
    pub inputs: Vec<String>,
    pub timeout_seconds: u64,
    #[serde(default)]
    pub required_destinations: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultManifest {
    schema_version: u32,
    target: String,
    files: Vec<Prepared>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepared {
    source: String,
    destination: String,
    kind: Kind,
    sha256: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub destination: String,
    pub sha256: String,
    pub mode: u32,
}
struct Input {
    entry: Entry,
    source: PathBuf,
}

fn fail<T>(message: impl Into<String>) -> Result<T, Error> {
    Err(Error(message.into()))
}
fn windows(target: &str) -> bool {
    target.split('-').any(|x| x == "windows")
}
fn render(s: &str, b: &Bundle, p: &SelectedPackage, target: &str) -> Result<String, Error> {
    let mut s = s.to_owned();
    for (key, value) in [
        ("name", p.name.as_str()),
        ("version", &p.version),
        ("bundle", &b.name),
        ("target", target),
        ("binary-ext", if windows(target) { ".exe" } else { "" }),
    ] {
        s = s.replace(&format!("{{{key}}}"), value);
    }
    if s.contains(['{', '}']) {
        return fail(format!("Unknown template placeholder: {s}"));
    }
    Ok(s)
}
fn safe(path: &str, win: bool) -> Result<(), Error> {
    if path.is_empty() || path.contains('\\') || path.starts_with('/') {
        return fail(format!("Unsafe bundle path: {path:?}"));
    }
    for part in path.split('/') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.chars().any(|c| c.is_control() || c == ':')
        {
            return fail(format!("Unsafe bundle path: {path:?}"));
        }
        if win {
            let base = part.split('.').next().unwrap().to_uppercase();
            if part.ends_with([' ', '.'])
                || part.contains(['<', '>', '"', '|', '?', '*'])
                || ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&base.as_str())
                || ["COM", "LPT"].iter().any(|prefix| {
                    base.strip_prefix(prefix).is_some_and(|n| {
                        ["1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"].contains(&n)
                    })
                })
            {
                return fail(format!("Invalid Windows destination: {path}"));
            }
        }
    }
    Ok(())
}
fn component(path: &str, win: bool) -> Result<(), Error> {
    safe(path, win)?;
    if path.contains('/') {
        return fail("Archive name and root must be single path components");
    }
    Ok(())
}
fn source(root: &Path, relative: &str) -> Result<PathBuf, Error> {
    safe(relative, false)?;
    if fs::symlink_metadata(root)?.file_type().is_symlink() {
        return fail("Symlinked input root rejected");
    }
    let mut path = root.to_path_buf();
    for part in relative.split('/') {
        path.push(part);
        let m = fs::symlink_metadata(&path)?;
        if m.file_type().is_symlink() {
            return fail(format!("Symlink input rejected: {}", path.display()));
        }
    }
    Ok(path)
}
fn regular(path: &Path) -> Result<(), Error> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() {
        return fail(format!("Not a regular bundle input: {}", path.display()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.nlink() != 1 {
            return fail(format!(
                "Hardlinked bundle input rejected: {}",
                path.display()
            ));
        }
    }
    Ok(())
}
fn selected<'a>(b: &Bundle, p: &'a SelectedPackage) -> Result<Vec<&'a String>, Error> {
    match &b.cargo_binaries {
        Selection::All(s) if s == "all" => Ok(p.binaries.iter().collect()),
        Selection::Names(names) if !names.is_empty() => {
            let mut result = Vec::new();
            for name in names {
                let n = p
                    .binaries
                    .iter()
                    .find(|n| *n == name)
                    .ok_or_else(|| Error(format!("Unknown Cargo binary {name}")))?;
                if result.contains(&n) {
                    return fail("Duplicate Cargo binary selection");
                }
                result.push(n);
            }
            Ok(result)
        }
        _ => fail("cargo-binaries must be 'all' or a nonempty array of declared binary names"),
    }
}

pub fn plan(config: &Config, p: &SelectedPackage, workspace: &Path) -> Result<(), Error> {
    let mut names = BTreeSet::new();
    let mut assets = BTreeSet::from(["SHA256SUMS".to_owned(), "artifact-manifest.json".to_owned()]);
    for target in &config.targets {
        if config.package_format.archives() {
            assets.insert(policy::archive_name(&p.name, target, &p.version));
        }
        if config.package_format.binaries() {
            for name in &p.binaries {
                assets.insert(policy::binary_asset_name(name, target, &p.version));
            }
        }
    }
    for b in &config.bundles {
        component(&b.name, true)?;
        if b.targets
            .iter()
            .chain(b.formats.keys())
            .any(|t| !crate::config::valid_target(t))
        {
            return fail(format!("Invalid target triple in bundle {}", b.name));
        }
        if b.smoke_test
            .as_ref()
            .is_some_and(|h| !h.required_destinations.is_empty())
        {
            return fail("required-destinations belongs to prepare, not smoke-test");
        }
        if !names.insert(&b.name) {
            return fail("Duplicate bundle name");
        }
        selected(b, p)?;
        if !b.targets.is_empty() && b.formats.keys().any(|t| !b.targets.contains(t)) {
            return fail(format!(
                "Bundle {} has a format outside its target filter",
                b.name
            ));
        }
        for h in [&b.prepare, &b.smoke_test].into_iter().flatten() {
            if h.command.is_empty() || h.command[0].is_empty() || h.timeout_seconds == 0 {
                return fail("Hook requires a nonempty command and positive timeout-seconds");
            }
            for input in &h.inputs {
                let (base, relative) = if let Some(relative) = input.strip_prefix("workspace:") {
                    (workspace, relative)
                } else {
                    (p.manifest.parent().unwrap(), input.as_str())
                };
                regular(&source(base, relative)?)?;
            }
        }
        for target in &config.targets {
            if !b.targets.is_empty() && !b.targets.contains(target) {
                continue;
            }
            let format = b
                .formats
                .get(target)
                .ok_or_else(|| Error(format!("Bundle {} has no format for {target}", b.name)))?;
            let root = render(&b.root, b, p, target)?;
            component(&root, windows(target))?;
            let name = archive_name(b, p, target, *format)?;
            if !assets.insert(name.clone()) {
                return fail(format!("Artifact filename collision: {name}"));
            }
            let mut inputs = BTreeMap::new();
            for binary in selected(b, p)? {
                insert_destination(
                    &mut inputs,
                    &policy::binary_filename(binary, target),
                    windows(target),
                )?;
            }
            for f in &b.files {
                let dest = render(&f.destination, b, p, target)?;
                let path = source(
                    if f.workspace_relative {
                        workspace
                    } else {
                        p.manifest.parent().unwrap()
                    },
                    &f.source,
                )?;
                validate_tree(&path, &dest, windows(target), &mut inputs)?;
            }
            println!(
                "Bundle {} / {target}: {name}, root {root}, format {format:?}; static files {:?}",
                b.name, b.files
            );
            for h in [&b.prepare, &b.smoke_test].into_iter().flatten() {
                for required in &h.required_destinations {
                    safe(&render(required, b, p, target)?, windows(target))?;
                }
                println!(
                    "  host command {:?}; required {:?}; dynamic results unresolved",
                    h.command, h.required_destinations
                );
            }
        }
    }
    Ok(())
}
fn archive_name(b: &Bundle, p: &SelectedPackage, t: &str, f: Format) -> Result<String, Error> {
    let n = render(&b.archive_name, b, p, t)?;
    component(&n, windows(t))?;
    Ok(format!(
        "{n}.{}",
        match f {
            Format::Tgz => "tgz",
            Format::Zip => "zip",
        }
    ))
}
fn insert_destination(map: &mut BTreeMap<String, ()>, dest: &str, win: bool) -> Result<(), Error> {
    safe(dest, win)?;
    let key = if win {
        dest.to_lowercase()
    } else {
        dest.into()
    };
    if map.keys().any(|k| {
        k == &key || k.starts_with(&format!("{key}/")) || key.starts_with(&format!("{k}/"))
    }) {
        return fail(format!("Bundle destination collision: {dest}"));
    }
    map.insert(key, ());
    Ok(())
}
fn validate_tree(
    path: &Path,
    dest: &str,
    win: bool,
    map: &mut BTreeMap<String, ()>,
) -> Result<(), Error> {
    safe(dest, win)?;
    let m = fs::symlink_metadata(path)?;
    if m.is_dir() {
        for child in fs::read_dir(path)? {
            let child = child?;
            let name = child
                .file_name()
                .into_string()
                .map_err(|_| Error("Non-UTF8 input filename".into()))?;
            validate_tree(&child.path(), &format!("{dest}/{name}"), win, map)?;
        }
    } else {
        regular(path)?;
        insert_destination(map, dest, win)?;
    }
    Ok(())
}
fn add(
    inputs: &mut Vec<Input>,
    path: &Path,
    dest: &str,
    kind: Kind,
    target: &str,
    expected: Option<&str>,
    map: &mut BTreeMap<String, ()>,
) -> Result<(), Error> {
    regular(path)?;
    insert_destination(map, dest, windows(target))?;
    let hash = package::checksum(path)?;
    if expected.is_some_and(|s| s != hash) {
        return fail(format!("SHA-256 mismatch: {dest}"));
    }
    if matches!(kind, Kind::Executable) {
        architecture(path, target)?;
    }
    inputs.push(Input {
        source: path.into(),
        entry: Entry {
            destination: dest.into(),
            sha256: hash,
            mode: if matches!(kind, Kind::Executable) {
                0o755
            } else {
                0o644
            },
        },
    });
    Ok(())
}
fn add_cargo(
    inputs: &mut Vec<Input>,
    path: &Path,
    staging: &Path,
    dest: &str,
    target: &str,
    map: &mut BTreeMap<String, ()>,
) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return fail(format!(
            "Not a regular Cargo executable: {}",
            path.display()
        ));
    }
    insert_destination(map, dest, windows(target))?;
    architecture(path, target)?;
    let source_hash = package::checksum(path)?;
    fs::create_dir_all(staging)?;
    let mut staged = tempfile::NamedTempFile::new_in(staging)?;
    let mut source = fs::File::open(path)?;
    std::io::copy(&mut source, staged.as_file_mut())?;
    staged.as_file_mut().flush()?;
    fs::set_permissions(staged.path(), metadata.permissions())?;
    let staged_hash = package::checksum(staged.path())?;
    if staged_hash != source_hash || package::checksum(path)? != source_hash {
        return fail(format!(
            "Cargo executable changed during staging: {}",
            path.display()
        ));
    }
    regular(staged.path())?;
    architecture(staged.path(), target)?;
    let (_, staged_path) = staged
        .keep()
        .map_err(|error| Error(format!("cannot retain staged Cargo executable: {error}")))?;
    inputs.push(Input {
        source: staged_path,
        entry: Entry {
            destination: dest.into(),
            sha256: source_hash,
            mode: 0o755,
        },
    });
    Ok(())
}
fn add_tree(
    inputs: &mut Vec<Input>,
    path: &Path,
    dest: &str,
    kind: Kind,
    target: &str,
    map: &mut BTreeMap<String, ()>,
) -> Result<(), Error> {
    let m = fs::symlink_metadata(path)?;
    if m.is_dir() {
        for child in fs::read_dir(path)? {
            let child = child?;
            let name = child
                .file_name()
                .into_string()
                .map_err(|_| Error("Non-UTF8 filename".into()))?;
            add_tree(
                inputs,
                &child.path(),
                &format!("{dest}/{name}"),
                kind,
                target,
                map,
            )?;
        }
        Ok(())
    } else {
        add(inputs, path, dest, kind, target, None, map)
    }
}
fn architecture(path: &Path, target: &str) -> Result<(), Error> {
    use std::io::{Seek, SeekFrom};
    let arch = target.split('-').next().unwrap_or("");
    let mut f = fs::File::open(path)?;
    let mut h = [0u8; 64];
    f.read_exact(&mut h)?;
    let valid = if windows(target) && &h[..2] == b"MZ" {
        let offset = u32::from_le_bytes(h[60..64].try_into().unwrap());
        f.seek(SeekFrom::Start(offset.into()))?;
        let mut pe = [0; 6];
        f.read_exact(&mut pe)?;
        &pe[..4] == b"PE\0\0"
            && Some(u16::from_le_bytes([pe[4], pe[5]]))
                == match arch {
                    "x86_64" => Some(0x8664),
                    "aarch64" => Some(0xaa64),
                    "i686" => Some(0x14c),
                    _ => None,
                }
    } else if target.contains("linux") && &h[..4] == b"\x7fELF" && h[5] == 1 {
        h[4] == if arch == "i686" { 1 } else { 2 }
            && Some(u16::from_le_bytes([h[18], h[19]]))
                == match arch {
                    "x86_64" => Some(62),
                    "aarch64" => Some(183),
                    "i686" => Some(3),
                    "riscv64gc" => Some(243),
                    _ => None,
                }
    } else {
        false
    };
    if !valid {
        return fail(format!(
            "Executable format/architecture mismatch or unsupported target {target}: {}",
            path.display()
        ));
    }
    Ok(())
}

pub fn assemble(
    workspace: &Path,
    p: &SelectedPackage,
    config: &Config,
    outputs: &[TargetOutputs],
    commit: &str,
    dirty: bool,
    runner: &Runner,
) -> Result<(Vec<Artifact>, PathBuf), Error> {
    // The outer temporary directory removes all partial artifacts on any failure.
    let dist = workspace.join("dist");
    if dist.is_symlink() {
        return fail("Symlinked dist rejected");
    }
    fs::create_dir_all(&dist)?;
    let stage = tempfile::Builder::new()
        .prefix("bundles-")
        .tempdir_in(&dist)?;
    let (mut artifacts, old_sums) = package::package(
        stage.path(),
        &p.name,
        &p.version,
        outputs,
        config.package_format,
    )?;
    let artifact_dir = old_sums.parent().unwrap().to_path_buf();
    let cargo_stage = stage.path().join("cargo-inputs");
    let mut records = Vec::new();
    for b in &config.bundles {
        for output in outputs {
            let target = &output.target;
            if !b.targets.is_empty() && !b.targets.contains(target) {
                continue;
            }
            let format = *b
                .formats
                .get(target)
                .ok_or_else(|| Error(format!("Missing bundle format for {target}")))?;
            let root = render(&b.root, b, p, target)?;
            let mut inputs = Vec::new();
            let mut map = BTreeMap::new();
            for binary in selected(b, p)? {
                let path = &output
                    .binaries
                    .iter()
                    .find(|(n, _)| n == binary)
                    .ok_or_else(|| Error(format!("Missing built binary {binary}")))?
                    .1;
                // Cargo may hardlink this path to release/deps. Copy the validated bytes into
                // publisher-owned staging while retaining strict link checks for other inputs.
                add_cargo(
                    &mut inputs,
                    path,
                    &cargo_stage,
                    &policy::binary_filename(binary, target),
                    target,
                    &mut map,
                )?;
            }
            for f in &b.files {
                let path = source(
                    if f.workspace_relative {
                        workspace
                    } else {
                        p.manifest.parent().unwrap()
                    },
                    &f.source,
                )?;
                add_tree(
                    &mut inputs,
                    &path,
                    &render(&f.destination, b, p, target)?,
                    f.kind,
                    target,
                    &mut map,
                )?;
            }
            let hook_stage = tempfile::tempdir_in(stage.path())?;
            let output_dir = hook_stage.path().join("output");
            fs::create_dir(&output_dir)?;
            let cache = p
                .target_directory
                .join("rust-release-bundle-cache")
                .join(&b.name)
                .join(target);
            fs::create_dir_all(&cache)?;
            let context = hook_stage.path().join("context.json");
            let manifest = hook_stage.path().join("result.json");
            let binaries = output
                .binaries
                .iter()
                .map(|(n, path)| Ok((n.clone(), fs::canonicalize(path)?)))
                .collect::<Result<BTreeMap<_, _>, Error>>()?;
            let tree = hook_stage.path().join("tree");
            fs::create_dir(&tree)?;
            let value = serde_json::json!({"schema_version":1,"package_name":p.name,"package_version":p.version,"target":target,"profile":config.profile,"binary_ext":if windows(target){".exe"}else{""},"workspace_root":fs::canonicalize(workspace)?,"package_root":fs::canonicalize(p.manifest.parent().unwrap())?,"source_commit":commit,"source_dirty":dirty,"binaries":binaries,"output_directory":fs::canonicalize(&output_dir)?,"cache_directory":fs::canonicalize(&cache)?,"manifest_path":manifest,"bundle":b.name,"bundle_directory":tree});
            fs::write(
                &context,
                serde_json::to_vec_pretty(&value).map_err(|e| Error(e.to_string()))?,
            )?;
            if let Some(h) = &b.prepare {
                runner.hook(
                    &h.command,
                    p.manifest.parent().unwrap(),
                    (&context, &manifest, &tree),
                    h.timeout_seconds,
                    &format!("{target}/{}", b.name),
                )?;
                regular(&manifest)?;
                let result: ResultManifest = serde_json::from_slice(&fs::read(&manifest)?)
                    .map_err(|e| Error(format!("Invalid hook manifest: {e}")))?;
                if result.schema_version != 1 || result.target != *target {
                    return fail("Hook manifest schema_version/target mismatch");
                }
                for f in result.files {
                    let path = source(&output_dir, &f.source)?;
                    add(
                        &mut inputs,
                        &path,
                        &f.destination,
                        f.kind,
                        target,
                        Some(&f.sha256),
                        &mut map,
                    )?;
                }
                for required in &h.required_destinations {
                    let required = render(required, b, p, target)?;
                    if !inputs.iter().any(|i| i.entry.destination == required) {
                        return fail(format!("Missing required hook destination {required}"));
                    }
                }
            }
            inputs.sort_by(|a, b| a.entry.destination.cmp(&b.entry.destination));
            for input in &inputs {
                let dest = tree.join(&input.entry.destination);
                fs::create_dir_all(dest.parent().unwrap())?;
                fs::copy(&input.source, &dest)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&dest, fs::Permissions::from_mode(input.entry.mode))?;
                }
                if package::checksum(&dest)? != input.entry.sha256 {
                    return fail("Input changed during staging");
                }
            }
            let smoke = if let Some(h) = &b.smoke_test {
                let host = runner
                    .text("rustc", &["-vV"], workspace)?
                    .lines()
                    .find_map(|l| l.strip_prefix("host: ").map(str::to_owned))
                    .unwrap_or_default();
                if host == *target {
                    runner.hook(
                        &h.command,
                        p.manifest.parent().unwrap(),
                        (&context, &manifest, &tree),
                        h.timeout_seconds,
                        &format!("{target}/{}/smoke", b.name),
                    )?;
                    "passed"
                } else {
                    "skipped: target differs from rustc host"
                }
            } else {
                "skipped: no native smoke-test configured"
            };
            // Smoke tests must not silently alter the declared tree.
            let entries = inputs.iter().map(|i| i.entry.clone()).collect::<Vec<_>>();
            let path = artifact_dir.join(archive_name(b, p, target, format)?);
            write_archive(&path, &tree, &root, format, &entries)?;
            let artifact = Artifact {
                target: target.clone(),
                sha256: package::checksum(&path)?,
                path,
                contents: ArtifactContents::Bundle {
                    root: root.clone(),
                    format,
                    entries: entries.clone(),
                },
            };
            verify(&artifact.path, &root, format, &entries)?;
            println!(
                "{}: portable tree/hash/architecture checks passed; native runtime {smoke}",
                artifact.path.display()
            );
            let filename = artifact_filename(&artifact.path)?;
            records.push(serde_json::json!({"bundle":b.name,"target":target,"filename":filename,"sha256":artifact.sha256,"root":root,"files":entries,"checks":{"portable_tree":"passed","hashes":"passed","architecture":"passed","native_runtime":smoke}}));
            artifacts.push(artifact);
        }
    }
    for a in &artifacts {
        if matches!(a.contents, ArtifactContents::Archive { .. }) {
            crate::verify::verify_archive(&a.path, a)?;
        }
        if !matches!(a.contents, ArtifactContents::Bundle { .. }) {
            let filename = artifact_filename(&a.path)?;
            records.push(serde_json::json!({"target":a.target,"filename":filename,"sha256":a.sha256,"checks":{"native_runtime":"skipped","cargo_binstall":"deferred until publication"}}));
        }
    }
    let receipt = artifact_dir.join("artifact-manifest.json");
    fs::write(
        &receipt,
        serde_json::to_vec_pretty(&serde_json::json!({"schema_version":1,"artifacts":records}))
            .map_err(|e| Error(e.to_string()))?,
    )?;
    artifacts.push(Artifact {
        target: "manifest".into(),
        sha256: package::checksum(&receipt)?,
        path: receipt,
        contents: ArtifactContents::Binary,
    });
    artifacts.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));
    fs::write(
        &old_sums,
        artifacts
            .iter()
            .map(|a| {
                format!(
                    "{}  {}\n",
                    a.sha256,
                    a.path.file_name().unwrap().to_string_lossy()
                )
            })
            .collect::<String>(),
    )?;
    // Stable local discovery entry; keep public manifests free of host paths.
    let mut locator = tempfile::NamedTempFile::new_in(&dist)?;
    locator.write_all(&serde_json::to_vec_pretty(&serde_json::json!({
        "schema_version": 1,
        "manifest": artifact_dir.join("artifact-manifest.json").strip_prefix(&dist).map_err(|e| Error(e.to_string()))?,
        "checksums": old_sums.strip_prefix(&dist).map_err(|e| Error(e.to_string()))?
    })).map_err(|e| Error(e.to_string()))?)?;
    locator
        .persist(dist.join("latest-artifacts.json"))
        .map_err(|e| Error(e.to_string()))?;
    let _ = stage.keep();
    Ok((artifacts, old_sums))
}
fn artifact_filename(path: &Path) -> Result<String, Error> {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error("Invalid artifact filename".into()))?;
    component(filename, false)?;
    Ok(filename.to_owned())
}
fn zip_error(e: zip::result::ZipError) -> Error {
    Error(e.to_string())
}
fn write_archive(
    path: &Path,
    tree: &Path,
    root: &str,
    format: Format,
    entries: &[Entry],
) -> Result<(), Error> {
    let temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    match format {
        Format::Tgz => {
            let gz = flate2::GzBuilder::new()
                .mtime(0)
                .operating_system(255)
                .write(temporary.reopen()?, flate2::Compression::default());
            let mut tar = tar::Builder::new(gz);
            let mut h = tar::Header::new_ustar();
            h.set_entry_type(tar::EntryType::Directory);
            h.set_mode(0o755);
            h.set_size(0);
            h.set_mtime(0);
            h.set_uid(0);
            h.set_gid(0);
            h.set_cksum();
            tar.append_data(&mut h, root, std::io::empty())?;
            for e in entries {
                let source = source(tree, &e.destination)?;
                regular(&source)?;
                if package::checksum(&source)? != e.sha256 {
                    return fail("Staged content changed");
                }
                let mut h = tar::Header::new_ustar();
                h.set_size(fs::metadata(&source)?.len());
                h.set_mode(e.mode);
                h.set_uid(0);
                h.set_gid(0);
                h.set_mtime(0);
                h.set_cksum();
                tar.append_data(
                    &mut h,
                    format!("{root}/{}", e.destination),
                    fs::File::open(source)?,
                )?;
            }
            tar.into_inner()?.finish()?;
        }
        Format::Zip => {
            let mut zip = zip::ZipWriter::new(temporary.reopen()?);
            for e in entries {
                let source = source(tree, &e.destination)?;
                regular(&source)?;
                if package::checksum(&source)? != e.sha256 {
                    return fail("Staged content changed");
                }
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .last_modified_time(zip::DateTime::default())
                    .unix_permissions(e.mode);
                zip.start_file(format!("{root}/{}", e.destination), options)
                    .map_err(zip_error)?;
                std::io::copy(&mut fs::File::open(source)?, &mut zip)?;
            }
            zip.finish().map_err(zip_error)?;
        }
    }
    temporary.persist(path).map_err(|e| Error(e.to_string()))?;
    Ok(())
}
pub fn verify(path: &Path, root: &str, format: Format, entries: &[Entry]) -> Result<(), Error> {
    use sha2::{Digest, Sha256};
    let mut seen = BTreeSet::new();
    let mut check = |name: String, mode: u32, reader: &mut dyn Read| -> Result<(), Error> {
        let dest = name
            .strip_prefix(&format!("{root}/"))
            .ok_or_else(|| Error("Unexpected archive root".into()))?;
        let expected = entries
            .iter()
            .find(|e| e.destination == dest)
            .ok_or_else(|| Error(format!("Unexpected archive member {name}")))?;
        if !seen.insert(dest.to_owned()) || mode & 0o777 != expected.mode {
            return fail("Duplicate archive member or incorrect permissions");
        }
        let mut hash = Sha256::new();
        let mut buf = [0; 65536];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
        }
        if format!("{:x}", hash.finalize()) != expected.sha256 {
            return fail("Archive member SHA-256 mismatch");
        }
        Ok(())
    };
    match format {
        Format::Tgz => {
            let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(path)?));
            let mut root_seen = false;
            for entry in tar.entries()? {
                let mut entry = entry?;
                let name = entry
                    .path()?
                    .to_str()
                    .ok_or_else(|| Error("Non-UTF8 archive path".into()))?
                    .to_owned();
                if entry.header().entry_type().is_dir() && name == root && !root_seen {
                    root_seen = true;
                    continue;
                }
                if !entry.header().entry_type().is_file() {
                    return fail("Unexpected non-file archive member");
                }
                check(name, entry.header().mode()?, &mut entry)?;
            }
            if !root_seen {
                return fail("Missing archive root");
            }
        }
        Format::Zip => {
            let mut zip = zip::ZipArchive::new(fs::File::open(path)?).map_err(zip_error)?;
            for i in 0..zip.len() {
                let mut entry = zip.by_index(i).map_err(zip_error)?;
                let mode = entry.unix_mode().unwrap_or(0);
                if mode & 0o170000 != 0o100000 {
                    return fail("Non-regular ZIP member");
                }
                check(entry.name().into(), mode, &mut entry)?;
            }
        }
    }
    if seen.len() != entries.len() {
        return fail("Missing archive members");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (
        tempfile::TempDir,
        SelectedPackage,
        Config,
        Vec<TargetOutputs>,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir(root.join("member")).unwrap();
        fs::write(root.join("member/LICENSE"), "license").unwrap();
        fs::write(root.join("member/README.txt"), "portable instructions").unwrap();
        let mut outputs = Vec::new();
        for target in ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-gnu"] {
            let path = root.join(format!("custom output é {target}"));
            let mut bytes = vec![0; 128];
            if windows(target) {
                bytes[..2].copy_from_slice(b"MZ");
                bytes[60] = 64;
                bytes[64..68].copy_from_slice(b"PE\0\0");
                bytes[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
            } else {
                bytes[..4].copy_from_slice(b"\x7fELF");
                bytes[4] = 2;
                bytes[5] = 1;
                bytes[18] = 62;
            }
            fs::write(&path, bytes).unwrap();
            outputs.push(TargetOutputs {
                target: target.into(),
                binaries: vec![("demo".into(), path.clone()), ("gui".into(), path)],
            });
        }
        let p = SelectedPackage {
            workspace_root: root.into(),
            name: "demo".into(),
            version: "1.2.3".into(),
            binaries: vec!["demo".into(), "gui".into()],
            manifest: root.join("member/Cargo.toml"),
            repository: None,
            target_directory: root.join("custom-target"),
        };
        fs::write(root.join("member/prepare.py"),r#"import json, sys, pathlib, hashlib, shutil, os
c = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert c['schema_version'] == 1 and c['profile'] == 'small'
assert c['package_version'] == '1.2.3' and c['source_dirty']
assert pathlib.Path(c['package_root']).name == 'member'
assert pathlib.Path(c['binaries']['demo']).is_absolute()
assert 'RELEASE_TEST_TOKEN' not in os.environ
out = pathlib.Path(c['output_directory'])
files = []
for name, data, kind in [('runtime'+c['binary_ext'], pathlib.Path(c['binaries']['demo']).read_bytes(), 'executable'), ('LICENSE.txt', b'upstream license', 'file'), ('provenance.json', b'{"release_id":123}', 'file')]:
    (out / name).write_bytes(data)
    files.append(dict(source=name, destination=name if kind == 'executable' else 'LICENSES/runtime/'+name, kind=kind, sha256=hashlib.sha256(data).hexdigest()))
(out / 'undeclared-cache').write_text('must not be archived')
pathlib.Path(c['manifest_path']).write_text(json.dumps(dict(schema_version=1, target=c['target'], files=files)))
"#).unwrap();
        let bundle:Bundle=toml::from_str(r#"
name='portable'
targets=['x86_64-unknown-linux-gnu','x86_64-pc-windows-gnu']
formats={x86_64-unknown-linux-gnu='tgz',x86_64-pc-windows-gnu='zip'}
archive-name='{name}-{bundle}-{target}-v{version}'
root='{name}-{bundle}-{target}-v{version}'
cargo-binaries='all'
files=[{source='LICENSE',destination='LICENSES/app.txt'},{source='README.txt',destination='README.txt'}]
[prepare]
command=['python3','prepare.py','{context-file}']
inputs=['prepare.py']
timeout-seconds=10
required-destinations=['runtime{binary-ext}','LICENSES/runtime/LICENSE.txt','LICENSES/runtime/provenance.json']
"#).unwrap();
        let config = Config {
            bundles: vec![bundle],
            features: Vec::new(),
            apple: crate::config::AppleConfig::default(),
            targets: outputs.iter().map(|o| o.target.clone()).collect(),
            profile: "small".into(),
            locked: true,
            tag_prefix: "v".into(),
            token_env: "RELEASE_TEST_TOKEN".into(),
            package_format: crate::config::PackageFormat::Tgz,
            repository_fallbacks: Vec::new(),
        };
        (temp, p, config, outputs)
    }
    #[test]
    fn complete_archives_are_deterministic_and_manifest_is_public() {
        let (temp, p, c, outputs) = fixture();
        plan(&c, &p, temp.path()).unwrap();
        assert!(!p.target_directory.exists()); // planning does not run preparation
        let runner = Runner::new("RELEASE_TEST_TOKEN");
        let (first, sums) = assemble(temp.path(), &p, &c, &outputs, "abc", true, &runner).unwrap();
        let (second, _) = assemble(temp.path(), &p, &c, &outputs, "abc", true, &runner).unwrap();
        assert_eq!(first.len(), 5);
        assert_eq!(fs::read_to_string(sums).unwrap().lines().count(), 5);
        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(a.sha256, b.sha256);
            if let ArtifactContents::Bundle {
                root,
                format,
                entries,
            } = &a.contents
            {
                assert_eq!(entries.len(), 7);
                verify(&a.path, root, *format, entries).unwrap();
                assert!(!entries.iter().any(|e| e.destination.contains("undeclared")));
            }
        }
        let receipt = first.iter().find(|a| a.target == "manifest").unwrap();
        let text = fs::read_to_string(&receipt.path).unwrap();
        assert!(!text.contains(&temp.path().display().to_string()));
        let public_manifest: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(public_manifest["schema_version"], 1);
        let records = public_manifest["artifacts"].as_array().unwrap();
        assert_eq!(records.len(), 4);
        for record in records {
            assert!(record["target"].is_string());
            assert!(record["filename"].is_string());
            assert_eq!(record["sha256"].as_str().unwrap().len(), 64);
            assert!(record["checks"].is_object());
        }
        fs::write(temp.path().join("member/LICENSE"), "new license").unwrap();
        let (third, _) = assemble(temp.path(), &p, &c, &outputs, "abc", true, &runner).unwrap();
        assert_ne!(
            first
                .iter()
                .find(|a| matches!(a.contents, ArtifactContents::Bundle { .. }))
                .unwrap()
                .sha256,
            third
                .iter()
                .find(|a| matches!(a.contents, ArtifactContents::Bundle { .. }))
                .unwrap()
                .sha256
        );
    }
    #[test]
    fn invalid_paths_collisions_links_and_executables_are_rejected() {
        for path in [
            "/a",
            "../a",
            "a/../b",
            "a\\b",
            "C:/a",
            "a//b",
            "NUL.txt",
            "a/COM1.exe",
            "x.",
            "x ",
        ] {
            assert!(safe(path, true).is_err(), "{path}");
        }
        let mut map = BTreeMap::new();
        insert_destination(&mut map, "Demo.exe", true).unwrap();
        assert!(insert_destination(&mut map, "demo.EXE", true).is_err());
        assert!(insert_destination(&mut map, "Demo.exe/child", true).is_err());
        let (temp, _, _, outputs) = fixture();
        assert!(architecture(&outputs[0].binaries[0].1, &outputs[1].target).is_err());
        #[cfg(unix)]
        {
            let link = temp.path().join("link");
            std::os::unix::fs::symlink(&outputs[0].binaries[0].1, &link).unwrap();
            assert!(source(temp.path(), "link").is_err());
            fs::remove_file(&link).unwrap();
            fs::hard_link(&outputs[0].binaries[0].1, &link).unwrap();
            assert!(regular(&link).is_err());
        }
    }
    #[cfg(unix)]
    #[test]
    fn hardlinked_cargo_executable_is_copied_to_independent_staging() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let (temp, _, _, outputs) = fixture();
        let source = &outputs[0].binaries[0].1;
        fs::set_permissions(source, fs::Permissions::from_mode(0o751)).unwrap();
        let other_link = temp.path().join("cargo-deps-link");
        fs::hard_link(source, &other_link).unwrap();
        assert_eq!(fs::metadata(source).unwrap().nlink(), 2);

        let mut inputs = Vec::new();
        add_cargo(
            &mut inputs,
            source,
            &temp.path().join("publisher-staging"),
            "demo",
            "x86_64-unknown-linux-gnu",
            &mut BTreeMap::new(),
        )
        .unwrap();

        assert_eq!(inputs.len(), 1);
        let staged = &inputs[0].source;
        assert_eq!(fs::read(staged).unwrap(), fs::read(source).unwrap());
        assert_eq!(fs::metadata(staged).unwrap().nlink(), 1);
        assert_eq!(
            fs::metadata(staged).unwrap().permissions().mode() & 0o777,
            0o751
        );
        assert_eq!(inputs[0].entry.sha256, package::checksum(source).unwrap());
        assert_eq!(fs::metadata(source).unwrap().nlink(), 2);
    }
    #[test]
    fn failed_hooks_clean_partial_artifacts_and_filter_targets() {
        let (temp, p, mut c, outputs) = fixture();
        let runner = Runner::new("RELEASE_TEST_TOKEN");
        c.targets.truncate(1);
        plan(&c, &p, temp.path()).unwrap();
        let (artifacts, _) =
            assemble(temp.path(), &p, &c, &outputs[..1], "abc", true, &runner).unwrap();
        assert!(!artifacts.iter().any(|a| windows(&a.target)));
        let baseline = fs::read_dir(temp.path().join("dist")).unwrap().count();
        let original = fs::read_to_string(temp.path().join("member/prepare.py")).unwrap();
        for tail in ["raise SystemExit(7)","pathlib.Path(c['manifest_path']).write_text('bad json')", "files[0]['sha256']='0'*64; pathlib.Path(c['manifest_path']).write_text(json.dumps(dict(schema_version=1,target=c['target'],files=files)))", "files[0]['destination']='demo'; pathlib.Path(c['manifest_path']).write_text(json.dumps(dict(schema_version=1,target=c['target'],files=files)))", "pathlib.Path(c['manifest_path']).write_text(json.dumps(dict(schema_version=1,target=c['target'],files=[])))", "import time; time.sleep(5)"] {
            fs::write(temp.path().join("member/prepare.py"),format!("{original}\n{tail}\n")).unwrap();c.bundles[0].prepare.as_mut().unwrap().timeout_seconds=1;
            assert!(assemble(temp.path(),&p,&c,&outputs[..1],"abc",true,&runner).is_err(),"{tail}");assert_eq!(fs::read_dir(temp.path().join("dist")).unwrap().count(),baseline);
        }
    }
    #[test]
    fn published_bundle_downloads_validate_full_contents() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            thread,
        };
        let (temp, p, c, outputs) = fixture();
        let (artifacts, sums) = assemble(
            temp.path(),
            &p,
            &c,
            &outputs,
            "abc",
            true,
            &Runner::new("RELEASE_TEST_TOKEN"),
        )
        .unwrap();
        let bodies = artifacts
            .iter()
            .map(|a| fs::read(&a.path).unwrap())
            .chain(std::iter::once(fs::read(&sums).unwrap()))
            .collect::<Vec<_>>();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let repo = format!("http://{}/team/demo", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            for body in bodies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        crate::verify::verify_published(&repo, "v1.2.3", &artifacts, &sums, None).unwrap();
        server.join().unwrap();
    }
    #[test]
    fn native_smoke_checks_are_explicit_and_cannot_change_payloads() {
        let (temp, p, mut c, outputs) = fixture();
        let host = Runner::new("TOKEN")
            .text("rustc", &["-vV"], temp.path())
            .unwrap();
        if !host.lines().any(|l| l == "host: x86_64-unknown-linux-gnu") {
            return;
        }
        c.bundles[0].smoke_test = Some(Hook {
            command: vec![
                "python3".into(),
                "-c".into(),
                "import pathlib,sys; assert (pathlib.Path(sys.argv[1])/'runtime').is_file()".into(),
                "{bundle-directory}".into(),
            ],
            inputs: vec![],
            timeout_seconds: 10,
            required_destinations: vec![],
        });
        let (artifacts, _) = assemble(
            temp.path(),
            &p,
            &c,
            &outputs,
            "abc",
            true,
            &Runner::new("TOKEN"),
        )
        .unwrap();
        let manifest = fs::read_to_string(
            &artifacts
                .iter()
                .find(|a| a.target == "manifest")
                .unwrap()
                .path,
        )
        .unwrap();
        assert!(manifest.contains("skipped: target differs"));
        assert!(manifest.contains(r#""native_runtime": "passed""#));
        c.bundles[0].smoke_test.as_mut().unwrap().command[2] =
            "import pathlib,sys; (pathlib.Path(sys.argv[1])/'runtime').write_text('changed')"
                .into();
        assert!(assemble(
            temp.path(),
            &p,
            &c,
            &outputs[..1],
            "abc",
            true,
            &Runner::new("TOKEN")
        )
        .is_err());
    }
    #[test]
    fn config_rejects_ambiguous_artifacts_and_missing_formats() {
        let (temp, p, mut c, _) = fixture();
        c.bundles[0].archive_name = "{name}-{target}-v{version}".into();
        assert!(plan(&c, &p, temp.path()).is_err());
        c.bundles[0].archive_name = "{name}-{bundle}-{target}".into();
        c.bundles[0].formats.clear();
        assert!(plan(&c, &p, temp.path()).is_err());
        assert!(toml::from_str::<Bundle>("name='x'\nunknown=true").is_err());
    }
}
