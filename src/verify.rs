use crate::{
    error::Error,
    package::{self, Artifact, ArtifactContents},
    policy,
    project::SelectedPackage,
    runner::Runner,
};
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    time::Duration,
};

pub fn asset_url(repository: &str, tag: &str, filename: &str) -> String {
    let encode =
        |s: &str| -> String { url::form_urlencoded::byte_serialize(s.as_bytes()).collect() };
    format!(
        "{repository}/releases/download/{}/{}",
        encode(tag),
        encode(filename)
    )
}

pub fn verify_archive(path: &Path, artifact: &Artifact) -> Result<(), Error> {
    if let ArtifactContents::Bundle {
        root,
        format,
        entries,
    } = &artifact.contents
    {
        return crate::bundle::verify(path, root, *format, entries);
    }
    let ArtifactContents::Archive { root, binaries } = &artifact.contents else {
        return Err(Error(format!("{} is not an archive.", path.display())));
    };
    let mut tar = Vec::new();
    let file =
        File::open(path).map_err(|e| Error(format!("cannot open {}: {e}", path.display())))?;
    GzDecoder::new(file)
        .read_to_end(&mut tar)
        .map_err(|e| Error(format!("{}: invalid gzip archive: {e}", path.display())))?;
    if tar.len() < 2048 {
        return Err(Error(format!("{}: truncated tar archive.", path.display())));
    }
    let directory = &tar[..512];
    let first = member_name(directory)?;
    if first != *root || directory[156] != b'5' {
        return Err(Error(format!(
            "{}: archive root does not match binstall metadata.",
            path.display()
        )));
    }
    let mut offset = 512_usize;
    for expected in binaries {
        let header_end = offset
            .checked_add(512)
            .ok_or_else(|| Error("Archive size overflow.".into()))?;
        if header_end > tar.len() {
            return Err(Error(format!(
                "{}: truncated binary header.",
                path.display()
            )));
        }
        let header = &tar[offset..header_end];
        if member_name(header)? != format!("{root}/{expected}") || header[156] != b'0' {
            return Err(Error(format!(
                "{}: archive binary does not match binstall metadata.",
                path.display()
            )));
        }
        let mode = octal(&header[100..108])?;
        if !artifact.target.split('-').any(|part| part == "windows") && mode & 0o111 == 0 {
            return Err(Error(format!(
                "{}: binary is not executable.",
                path.display()
            )));
        }
        let size = usize::try_from(octal(&header[124..136])?)
            .map_err(|_| Error("Archive size overflow.".into()))?;
        let rounded = size
            .checked_add(511)
            .ok_or_else(|| Error("Archive size overflow.".into()))?
            / 512
            * 512;
        offset = header_end
            .checked_add(rounded)
            .ok_or_else(|| Error("Archive size overflow.".into()))?;
    }
    let end = offset
        .checked_add(1024)
        .ok_or_else(|| Error("Archive size overflow.".into()))?;
    if end > tar.len() || tar[offset..].iter().any(|b| *b != 0) {
        return Err(Error(format!(
            "{}: archive contains unexpected members.",
            path.display()
        )));
    }
    Ok(())
}

pub fn verify_apple_artifact(
    path: &Path,
    artifact: &Artifact,
    deployment_target: &str,
) -> Result<(), Error> {
    match &artifact.contents {
        ArtifactContents::Binary => {
            crate::macho::verify_path(path, &artifact.target, deployment_target)
        }
        ArtifactContents::Archive { binaries, .. } => {
            let gzip = GzDecoder::new(
                File::open(path)
                    .map_err(|e| Error(format!("cannot open {}: {e}", path.display())))?,
            );
            let mut archive = tar::Archive::new(gzip);
            let mut checked = 0_usize;
            for entry in archive.entries().map_err(|e| {
                Error(format!(
                    "cannot inspect Apple archive {}: {e}",
                    path.display()
                ))
            })? {
                let mut entry = entry.map_err(|e| Error(format!("invalid Apple archive: {e}")))?;
                if !entry.header().entry_type().is_file() {
                    continue;
                }
                let name = entry
                    .path()
                    .map_err(|e| Error(format!("invalid Apple archive path: {e}")))?;
                let filename = name.file_name().and_then(|name| name.to_str());
                if !filename.is_some_and(|name| binaries.iter().any(|binary| binary == name)) {
                    return Err(Error(format!(
                        "unexpected Apple executable in {}",
                        path.display()
                    )));
                }
                let mut bytes = Vec::new();
                entry
                    .read_to_end(&mut bytes)
                    .map_err(|e| Error(format!("cannot read Apple archive member: {e}")))?;
                crate::macho::verify(&bytes, &artifact.target, deployment_target)?;
                checked += 1;
            }
            if checked != binaries.len() {
                return Err(Error(format!(
                    "{} does not contain every declared Apple binary",
                    path.display()
                )));
            }
            Ok(())
        }
        ArtifactContents::Bundle { .. } => Err(Error(
            "Apple named bundles require an explicit Mach-O deployment policy".into(),
        )),
    }
}

