use crate::error::Error;
use reqwest::{
    blocking::{
        multipart::{Form, Part},
        Client,
    },
    header::{HeaderMap, HeaderValue, AUTHORIZATION},
    Method, StatusCode,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{env, fs::File, path::Path, time::Duration};

#[derive(Debug, Deserialize)]
pub struct RemoteRelease {
    pub id: u64,
    #[serde(default)]
    pub draft: bool,
}

#[derive(Debug, Deserialize)]
struct Asset {
    id: u64,
    name: String,
}

pub struct Gitea {
    client: Client,
    base: String,
}

impl Gitea {
    pub fn new(repository: &str, token_env: &str) -> Result<Self, Error> {
        if !repository.starts_with("https://") {
            return Err(Error(
                "Authenticated Gitea publishing requires HTTPS.".into(),
            ));
        }
        let token = env::var(token_env)
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error(format!("Set {token_env} before publishing.")))?;
        Self::with_token(repository, &token)
    }

    pub(crate) fn with_token(repository: &str, token: &str) -> Result<Self, Error> {
        let parsed = url::Url::parse(repository)
            .map_err(|_| Error("Invalid release repository URL.".into()))?;
        let parts: Vec<_> = parsed
            .path_segments()
            .ok_or_else(|| Error("Repository URL must end in owner/repository.".into()))?
            .filter(|part| !part.is_empty())
            .collect();
        if parts.len() < 2 {
            return Err(Error("Repository URL must end in owner/repository.".into()));
        }
        let prefix = parts[..parts.len() - 2].join("/");
        let owner = parts[parts.len() - 2];
        let name = parts[parts.len() - 1];
        let origin = parsed.origin().ascii_serialization();
        let base = if prefix.is_empty() {
            format!("{origin}/api/v1/repos/{owner}/{name}")
        } else {
            format!("{origin}/{prefix}/api/v1/repos/{owner}/{name}")
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("token {token}"))
                .map_err(|_| Error("Invalid Gitea token value.".into()))?,
        );
        let client = Client::builder()
            .default_headers(headers)
            .user_agent("rust_realease/0.1")
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| Error("Cannot initialize Gitea HTTP client.".into()))?;
        Ok(Self { client, base })
    }

    fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        missing_ok: bool,
    ) -> Result<Option<Value>, Error> {
        let url = format!("{}{path}", self.base);
        let mut request = self.client.request(method.clone(), &url);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .map_err(|_| Error(format!("Gitea {method} {path}: connection failed.")))?;
        if missing_ok && response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(Error(format!(
                "Gitea {method} {path}: HTTP {}.",
                response.status().as_u16()
            )));
        }
        let bytes = response
            .bytes()
            .map_err(|_| Error(format!("Gitea {method} {path}: response failed.")))?;
        if bytes.is_empty() {
            return Ok(Some(Value::Null));
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| Error(format!("Gitea {method} {path}: invalid JSON response.")))
    }

    pub fn ensure_release(
        &self,
        tag: &str,
        commit: &str,
        draft: bool,
        prerelease: bool,
        message: Option<&str>,
    ) -> Result<RemoteRelease, Error> {
        let encoded = encode(tag);
        let remote = self
            .request(Method::GET, &format!("/tags/{encoded}"), None, true)?
            .ok_or_else(|| {
                Error(format!(
                    "Remote tag {tag:?} is missing. Push the tag explicitly before publishing."
                ))
            })?;
        if remote.pointer("/commit/sha").and_then(Value::as_str) != Some(commit) {
            return Err(Error(
                "Remote tag commit differs from the checkout; refusing publication.".into(),
            ));
        }
        let (release, existed) = if let Some(existing) = self.request(
            Method::GET,
            &format!("/releases/tags/{encoded}"),
            None,
            true,
        )? {
            (existing, true)
        } else {
            (
                self.request(Method::POST, "/releases", Some(json!({"tag_name": tag, "name": tag, "target_commitish": commit, "draft": draft, "prerelease": prerelease, "body": message.unwrap_or_default()})), false)?
                    .ok_or_else(|| Error("Gitea release creation returned no response.".into()))?,
                false,
            )
        };
        let release: RemoteRelease = serde_json::from_value(release)
            .map_err(|_| Error("Gitea release response is missing id or draft state.".into()))?;
        if existed {
            if let Some(message) = message {
                self.request(
                    Method::PATCH,
                    &format!("/releases/{}", release.id),
                    Some(json!({"body": message})),
                    false,
                )?;
            }
        }
        Ok(release)
    }

    pub fn repository_private(&self) -> Result<bool, Error> {
        self.request(Method::GET, "", None, false)?
            .and_then(|repository| repository.get("private").and_then(Value::as_bool))
            .ok_or_else(|| Error("Gitea repository response is missing visibility.".into()))
    }

    pub fn download_client(&self) -> &Client {
        &self.client
    }

    fn assets(&self, release_id: u64) -> Result<Vec<Asset>, Error> {
        let response = self
            .request(
                Method::GET,
                &format!("/releases/{release_id}/assets"),
                None,
                false,
            )?
            .ok_or_else(|| Error("Gitea assets response is empty.".into()))?;
        serde_json::from_value(response)
            .map_err(|_| Error("Gitea assets response is invalid.".into()))
    }

    pub fn upload(&self, release_id: u64, paths: &[&Path], replace: bool) -> Result<(), Error> {
        let existing = self.assets(release_id)?;
        let names: Vec<_> = paths
            .iter()
            .map(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| Error("Invalid upload filename.".into()))
            })
            .collect::<Result<_, _>>()?;
        let collisions: Vec<_> = existing
            .iter()
            .filter(|asset| names.contains(&asset.name.as_str()))
            .collect();
        if !collisions.is_empty() && !replace {
            return Err(Error(format!("Release assets already exist: {}. Use --replace-assets to replace them explicitly.", collisions.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", "))));
        }
        for (path, name) in paths.iter().zip(names.iter()) {
            for asset in collisions.iter().filter(|asset| asset.name == *name) {
                self.request(
                    Method::DELETE,
                    &format!("/releases/{release_id}/assets/{}", asset.id),
                    None,
                    false,
                )?;
            }
            let input = File::open(path)
                .map_err(|e| Error(format!("cannot open asset {}: {e}", path.display())))?;
            let size = input
                .metadata()
                .map_err(|e| Error(format!("cannot inspect asset {}: {e}", path.display())))?
                .len();
            let part = Part::reader_with_length(input, size)
                .file_name((*name).to_owned())
                .mime_str("application/octet-stream")
                .map_err(|_| Error("Invalid upload MIME type.".into()))?;
            let mut url = url::Url::parse(&format!("{}/releases/{release_id}/assets", self.base))
                .map_err(|_| Error("Invalid Gitea upload URL.".into()))?;
            url.query_pairs_mut().append_pair("name", name);
            let response = self
                .client
                .post(url)
                .multipart(Form::new().part("attachment", part))
                .send()
                .map_err(|_| {
                    Error(format!(
                        "Gitea POST /releases/{release_id}/assets: connection failed."
                    ))
                })?;
            if !response.status().is_success() {
                return Err(Error(format!(
                    "Gitea POST /releases/{release_id}/assets: HTTP {}.",
                    response.status().as_u16()
                )));
            }
            println!("Uploaded {name}");
        }
        Ok(())
    }
}

