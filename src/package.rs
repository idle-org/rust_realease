use crate::{build::TargetOutputs, config::PackageFormat, error::Error, policy};
use flate2::{Compression, GzBuilder};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct Artifact {
    pub target: String,
    pub path: PathBuf,
    pub contents: ArtifactContents,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub enum ArtifactContents {
    Archive { root: String, binaries: Vec<String> },
    Binary,
}

pub fn package(
    root: &Path,
    name: &str,
    version: &str,
    outputs: &[TargetOutputs],
    format: PackageFormat,
) -> Result<(Vec<Artifact>, PathBuf), Error> {
    for output in outputs {
        safe_component(&policy::archive_root(name, &output.target, version))?;
        safe_component(&policy::archive_name(name, &output.target, version))?;
        for (binary, _) in &output.binaries {
            safe_component(&policy::binary_filename(binary, &output.target))?;
            safe_component(&policy::binary_asset_name(binary, &output.target, version))?;
        }
    }
    let dist = root.join("dist");
    if dist.is_symlink() {
        return Err(Error("Refusing a symlinked dist directory.".into()));
    }
    fs::create_dir_all(&dist)
        .map_err(|e| Error(format!("cannot create {}: {e}", dist.display())))?;
    let staging = tempfile::Builder::new()
        .prefix("release-")
        .tempdir_in(&dist)
        .map_err(|e| Error(format!("cannot create release staging directory: {e}")))?;
    let mut artifacts = Vec::new();
    for output in outputs {
        for (_, source) in &output.binaries {
            let metadata = fs::symlink_metadata(source)
                .map_err(|e| Error(format!("cannot inspect binary {}: {e}", source.display())))?;
            if !metadata.file_type().is_file() {
                return Err(Error(format!(
                    "Build did not produce a regular binary: {}",
                    source.display()
                )));
            }
        }
        if format.archives() {
            let archive_root = policy::archive_root(name, &output.target, version);
            let path = staging
                .path()
                .join(policy::archive_name(name, &output.target, version));
            let archive = File::create(&path)
                .map_err(|e| Error(format!("cannot create archive {}: {e}", path.display())))?;
            let mut gzip = GzBuilder::new()
                .mtime(0)
                .operating_system(255)
                .write(archive, Compression::default());
            write_header(&mut gzip, &archive_root, 0o755, 0, b'5')?;
            let mut names = Vec::new();
            for (binary, source) in &output.binaries {
                let metadata = fs::symlink_metadata(source).map_err(|e| {
                    Error(format!("cannot inspect binary {}: {e}", source.display()))
                })?;
                let executable = policy::binary_filename(binary, &output.target);
                let mode = if output.target.split('-').any(|part| part == "windows") {
                    0o644
                } else {
                    unix_mode(&metadata) | 0o111
                };
                write_header(
                    &mut gzip,
                    &format!("{archive_root}/{executable}"),
                    mode,
                    metadata.len(),
                    b'0',
                )?;
                let mut input = File::open(source)
                    .map_err(|e| Error(format!("cannot open binary {}: {e}", source.display())))?;
                let copied = io::copy(&mut input, &mut gzip)
                    .map_err(|e| Error(format!("cannot archive {}: {e}", source.display())))?;
                if copied != metadata.len() {
                    return Err(Error(format!(
                        "Binary changed while packaging: {}",
                        source.display()
                    )));
                }
                let padding = (512 - metadata.len() % 512) % 512;
                gzip.write_all(&vec![0; padding as usize])
                    .map_err(|e| Error(format!("cannot pad archive: {e}")))?;
                names.push(executable);
            }
            gzip.write_all(&[0; 1024])
                .map_err(|e| Error(format!("cannot finish tar archive: {e}")))?;
            gzip.finish()
                .map_err(|e| Error(format!("cannot finish gzip archive: {e}")))?;
            artifacts.push(Artifact {
                target: output.target.clone(),
                sha256: checksum(&path)?,
                path,
                contents: ArtifactContents::Archive {
                    root: archive_root,
                    binaries: names,
                },
            });
        }
        if format.binaries() {
            for (binary, source) in &output.binaries {
                let path =
                    staging
                        .path()
                        .join(policy::binary_asset_name(binary, &output.target, version));
                fs::copy(source, &path).map_err(|e| {
                    Error(format!(
                        "cannot export binary {} to {}: {e}",
                        source.display(),
                        path.display()
                    ))
                })?;
                artifacts.push(Artifact {
                    target: output.target.clone(),
                    sha256: checksum(&path)?,
                    path,
                    contents: ArtifactContents::Binary,
                });
            }
        }
    }
    let sums = staging.path().join("SHA256SUMS");
    artifacts.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));
    let mut checksums = String::new();
    for artifact in &artifacts {
        let filename = artifact
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| Error("Invalid archive filename.".into()))?;
        checksums.push_str(&format!("{}  {filename}\n", artifact.sha256));
    }
    fs::write(&sums, checksums)
        .map_err(|e| Error(format!("cannot write {}: {e}", sums.display())))?;
    let _ = staging.keep();
    Ok((artifacts, sums))
}

fn safe_component(component: &str) -> Result<(), Error> {
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.contains('/')
        || component.contains('\\')
        || component.bytes().any(|b| b == 0 || b < 32)
    {
        return Err(Error(format!("Unsafe archive filename: {component:?}")));
    }
    Ok(())
}

fn unix_mode(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0o755
    }
}