fn member_name(header: &[u8]) -> Result<String, Error> {
    let name = text_field(&header[..100])?;
    let prefix = text_field(&header[345..500])?;
    if prefix.is_empty() {
        Ok(name)
    } else {
        Ok(format!("{prefix}/{name}"))
    }
}

fn text_field(field: &[u8]) -> Result<String, Error> {
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    std::str::from_utf8(&field[..end])
        .map(str::to_owned)
        .map_err(|_| Error("Archive contains non-UTF-8 paths.".into()))
}

fn octal(field: &[u8]) -> Result<u64, Error> {
    let bytes = field
        .iter()
        .take_while(|b| **b != 0)
        .copied()
        .collect::<Vec<_>>();
    let string =
        std::str::from_utf8(&bytes).map_err(|_| Error("Invalid tar numeric field.".into()))?;
    u64::from_str_radix(string.trim(), 8).map_err(|_| Error("Invalid tar numeric field.".into()))
}

pub fn verify_published(
    repository: &str,
    tag: &str,
    artifacts: &[Artifact],
    sums: &Path,
    authenticated_client: Option<&Client>,
) -> Result<(), Error> {
    let downloads = tempfile::Builder::new()
        .prefix("rust-realease-download-")
        .tempdir()
        .map_err(|e| Error(format!("cannot create download directory: {e}")))?;
    let client = match authenticated_client {
        Some(client) => client.clone(),
        None => Client::builder()
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|_| Error("Cannot initialize anonymous download client.".into()))?,
    };
    for local in artifacts
        .iter()
        .map(|a| a.path.as_path())
        .chain(std::iter::once(sums))
    {
        let filename = local
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| Error("Invalid asset filename.".into()))?;
        let mut response = client
            .get(asset_url(repository, tag, filename))
            .send()
            .map_err(|_| Error(format!("{filename}: release download failed.")))?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(Error(format!(
                "{filename}: release download returned HTTP {}.",
                response.status().as_u16()
            )));
        }
        let expected = fs::metadata(local)
            .map_err(|e| {
                Error(format!(
                    "cannot inspect local asset {}: {e}",
                    local.display()
                ))
            })?
            .len();
        let path = downloads.path().join(filename);
        let mut output = File::create(&path)
            .map_err(|e| Error(format!("cannot create download {}: {e}", path.display())))?;
        let mut count = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let n = response
                .read(&mut buffer)
                .map_err(|_| Error(format!("{filename}: release download failed.")))?;
            if n == 0 {
                break;
            }
            count = count
                .checked_add(n as u64)
                .ok_or_else(|| Error("Download size overflow.".into()))?;
            if count > expected {
                return Err(Error(format!(
                    "{filename}: download exceeds expected size."
                )));
            }
            output
                .write_all(&buffer[..n])
                .map_err(|e| Error(format!("cannot write download: {e}")))?;
        }
        drop(output);
        if package::checksum(&path)? != package::checksum(local)? {
            return Err(Error(format!(
                "{filename}: published SHA-256 differs from local artifact."
            )));
        }
        if let Some(artifact) = artifacts.iter().find(|a| a.path == local) {
            match &artifact.contents {
                ArtifactContents::Archive { .. } | ArtifactContents::Bundle { .. } => {
                    verify_archive(&path, artifact)?;
                    println!("Verified URL, checksum and archive: {}", artifact.target);
                }
                ArtifactContents::Binary => {
                    println!("Verified URL and checksum: {}", filename);
                }
            }
        }
    }
    println!("Verified published SHA256SUMS");
    Ok(())
}

