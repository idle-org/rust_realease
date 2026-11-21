use crate::{error::Error, gitea::RemoteRelease};
use reqwest::{
    blocking::{Body, Client},
    header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE},
    Method, StatusCode,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs::File, path::Path, time::Duration};

#[derive(Debug, Deserialize)]
struct Asset {
    id: u64,
    name: String,
}

pub struct Github {
    client: Client,
    base: String,
    uploads: String,
}

impl Github {
    pub fn with_token(repository: &str, token: &str) -> Result<Self, Error> {
        Self::with_origins(
            repository,
            token,
            "https://api.github.com",
            "https://uploads.github.com",
        )
    }

    pub(crate) fn with_origins(
        repository: &str,
        token: &str,
        api_origin: &str,
        upload_origin: &str,
    ) -> Result<Self, Error> {
        let parsed = url::Url::parse(repository)
            .map_err(|_| Error("Invalid GitHub repository URL.".into()))?;
        if parsed.scheme() != "https" || parsed.host_str() != Some("github.com") {
            return Err(Error(
                "GitHub publishing requires an HTTPS github.com repository URL.".into(),
            ));
        }
        let parts: Vec<_> = parsed
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|part| !part.is_empty())
            .collect();
        if parts.len() != 2 {
            return Err(Error(
                "GitHub repository URL must end in owner/repository.".into(),
            ));
        }
        let repository_path = format!("{}/{}", parts[0], parts[1]);
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|_| Error("Invalid GitHub token value.".into()))?,
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "x-github-api-version",
            HeaderValue::from_static("2022-11-28"),
        );
        let client = Client::builder()
            .default_headers(headers)
            .user_agent("rust_realease/0.1")
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|_| Error("Cannot initialize GitHub HTTP client.".into()))?;
        Ok(Self {
            client,
            base: format!("{api_origin}/repos/{repository_path}"),
            uploads: format!("{upload_origin}/repos/{repository_path}"),
        })
    }

    fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        missing_ok: bool,
    ) -> Result<Option<Value>, Error> {
        let mut request = self
            .client
            .request(method.clone(), format!("{}{path}", self.base));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .map_err(|_| Error(format!("GitHub {method} {path}: connection failed.")))?;
        if missing_ok && response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(Error(format!(
                "GitHub {method} {path}: HTTP {}.",
                response.status().as_u16()
            )));
        }
        let bytes = response
            .bytes()
            .map_err(|_| Error(format!("GitHub {method} {path}: response failed.")))?;
        if bytes.is_empty() {
            return Ok(Some(Value::Null));
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| Error(format!("GitHub {method} {path}: invalid JSON response.")))
    }

    pub fn ensure_release(
        &self,
        tag: &str,
        commit: &str,
        draft: bool,
        prerelease: bool,
        message: Option<&str>,
        replace_existing_message: bool,
    ) -> Result<RemoteRelease, Error> {
        let encoded = encode(tag);
        let reference = self
            .request(Method::GET, &format!("/git/ref/tags/{encoded}"), None, true)?
            .ok_or_else(|| {
                Error(format!(
                    "Remote tag {tag:?} is missing. Push the tag explicitly before publishing."
                ))
            })?;
        let mut remote_commit = reference
            .pointer("/object/sha")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if reference.pointer("/object/type").and_then(Value::as_str) == Some("tag") {
            let sha = remote_commit
                .as_deref()
                .ok_or_else(|| Error("GitHub tag response is missing its object SHA.".into()))?;
            let tag_object = self
                .request(Method::GET, &format!("/git/tags/{sha}"), None, false)?
                .ok_or_else(|| Error("GitHub annotated tag response is empty.".into()))?;
            remote_commit = tag_object
                .pointer("/object/sha")
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
        if remote_commit.as_deref() != Some(commit) {
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
                    .ok_or_else(|| Error("GitHub release creation returned no response.".into()))?,
                false,
            )
        };
        let release: RemoteRelease = serde_json::from_value(release)
            .map_err(|_| Error("GitHub release response is missing id or draft state.".into()))?;
        if existed && replace_existing_message {
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
            .ok_or_else(|| Error("GitHub repository response is missing visibility.".into()))
    }

    pub fn download_client(&self) -> &Client {
        &self.client
    }

    fn assets(&self, release_id: u64) -> Result<Vec<Asset>, Error> {
        let response = self
            .request(
                Method::GET,
                &format!("/releases/{release_id}/assets?per_page=100"),
                None,
                false,
            )?
            .ok_or_else(|| Error("GitHub assets response is empty.".into()))?;
        serde_json::from_value(response)
            .map_err(|_| Error("GitHub assets response is invalid.".into()))
    }

    pub fn asset_urls(&self, release_id: u64) -> Result<BTreeMap<String, String>, Error> {
        Ok(self
            .assets(release_id)?
            .into_iter()
            .map(|asset| {
                (
                    asset.name,
                    format!("{}/releases/assets/{}", self.base, asset.id),
                )
            })
            .collect())
    }

    pub fn upload(&self, release_id: u64, paths: &[&Path], replace: bool) -> Result<(), Error> {
        let existing = self.assets(release_id)?;
        for path in paths {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| Error("Invalid upload filename.".into()))?;
            let collisions: Vec<_> = existing.iter().filter(|asset| asset.name == name).collect();
            if !collisions.is_empty() && !replace {
                return Err(Error(format!(
                    "Release asset already exists: {name}. Use --replace-assets to replace it explicitly."
                )));
            }
            for asset in collisions {
                self.request(
                    Method::DELETE,
                    &format!("/releases/assets/{}", asset.id),
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
            let mut url =
                url::Url::parse(&format!("{}/releases/{release_id}/assets", self.uploads))
                    .map_err(|_| Error("Invalid GitHub upload URL.".into()))?;
            url.query_pairs_mut().append_pair("name", name);
            let response = self
                .client
                .post(url)
                .header(CONTENT_TYPE, "application/octet-stream")
                .header(CONTENT_LENGTH, size)
                .body(Body::new(input))
                .send()
                .map_err(|_| {
                    Error(format!(
                        "GitHub POST /releases/{release_id}/assets: connection failed."
                    ))
                })?;
            if !response.status().is_success() {
                return Err(Error(format!(
                    "GitHub POST /releases/{release_id}/assets: HTTP {}.",
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
    };

    struct Reply {
        method: &'static str,
        path: &'static str,
        status: u16,
        body: &'static str,
    }

    fn server(replies: Vec<Reply>) -> (String, thread::JoinHandle<Vec<Vec<u8>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let mut bodies = Vec::new();
            for reply in replies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 8192];
                let header_end = loop {
                    let count = stream.read(&mut buffer).unwrap();
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        break index + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let request_line = headers.lines().next().unwrap();
                assert_eq!(
                    request_line,
                    format!("{} {} HTTP/1.1", reply.method, reply.path)
                );
                assert!(headers
                    .lines()
                    .any(|line| line.eq_ignore_ascii_case("authorization: Bearer test-token")));
                assert!(headers.lines().any(|line| {
                    line.eq_ignore_ascii_case("accept: application/vnd.github+json")
                }));
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(str::to_owned)
                    })
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                while bytes.len() < header_end + length {
                    let count = stream.read(&mut buffer).unwrap();
                    bytes.extend_from_slice(&buffer[..count]);
                }
                bodies.push(bytes[header_end..header_end + length].to_vec());
                write!(
                    stream,
                    "HTTP/1.1 {} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    reply.status,
                    reply.body.len(),
                    reply.body
                )
                .unwrap();
            }
            bodies
        });
        (address, handle)
    }

    #[test]
    fn creates_release_and_uploads_raw_github_asset() {
        let (host, handle) = server(vec![
            Reply {
                method: "GET",
                path: "/repos/team/demo/git/ref/tags/v1.0.0",
                status: 200,
                body: r#"{"object":{"type":"commit","sha":"abc"}}"#,
            },
            Reply {
                method: "GET",
                path: "/repos/team/demo/releases/tags/v1.0.0",
                status: 404,
                body: "{}",
            },
            Reply {
                method: "POST",
                path: "/repos/team/demo/releases",
                status: 201,
                body: r#"{"id":7,"draft":false}"#,
            },
            Reply {
                method: "GET",
                path: "/repos/team/demo/releases/7/assets?per_page=100",
                status: 200,
                body: "[]",
            },
            Reply {
                method: "POST",
                path: "/repos/team/demo/releases/7/assets?name=asset.bin",
                status: 201,
                body: "{}",
            },
            Reply {
                method: "GET",
                path: "/repos/team/demo/releases/7/assets?per_page=100",
                status: 200,
                body: r#"[{"id":11,"name":"asset.bin"}]"#,
            },
        ]);
        let api = Github::with_origins("https://github.com/team/demo", "test-token", &host, &host)
            .unwrap();
        let release = api
            .ensure_release("v1.0.0", "abc", false, false, Some("notes"), false)
            .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let asset = temp.path().join("asset.bin");
        std::fs::write(&asset, b"asset bytes").unwrap();
        api.upload(release.id, &[asset.as_path()], false).unwrap();
        assert_eq!(
            api.asset_urls(release.id)
                .unwrap()
                .get("asset.bin")
                .map(String::as_str),
            Some(format!("{host}/repos/team/demo/releases/assets/11").as_str())
        );
        let bodies = handle.join().unwrap();
        assert!(String::from_utf8_lossy(&bodies[2]).contains("\"tag_name\":\"v1.0.0\""));
        assert_eq!(bodies[4], b"asset bytes");
    }

    #[test]
    fn rejects_non_github_and_malformed_repository_urls() {
        for repository in [
            "http://github.com/team/demo",
            "https://example.com/team/demo",
            "https://github.com/team/demo/extra",
        ] {
            assert!(Github::with_token(repository, "token").is_err());
        }
    }

    #[test]
    fn updates_existing_annotated_release_and_replaces_colliding_asset() {
        let (host, handle) = server(vec![
            Reply {
                method: "GET",
                path: "/repos/team/demo/git/ref/tags/v1.0.0",
                status: 200,
                body: r#"{"object":{"type":"tag","sha":"tag-object"}}"#,
            },
            Reply {
                method: "GET",
                path: "/repos/team/demo/git/tags/tag-object",
                status: 200,
                body: r#"{"object":{"sha":"abc"}}"#,
            },
            Reply {
                method: "GET",
                path: "/repos/team/demo/releases/tags/v1.0.0",
                status: 200,
                body: r#"{"id":7,"draft":false}"#,
            },
            Reply {
                method: "PATCH",
                path: "/repos/team/demo/releases/7",
                status: 200,
                body: "",
            },
            Reply {
                method: "GET",
                path: "/repos/team/demo",
                status: 200,
                body: r#"{"private":true}"#,
            },
            Reply {
                method: "GET",
                path: "/repos/team/demo/releases/7/assets?per_page=100",
                status: 200,
                body: r#"[{"id":11,"name":"asset.bin"}]"#,
            },
            Reply {
                method: "DELETE",
                path: "/repos/team/demo/releases/assets/11",
                status: 204,
                body: "",
            },
            Reply {
                method: "POST",
                path: "/repos/team/demo/releases/7/assets?name=asset.bin",
                status: 201,
                body: "{}",
            },
        ]);
        let api = Github::with_origins("https://github.com/team/demo", "test-token", &host, &host)
            .unwrap();
        let release = api
            .ensure_release("v1.0.0", "abc", false, false, Some("new notes"), true)
            .unwrap();
        assert_eq!(release.id, 7);
        assert!(api.repository_private().unwrap());
        let _ = api.download_client();

        let temp = tempfile::tempdir().unwrap();
        let asset = temp.path().join("asset.bin");
        std::fs::write(&asset, b"replacement").unwrap();
        api.upload(release.id, &[asset.as_path()], true).unwrap();

        let bodies = handle.join().unwrap();
        assert!(String::from_utf8_lossy(&bodies[3]).contains("new notes"));
        assert_eq!(bodies[7], b"replacement");
    }

    #[test]
    fn reports_missing_mismatched_and_malformed_github_responses() {
        let cases = [
            (404, "{}", "missing"),
            (
                200,
                r#"{"object":{"type":"commit","sha":"other"}}"#,
                "differs",
            ),
            (200, "not json", "invalid JSON"),
            (500, "{}", "HTTP 500"),
        ];
        for (status, body, expected) in cases {
            let (host, handle) = server(vec![Reply {
                method: "GET",
                path: "/repos/team/demo/git/ref/tags/v1.0.0",
                status,
                body,
            }]);
            let api =
                Github::with_origins("https://github.com/team/demo", "test-token", &host, &host)
                    .unwrap();
            let error = api
                .ensure_release("v1.0.0", "abc", false, false, None, false)
                .unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
            handle.join().unwrap();
        }
    }

    #[test]
    fn reports_malformed_release_repository_and_asset_responses() {
        let (host, handle) = server(vec![Reply {
            method: "GET",
            path: "/repos/team/demo/git/ref/tags/v1.0.0",
            status: 200,
            body: r#"{"object":{"type":"tag"}}"#,
        }]);
        let api = Github::with_origins("https://github.com/team/demo", "test-token", &host, &host)
            .unwrap();
        assert!(api
            .ensure_release("v1.0.0", "abc", false, false, None, false)
            .unwrap_err()
            .to_string()
            .contains("object SHA"));
        handle.join().unwrap();

        let (host, handle) = server(vec![
            Reply {
                method: "GET",
                path: "/repos/team/demo/git/ref/tags/v1.0.0",
                status: 200,
                body: r#"{"object":{"type":"commit","sha":"abc"}}"#,
            },
            Reply {
                method: "GET",
                path: "/repos/team/demo/releases/tags/v1.0.0",
                status: 200,
                body: "{}",
            },
        ]);
        let api = Github::with_origins("https://github.com/team/demo", "test-token", &host, &host)
            .unwrap();
        assert!(api
            .ensure_release("v1.0.0", "abc", false, false, None, false)
            .unwrap_err()
            .to_string()
            .contains("missing id"));
        handle.join().unwrap();

        for (path, operation, expected) in [
            ("/repos/team/demo", "repository", "visibility"),
            (
                "/repos/team/demo/releases/7/assets?per_page=100",
                "assets",
                "invalid",
            ),
        ] {
            let (host, handle) = server(vec![Reply {
                method: "GET",
                path,
                status: 200,
                body: "{}",
            }]);
            let api =
                Github::with_origins("https://github.com/team/demo", "test-token", &host, &host)
                    .unwrap();
            let error = if operation == "repository" {
                api.repository_private().unwrap_err()
            } else {
                api.asset_urls(7).unwrap_err()
            };
            assert!(error.to_string().contains(expected), "{error}");
            handle.join().unwrap();
        }
    }

    #[test]
    fn refuses_asset_collision_and_reports_upload_http_failure() {
        let temp = tempfile::tempdir().unwrap();
        let asset = temp.path().join("asset.bin");
        std::fs::write(&asset, b"asset").unwrap();

        let (host, handle) = server(vec![Reply {
            method: "GET",
            path: "/repos/team/demo/releases/7/assets?per_page=100",
            status: 200,
            body: r#"[{"id":11,"name":"asset.bin"}]"#,
        }]);
        let api = Github::with_origins("https://github.com/team/demo", "test-token", &host, &host)
            .unwrap();
        assert!(api.upload(7, &[asset.as_path()], false).is_err());
        handle.join().unwrap();

        let (host, handle) = server(vec![
            Reply {
                method: "GET",
                path: "/repos/team/demo/releases/7/assets?per_page=100",
                status: 200,
                body: "[]",
            },
            Reply {
                method: "POST",
                path: "/repos/team/demo/releases/7/assets?name=asset.bin",
                status: 500,
                body: "{}",
            },
        ]);
        let api = Github::with_origins("https://github.com/team/demo", "test-token", &host, &host)
            .unwrap();
        let error = api.upload(7, &[asset.as_path()], false).unwrap_err();
        assert!(error.to_string().contains("HTTP 500"), "{error}");
        handle.join().unwrap();
    }
}