fn encode(segment: &str) -> String {
    url::form_urlencoded::byte_serialize(segment.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };

    struct Reply {
        method: &'static str,
        path: &'static str,
        status: u16,
        body: &'static str,
    }

    fn server(replies: Vec<Reply>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for reply in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 8192];
                let header_end = loop {
                    let n = stream.read(&mut buffer).unwrap();
                    assert!(n > 0, "client closed before headers");
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(index) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        break index + 4;
                    }
                    assert!(bytes.len() < 64 * 1024);
                };
                let headers = String::from_utf8_lossy(&bytes[..header_end]).to_string();
                assert!(
                    headers
                        .lines()
                        .any(|line| line.eq_ignore_ascii_case("authorization: token test-token")),
                    "authenticated Gitea request lacked the configured token"
                );
                let first = headers.lines().next().unwrap().to_owned();
                assert!(
                    first.starts_with(&format!("{} {} ", reply.method, reply.path)),
                    "{first}"
                );
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                while bytes.len() < header_end + length {
                    let n = stream.read(&mut buffer).unwrap();
                    assert!(n > 0, "client closed before body");
                    bytes.extend_from_slice(&buffer[..n]);
                }
                requests.push(format!(
                    "{first}\n{}",
                    String::from_utf8_lossy(&bytes[header_end..header_end + length])
                ));
                let reason = if reply.status == 200 {
                    "OK"
                } else if reply.status == 201 {
                    "Created"
                } else if reply.status == 204 {
                    "No Content"
                } else if reply.status == 404 {
                    "Not Found"
                } else {
                    "Unauthorized"
                };
                write!(stream, "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", reply.status, reason, reply.body.len(), reply.body).unwrap();
            }
            requests
        });
        (address, handle)
    }

    #[test]
    fn creates_or_reuses_release_after_remote_tag_check() {
        let base = "/gitea/api/v1/repos/team/demo";
        let (host, handle) = server(vec![
            Reply {
                method: "GET",
                path: "/gitea/api/v1/repos/team/demo/tags/v0.1.0",
                status: 200,
                body: r#"{"commit":{"sha":"abc"}}"#,
            },
            Reply {
                method: "GET",
                path: "/gitea/api/v1/repos/team/demo/releases/tags/v0.1.0",
                status: 404,
                body: "{}",
            },
            Reply {
                method: "POST",
                path: "/gitea/api/v1/repos/team/demo/releases",
                status: 201,
                body: r#"{"id":7,"draft":false}"#,
            },
        ]);
        let api = Gitea::with_token(&format!("{host}/gitea/team/demo"), "test-token").unwrap();
        let release = api
            .ensure_release("v0.1.0", "abc", false, true, Some("Release notes"))
            .unwrap();
        assert_eq!(release.id, 7);
        let requests = handle.join().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[2].contains(r#""body":"Release notes""#));

        let (host, handle) = server(vec![
            Reply {
                method: "GET",
                path: "/gitea/api/v1/repos/team/demo/tags/v0.1.0",
                status: 200,
                body: r#"{"commit":{"sha":"abc"}}"#,
            },
            Reply {
                method: "GET",
                path: "/gitea/api/v1/repos/team/demo/releases/tags/v0.1.0",
                status: 200,
                body: r#"{"id":9,"draft":true}"#,
            },
            Reply {
                method: "PATCH",
                path: "/gitea/api/v1/repos/team/demo/releases/9",
                status: 200,
                body: r#"{"id":9,"draft":true}"#,
            },
        ]);
        let api = Gitea::with_token(&format!("{host}/gitea/team/demo"), "test-token").unwrap();
        assert_eq!(
            api.ensure_release("v0.1.0", "abc", false, false, Some("Updated notes"))
                .unwrap()
                .id,
            9
        );
        let requests = handle.join().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[2].contains(r#""body":"Updated notes""#));
        let _ = base;
    }

    #[test]
    fn collisions_refuse_or_replace_only_matching_asset() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("one.tgz");
        std::fs::write(&path, b"archive").unwrap();
        let assets = r#"[{"id":3,"name":"one.tgz"},{"id":4,"name":"unrelated.tgz"}]"#;
        let (host, handle) = server(vec![Reply {
            method: "GET",
            path: "/api/v1/repos/team/demo/releases/7/assets",
            status: 200,
            body: assets,
        }]);
        let api = Gitea::with_token(&format!("{host}/team/demo"), "test-token").unwrap();
        assert!(api
            .upload(7, &[&path], false)
            .unwrap_err()
            .to_string()
            .contains("--replace-assets"));
        assert_eq!(handle.join().unwrap().len(), 1);
        let (host, handle) = server(vec![
            Reply {
                method: "GET",
                path: "/api/v1/repos/team/demo/releases/7/assets",
                status: 200,
                body: assets,
            },
            Reply {
                method: "DELETE",
                path: "/api/v1/repos/team/demo/releases/7/assets/3",
                status: 204,
                body: "",
            },
            Reply {
                method: "POST",
                path: "/api/v1/repos/team/demo/releases/7/assets?name=one.tgz",
                status: 201,
                body: r#"{"id":5}"#,
            },
        ]);
        let api = Gitea::with_token(&format!("{host}/team/demo"), "test-token").unwrap();
        api.upload(7, &[&path], true).unwrap();
        assert_eq!(handle.join().unwrap().len(), 3);
    }

    #[test]
    fn api_errors_do_not_echo_server_body_or_token() {
        let (host, handle) = server(vec![Reply {
            method: "GET",
            path: "/api/v1/repos/team/demo/tags/v0.1.0",
            status: 401,
            body: "test-token",
        }]);
        let api = Gitea::with_token(&format!("{host}/team/demo"), "test-token").unwrap();
        let error = api
            .ensure_release("v0.1.0", "abc", false, false, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("HTTP 401"));
        assert!(!error.contains("test-token"));
        handle.join().unwrap();
    }

    #[test]
    fn missing_or_mismatched_remote_tag_stops_before_release_mutation() {
        for (status, body, expected) in [
            (404, "{}", "missing"),
            (200, r#"{"commit":{"sha":"other"}}"#, "differs"),
        ] {
            let (host, handle) = server(vec![Reply {
                method: "GET",
                path: "/api/v1/repos/team/demo/tags/v0.1.0",
                status,
                body,
            }]);
            let api = Gitea::with_token(&format!("{host}/team/demo"), "test-token").unwrap();
            let error = api
                .ensure_release("v0.1.0", "abc", false, false, None)
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "{error}");
            assert_eq!(handle.join().unwrap().len(), 1);
        }
    }

    #[test]
    fn reads_repository_visibility_before_private_verification() {
        let (host, handle) = server(vec![Reply {
            method: "GET",
            path: "/api/v1/repos/team/demo",
            status: 200,
            body: r#"{"private":true}"#,
        }]);
        let api = Gitea::with_token(&format!("{host}/team/demo"), "test-token").unwrap();
        assert!(api.repository_private().unwrap());
        assert_eq!(handle.join().unwrap().len(), 1);
    }

    #[test]
    fn encodes_tag_and_requires_https() {
        assert_eq!(encode("v1.0+build"), "v1.0%2Bbuild");
        assert!(Gitea::new("http://example.com/team/demo", "GITEA_TOKEN")
            .err()
            .unwrap()
            .to_string()
            .contains("HTTPS"));
    }

    #[test]
    fn constructor_rejects_missing_credentials_and_malformed_repositories() {
        let missing = "RUST_REALEASE_TEST_TOKEN_THAT_IS_NOT_SET";
        assert!(Gitea::new("https://example.com/team/demo", missing)
            .err()
            .unwrap()
            .to_string()
            .contains(missing));
        for repository in [
            "://",
            "mailto:demo@example.com",
            "https://example.com/only-one",
        ] {
            assert!(
                Gitea::with_token(repository, "token").is_err(),
                "{repository}"
            );
        }
        assert!(
            Gitea::with_token("https://example.com/team/demo", "bad\ntoken")
                .err()
                .unwrap()
                .to_string()
                .contains("token value")
        );
    }

    #[test]
    fn malformed_api_responses_and_upload_failures_are_reported() {
        let (host, handle) = server(vec![Reply {
            method: "GET",
            path: "/api/v1/repos/team/demo/tags/v0.1.0",
            status: 200,
            body: "not-json",
        }]);
        let api = Gitea::with_token(&format!("{host}/team/demo"), "test-token").unwrap();
        assert!(api
            .ensure_release("v0.1.0", "abc", false, false, None)
            .unwrap_err()
            .to_string()
            .contains("invalid JSON"));
        handle.join().unwrap();

        let (host, handle) = server(vec![Reply {
            method: "GET",
            path: "/api/v1/repos/team/demo",
            status: 200,
            body: "{}",
        }]);
        let api = Gitea::with_token(&format!("{host}/team/demo"), "test-token").unwrap();
        assert!(api.repository_private().is_err());
        handle.join().unwrap();

        let (host, handle) = server(vec![Reply {
            method: "GET",
            path: "/api/v1/repos/team/demo/releases/7/assets",
            status: 200,
            body: "{}",
        }]);
        let api = Gitea::with_token(&format!("{host}/team/demo"), "test-token").unwrap();
        assert!(api.upload(7, &[Path::new("missing")], false).is_err());
        handle.join().unwrap();

        let (host, handle) = server(vec![Reply {
            method: "GET",
            path: "/api/v1/repos/team/demo/releases/7/assets",
            status: 200,
            body: "[]",
        }]);
        let api = Gitea::with_token(&format!("{host}/team/demo"), "test-token").unwrap();
        assert!(api.upload(7, &[Path::new("missing")], false).is_err());
        handle.join().unwrap();

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("asset.bin");
        std::fs::write(&path, b"asset").unwrap();
        let (host, handle) = server(vec![
            Reply {
                method: "GET",
                path: "/api/v1/repos/team/demo/releases/7/assets",
                status: 200,
                body: "[]",
            },
            Reply {
                method: "POST",
                path: "/api/v1/repos/team/demo/releases/7/assets?name=asset.bin",
                status: 401,
                body: "{}",
            },
        ]);
        let api = Gitea::with_token(&format!("{host}/team/demo"), "test-token").unwrap();
        assert!(api
            .upload(7, &[&path], false)
            .unwrap_err()
            .to_string()
            .contains("HTTP 401"));
        handle.join().unwrap();
    }
}