pub fn binstall_arguments(project: &SelectedPackage, target: &str, install: &Path) -> Vec<String> {
    vec![
        "binstall".into(),
        project.name.clone(),
        "--manifest-path".into(),
        project.manifest.display().to_string(),
        "--version".into(),
        format!("={}", project.version),
        "--targets".into(),
        target.into(),
        "--strategies".into(),
        "crate-meta-data".into(),
        "--no-confirm".into(),
        "--no-track".into(),
        "--disable-telemetry".into(),
        "--no-discover-github-token".into(),
        "--install-path".into(),
        install.display().to_string(),
    ]
}

pub fn verify_binstall(
    root: &Path,
    project: &SelectedPackage,
    targets: &[String],
    runner: &Runner,
) -> Result<(), Error> {
    verify_binstall_with_options(root, project, targets, runner, false)
}

fn verify_binstall_with_options(
    root: &Path,
    project: &SelectedPackage,
    targets: &[String],
    runner: &Runner,
    allow_insecure_http: bool,
) -> Result<(), Error> {
    for target in targets {
        let temp = tempfile::Builder::new()
            .prefix("rust-realease-binstall-")
            .tempdir()
            .map_err(|e| Error(format!("cannot create isolated install directory: {e}")))?;
        let cargo_home = temp.path().join("cargo-home");
        let install = temp.path().join("install");
        fs::create_dir_all(&cargo_home)
            .map_err(|e| Error(format!("cannot create isolated Cargo home: {e}")))?;
        fs::create_dir_all(&install)
            .map_err(|e| Error(format!("cannot create isolated install root: {e}")))?;
        let mut args = binstall_arguments(project, target, &install);
        if allow_insecure_http {
            args.push("--allow-insecure-http".into());
        }
        let borrowed: Vec<_> = args.iter().map(String::as_str).collect();
        let cargo_home = cargo_home
            .to_str()
            .ok_or_else(|| Error("Cargo home path is not UTF-8.".into()))?;
        runner.output_with("cargo", &borrowed, root, &[("CARGO_HOME", cargo_home)], &[])?;
        for name in &project.binaries {
            let binary = install.join(policy::binary_filename(name, target));
            if !binary.is_file() {
                return Err(Error(format!(
                    "cargo-binstall did not install {name:?} for {target}."
                )));
            }
        }
        println!("cargo-binstall installation verified: {target}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build::TargetOutputs, config::PackageFormat};

    fn outputs(target: &str, binary: std::path::PathBuf) -> Vec<TargetOutputs> {
        vec![TargetOutputs {
            target: target.into(),
            binaries: vec![("demo".into(), binary)],
        }]
    }
    #[test]
    #[ignore = "requires RUST_RELEASE_LIVE_* variables and a disposable Gitea release"]
    fn live_release_downloads_match_local_artifacts() {
        let required = |name: &str| std::env::var(name).expect("set live release test variables");
        let repository = required("RUST_RELEASE_LIVE_REPOSITORY");
        let tag = required("RUST_RELEASE_LIVE_TAG");
        let name = required("RUST_RELEASE_LIVE_NAME");
        let version = required("RUST_RELEASE_LIVE_VERSION");
        let directory = std::path::PathBuf::from(required("RUST_RELEASE_LIVE_ARTIFACTS"));
        let targets = [
            "x86_64-unknown-linux-musl",
            "aarch64-unknown-linux-musl",
            "x86_64-pc-windows-gnu",
        ];
        let artifacts = targets
            .iter()
            .map(|target| {
                let path = directory.join(policy::archive_name(&name, target, &version));
                Ok(Artifact {
                    target: (*target).into(),
                    sha256: package::checksum(&path)?,
                    path,
                    contents: ArtifactContents::Archive {
                        root: policy::archive_root(&name, target, &version),
                        binaries: vec![policy::binary_filename(&name, target)],
                    },
                })
            })
            .collect::<Result<Vec<_>, Error>>()
            .unwrap();
        let private = std::env::var("RUST_RELEASE_LIVE_PRIVATE").as_deref() == Ok("1");
        let api = private.then(|| crate::gitea::Gitea::new(&repository, "GITEA_TOKEN").unwrap());
        verify_published(
            &repository,
            &tag,
            &artifacts,
            &directory.join("SHA256SUMS"),
            api.as_ref().map(crate::gitea::Gitea::download_client),
        )
        .unwrap();
    }

    #[test]
    #[ignore = "requires a public disposable release and cargo-binstall"]
    fn live_binstall_installs_public_release() {
        let required = |name: &str| std::env::var(name).expect("set live release test variables");
        let manifest = std::path::PathBuf::from(required("RUST_RELEASE_LIVE_MANIFEST"));
        let root = manifest.parent().unwrap().to_path_buf();
        let name = required("RUST_RELEASE_LIVE_NAME");
        let project = SelectedPackage {
            workspace_root: std::path::PathBuf::new(),
            name: name.clone(),
            version: required("RUST_RELEASE_LIVE_VERSION"),
            binaries: vec![name],
            manifest,
            repository: Some(required("RUST_RELEASE_LIVE_REPOSITORY")),
            target_directory: root.join("target"),
        };
        verify_binstall(
            &root,
            &project,
            &[
                "x86_64-unknown-linux-musl".into(),
                "aarch64-unknown-linux-musl".into(),
                "x86_64-pc-windows-gnu".into(),
            ],
            &Runner::new("GITEA_TOKEN"),
        )
        .unwrap();
    }

    #[test]
    fn anonymous_downloads_match_local_archives_and_checksums() {
        use std::{net::TcpListener, thread};
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join("demo");
        fs::write(&binary, b"binary").unwrap();
        let (artifacts, sums) = package::package(
            temp.path(),
            "demo",
            "0.1.0",
            &outputs("x86_64-unknown-linux-musl", binary),
            PackageFormat::Tgz,
        )
        .unwrap();
        let filename = artifacts[0]
            .path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let bodies = [
            (filename, fs::read(&artifacts[0].path).unwrap()),
            ("SHA256SUMS".into(), fs::read(&sums).unwrap()),
        ];
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let repository = format!("http://{}/team/demo", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            for (filename, body) in bodies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = [0_u8; 4096];
                let count = stream.read(&mut buffer).unwrap();
                let request = String::from_utf8_lossy(&buffer[..count]);
                assert!(request.starts_with(&format!(
                    "GET /team/demo/releases/download/v0.1.0/{filename} "
                )));
                assert!(!request.to_ascii_lowercase().contains("authorization:"));
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        verify_published(&repository, "v0.1.0", &artifacts, &sums, None).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn authenticated_private_downloads_match_local_assets() {
        use std::{net::TcpListener, thread};
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join("demo");
        fs::write(&binary, b"binary").unwrap();
        let (artifacts, sums) = package::package(
            temp.path(),
            "demo",
            "0.1.0",
            &outputs("x86_64-unknown-linux-musl", binary),
            PackageFormat::Tgz,
        )
        .unwrap();
        let bodies = [
            fs::read(&artifacts[0].path).unwrap(),
            fs::read(&sums).unwrap(),
        ];
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let repository = format!("http://{}/team/demo", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            for body in bodies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = [0_u8; 4096];
                let count = stream.read(&mut buffer).unwrap();
                let request = String::from_utf8_lossy(&buffer[..count]);
                assert!(request
                    .lines()
                    .any(|line| line.eq_ignore_ascii_case("authorization: token test-token")));
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_static("token test-token"),
        );
        let client = Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        verify_published(&repository, "v0.1.0", &artifacts, &sums, Some(&client)).unwrap();
        handle.join().unwrap();
    }

    #[test]
    #[ignore = "requires cargo-binstall and a local HTTP listener"]
    fn real_binstall_installs_local_release_fixture() {
        use std::{
            collections::HashMap,
            net::TcpListener,
            sync::{
                atomic::{AtomicBool, Ordering},
                Arc,
            },
            thread,
        };
        let temp = tempfile::tempdir().unwrap();
        let targets = [
            "x86_64-unknown-linux-musl".to_owned(),
            "aarch64-unknown-linux-musl".to_owned(),
            "x86_64-pc-windows-gnu".to_owned(),
        ];
        let outputs = targets
            .iter()
            .map(|target| {
                let binary = temp.path().join(format!("binary-{target}"));
                fs::write(&binary, format!("fixture binary for {target}"))?;
                Ok(TargetOutputs {
                    target: target.clone(),
                    binaries: vec![("demo".into(), binary)],
                })
            })
            .collect::<Result<Vec<_>, std::io::Error>>()
            .unwrap();
        let (artifacts, _) =
            package::package(temp.path(), "demo", "0.1.0", &outputs, PackageFormat::Tgz).unwrap();
        let archives = artifacts
            .iter()
            .map(|artifact| {
                (
                    artifact
                        .path
                        .file_name()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned(),
                    fs::read(&artifact.path).unwrap(),
                )
            })
            .collect::<HashMap<_, _>>();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let repository = format!("http://{}/team/demo", listener.local_addr().unwrap());
        fs::create_dir(temp.path().join("src")).unwrap();
        fs::write(temp.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        let manifest = temp.path().join("Cargo.toml");
        fs::write(&manifest, format!("[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\nrepository='{repository}'\n[package.metadata.binstall]\npkg-url='{{ repo }}/releases/download/v{{ version }}/{{ name }}-{{ target }}-v{{ version }}.tgz'\npkg-fmt='tgz'\nbin-dir='{{ name }}-{{ target }}-v{{ version }}/{{ bin }}{{ binary-ext }}'\n")).unwrap();
        let done = Arc::new(AtomicBool::new(false));
        let server_done = Arc::clone(&done);
        let handle = thread::spawn(move || {
            while !server_done.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        let mut request = Vec::new();
                        let mut buffer = [0_u8; 4096];
                        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                            let count = stream.read(&mut buffer).unwrap();
                            if count == 0 {
                                break;
                            }
                            request.extend_from_slice(&buffer[..count]);
                        }
                        let request = String::from_utf8_lossy(&request);
                        let requested_archive = request
                            .lines()
                            .next()
                            .and_then(|line| line.split_whitespace().nth(1))
                            .and_then(|path| path.rsplit('/').next())
                            .and_then(|name| archives.get(name));
                        let body = requested_archive.map_or(&[][..], Vec::as_slice);
                        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                        stream.write_all(body).unwrap();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("local HTTP listener failed: {error}"),
                }
            }
        });
        let project = SelectedPackage {
            workspace_root: std::path::PathBuf::new(),
            name: "demo".into(),
            version: "0.1.0".into(),
            binaries: vec!["demo".into()],
            manifest,
            repository: Some(repository),
            target_directory: temp.path().join("target"),
        };
        let result = verify_binstall_with_options(
            temp.path(),
            &project,
            &targets,
            &Runner::new("GITEA_TOKEN"),
            true,
        );
        done.store(true, Ordering::Relaxed);
        handle.join().unwrap();
        result.unwrap();
    }

    #[test]
    fn local_archive_validation_and_binstall_command() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join("demo");
        fs::write(&binary, b"binary").unwrap();
        let (artifacts, _) = package::package(
            temp.path(),
            "demo",
            "0.1.0",
            &outputs("x86_64-unknown-linux-musl", binary),
            PackageFormat::Tgz,
        )
        .unwrap();
        verify_archive(&artifacts[0].path, &artifacts[0]).unwrap();
        let mut tar = Vec::new();
        GzDecoder::new(File::open(&artifacts[0].path).unwrap())
            .read_to_end(&mut tar)
            .unwrap();
        tar.extend_from_slice(&[1; 512]);
        let extra = temp.path().join("extra.tgz");
        let mut gzip = flate2::write::GzEncoder::new(
            File::create(&extra).unwrap(),
            flate2::Compression::default(),
        );
        gzip.write_all(&tar).unwrap();
        gzip.finish().unwrap();
        assert!(verify_archive(&extra, &artifacts[0])
            .unwrap_err()
            .to_string()
            .contains("unexpected members"));
        let project = SelectedPackage {
            workspace_root: std::path::PathBuf::new(),
            name: "demo".into(),
            version: "0.1.0".into(),
            binaries: vec!["demo".into()],
            manifest: temp.path().join("Cargo.toml"),
            repository: None,
            target_directory: temp.path().join("target"),
        };
        let args = binstall_arguments(&project, "x86_64-unknown-linux-musl", temp.path());
        assert!(args.contains(&"--no-track".to_owned()));
        assert!(args.contains(&"--no-discover-github-token".to_owned()));
        assert_eq!(
            asset_url("https://example.com/team/demo", "release+1", "one.tgz"),
            "https://example.com/team/demo/releases/download/release%2B1/one.tgz"
        );
    }

    #[test]
    fn archive_validation_rejects_corrupt_metadata_and_payloads() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join("demo");
        fs::write(&binary, b"binary").unwrap();
        let (artifacts, _) = package::package(
            temp.path(),
            "demo",
            "0.1.0",
            &outputs("x86_64-unknown-linux-musl", binary),
            PackageFormat::Tgz,
        )
        .unwrap();
        let artifact = &artifacts[0];
        let mut not_archive = artifact.clone();
        not_archive.contents = ArtifactContents::Binary;
        assert!(verify_archive(&artifact.path, &not_archive)
            .unwrap_err()
            .to_string()
            .contains("is not an archive"));

        let invalid_gzip = temp.path().join("invalid.tgz");
        fs::write(&invalid_gzip, b"not gzip").unwrap();
        assert!(verify_archive(&invalid_gzip, artifact)
            .unwrap_err()
            .to_string()
            .contains("invalid gzip"));

        let mut valid_tar = Vec::new();
        GzDecoder::new(File::open(&artifact.path).unwrap())
            .read_to_end(&mut valid_tar)
            .unwrap();
        let write_tar = |name: &str, bytes: &[u8]| {
            let path = temp.path().join(name);
            let mut gzip = flate2::write::GzEncoder::new(
                File::create(&path).unwrap(),
                flate2::Compression::default(),
            );
            gzip.write_all(bytes).unwrap();
            gzip.finish().unwrap();
            path
        };
        let truncated = write_tar("truncated.tgz", &[0; 1024]);
        assert!(verify_archive(&truncated, artifact)
            .unwrap_err()
            .to_string()
            .contains("truncated tar"));

        for (name, index, byte, expected) in [
            ("root.tgz", 0, b'x', "archive root"),
            ("binary.tgz", 512, b'x', "archive binary"),
            ("utf8.tgz", 0, 0xff, "non-UTF-8"),
            ("numeric.tgz", 512 + 100, b'x', "numeric field"),
        ] {
            let mut corrupt = valid_tar.clone();
            corrupt[index] = byte;
            let path = write_tar(name, &corrupt);
            let error = verify_archive(&path, artifact).unwrap_err().to_string();
            assert!(error.contains(expected), "{error}");
        }

        let mut non_executable = valid_tar;
        non_executable[512 + 100..512 + 108].copy_from_slice(b"0000644\0");
        let path = write_tar("mode.tgz", &non_executable);
        assert!(verify_archive(&path, artifact)
            .unwrap_err()
            .to_string()
            .contains("not executable"));
    }

    #[test]
    fn published_verification_rejects_http_size_and_checksum_failures() {
        use std::{net::TcpListener, thread};

        let temp = tempfile::tempdir().unwrap();
        let local = temp.path().join("demo.bin");
        let sums = temp.path().join("SHA256SUMS");
        fs::write(&local, b"local").unwrap();
        fs::write(&sums, b"unused").unwrap();
        let artifact = Artifact {
            target: "x86_64-unknown-linux-musl".into(),
            path: local,
            contents: ArtifactContents::Binary,
            sha256: String::new(),
        };

        for (status, body, expected) in [
            (404, b"missing".as_slice(), "HTTP 404"),
            (200, b"too-long".as_slice(), "exceeds expected size"),
            (200, b"wrong".as_slice(), "SHA-256 differs"),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let repository = format!("http://{}/team/demo", listener.local_addr().unwrap());
            let response_body = body.to_vec();
            let handle = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 {status} Result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response_body.len()
                )
                .unwrap();
                stream.write_all(&response_body).unwrap();
            });
            let error = verify_published(
                &repository,
                "v0.1.0",
                std::slice::from_ref(&artifact),
                &sums,
                None,
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains(expected), "{error}");
            handle.join().unwrap();
        }
    }
}
