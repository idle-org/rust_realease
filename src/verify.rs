use crate::{
    error::Error,
    package::{self, Artifact},
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
    if first != artifact.root || directory[156] != b'5' {
        return Err(Error(format!(
            "{}: archive root does not match binstall metadata.",
            path.display()
        )));
    }
    let binary = &tar[512..1024];
    let second = member_name(binary)?;
    if second != format!("{}/{}", artifact.root, artifact.binary) || binary[156] != b'0' {
        return Err(Error(format!(
            "{}: archive binary does not match binstall metadata.",
            path.display()
        )));
    }
    let mode = octal(&binary[100..108])?;
    if !artifact.target.split('-').any(|part| part == "windows") && mode & 0o111 == 0 {
        return Err(Error(format!(
            "{}: binary is not executable.",
            path.display()
        )));
    }
    let size = octal(&binary[124..136])?;
    let rounded = size
        .checked_add(511)
        .ok_or_else(|| Error("Archive size overflow.".into()))?
        / 512
        * 512;
    let end = 1024_u64
        .checked_add(rounded)
        .and_then(|n| n.checked_add(1024))
        .ok_or_else(|| Error("Archive size overflow.".into()))?;
    if end > tar.len() as u64 {
        return Err(Error(format!(
            "{}: truncated binary or tar footer.",
            path.display()
        )));
    }
    if tar[(end - 1024) as usize..].iter().any(|b| *b != 0) {
        return Err(Error(format!(
            "{}: archive contains unexpected members.",
            path.display()
        )));
    }
    Ok(())
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
) -> Result<(), Error> {
    let downloads = tempfile::Builder::new()
        .prefix("rust-realease-download-")
        .tempdir()
        .map_err(|e| Error(format!("cannot create download directory: {e}")))?;
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|_| Error("Cannot initialize anonymous download client.".into()))?;
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
            .map_err(|_| Error(format!("{filename}: public download failed.")))?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(Error(format!(
                "{filename}: public download returned HTTP {}.",
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
                .map_err(|_| Error(format!("{filename}: public download failed.")))?;
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
            verify_archive(&path, artifact)?;
            println!("Verified URL, checksum and archive: {}", artifact.target);
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
        let args = binstall_arguments(project, target, &install);
        let borrowed: Vec<_> = args.iter().map(String::as_str).collect();
        let cargo_home = cargo_home
            .to_str()
            .ok_or_else(|| Error("Cargo home path is not UTF-8.".into()))?;
        runner.output_with("cargo", &borrowed, root, &[("CARGO_HOME", cargo_home)], &[])?;
        let binary = install.join(policy::binary_filename(&project.binary, target));
        if !binary.is_file() {
            return Err(Error(format!(
                "cargo-binstall did not install the expected binary for {target}."
            )));
        }
        println!("cargo-binstall installation verified: {target}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
            "demo",
            &[("x86_64-unknown-linux-musl".into(), binary)],
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
        verify_published(&repository, "v0.1.0", &artifacts, &sums).unwrap();
        handle.join().unwrap();
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
            "demo",
            &[("x86_64-unknown-linux-musl".into(), binary)],
        )
        .unwrap();
        verify_archive(&artifacts[0].path, &artifacts[0]).unwrap();
        let project = SelectedPackage {
            name: "demo".into(),
            version: "0.1.0".into(),
            binary: "demo".into(),
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
}