pub fn checksum(path: &Path) -> Result<String, Error> {
    let mut input =
        File::open(path).map_err(|e| Error(format!("cannot open {}: {e}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|e| Error(format!("cannot hash {}: {e}", path.display())))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn write_header<W: Write>(
    output: &mut W,
    path: &str,
    mode: u32,
    size: u64,
    kind: u8,
) -> Result<(), Error> {
    let mut header = [0_u8; 512];
    let (name, prefix) = if path.len() <= 100 {
        (path, "")
    } else {
        let (prefix, name) = path
            .rsplit_once('/')
            .ok_or_else(|| Error("Archive path exceeds USTAR name limit.".into()))?;
        if name.len() > 100 || prefix.len() > 155 {
            return Err(Error("Archive path exceeds USTAR limits.".into()));
        }
        (name, prefix)
    };
    header[..name.len()].copy_from_slice(name.as_bytes());
    header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    octal(&mut header[100..108], u64::from(mode))?;
    octal(&mut header[108..116], 0)?;
    octal(&mut header[116..124], 0)?;
    octal(&mut header[124..136], size)?;
    octal(&mut header[136..148], 0)?;
    header[148..156].fill(b' ');
    header[156] = kind;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let sum: u64 = header.iter().map(|byte| u64::from(*byte)).sum();
    let check = format!("{sum:06o}\0 ");
    if check.len() != 8 {
        return Err(Error("Tar header checksum overflow.".into()));
    }
    header[148..156].copy_from_slice(check.as_bytes());
    output
        .write_all(&header)
        .map_err(|e| Error(format!("cannot write tar header: {e}")))
}

fn octal(field: &mut [u8], number: u64) -> Result<(), Error> {
    let rendered = format!("{:0width$o}\0", number, width = field.len() - 1);
    if rendered.len() != field.len() {
        return Err(Error("Tar field exceeds USTAR limit.".into()));
    }
    field.copy_from_slice(rendered.as_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    #[test]
    fn archive_contract_and_reproducibility() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join("demo");
        let helper = temp.path().join("helper");
        fs::write(&binary, b"small executable").unwrap();
        fs::write(&helper, b"helper executable").unwrap();
        let outputs = [
            TargetOutputs {
                target: "x86_64-unknown-linux-musl".to_owned(),
                binaries: vec![
                    ("demo".into(), binary.clone()),
                    ("helper".into(), helper.clone()),
                ],
            },
            TargetOutputs {
                target: "x86_64-pc-windows-gnu".to_owned(),
                binaries: vec![("demo".into(), binary), ("helper".into(), helper)],
            },
        ];
        let (first, sums) =
            package(temp.path(), "demo", "0.1.0", &outputs, PackageFormat::Both).unwrap();
        assert_eq!(first.len(), 6);
        assert_eq!(fs::read_to_string(sums).unwrap().lines().count(), 6);
        let names = first
            .iter()
            .map(|artifact| artifact.path.file_name().unwrap().to_str().unwrap())
            .collect::<Vec<_>>();
        assert!(names.contains(&"demo-x86_64-unknown-linux-musl-v0.1.0"));
        assert!(names.contains(&"helper-x86_64-pc-windows-gnu-v0.1.0.exe"));
        let (binary_only, _) = package(
            temp.path(),
            "demo",
            "0.1.0",
            &outputs,
            PackageFormat::Binary,
        )
        .unwrap();
        assert_eq!(binary_only.len(), 4);
        assert!(binary_only
            .iter()
            .all(|artifact| matches!(&artifact.contents, ArtifactContents::Binary)));
        let (second, _) =
            package(temp.path(), "demo", "0.1.0", &outputs, PackageFormat::Both).unwrap();
        for (one, two) in first.iter().zip(second.iter()) {
            assert_eq!(fs::read(&one.path).unwrap(), fs::read(&two.path).unwrap());
            assert_eq!(one.sha256, checksum(&one.path).unwrap());
            let ArtifactContents::Archive { root, binaries } = &one.contents else {
                continue;
            };
            let mut tar = Vec::new();
            GzDecoder::new(File::open(&one.path).unwrap())
                .read_to_end(&mut tar)
                .unwrap();
            assert_eq!(&tar[..root.len()], root.as_bytes());
            assert_eq!(tar[156], b'5');
            let file_header = &tar[512..1024];
            let expected = format!("{}/{}", root, binaries[0]);
            assert_eq!(&file_header[..expected.len()], expected.as_bytes());
            assert_eq!(file_header[156], b'0');
            assert_eq!(&tar[1024..1024 + 16], b"small executable");
            let mode = std::str::from_utf8(&file_header[100..107]).unwrap();
            if one.target.contains("windows") {
                assert_eq!(mode, "0000644");
            } else {
                assert_eq!(mode, "0000755");
            }
        }
    }
    #[test]
    fn rejects_symlink_dist_and_source() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join("demo");
        fs::write(&binary, "binary").unwrap();
        let outputs = [TargetOutputs {
            target: "x86_64-unknown-linux-musl".into(),
            binaries: vec![("demo".into(), binary.clone())],
        }];
        assert!(package(
            temp.path(),
            "../escape",
            "0.1.0",
            &outputs,
            PackageFormat::Tgz,
        )
        .is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(temp.path(), temp.path().join("dist")).unwrap();
            assert!(package(temp.path(), "demo", "0.1.0", &outputs, PackageFormat::Tgz,).is_err());
        }
    }
}
