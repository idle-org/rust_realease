use crate::error::Error;
use regex::Regex;
use std::{path::Path, process::Command, sync::OnceLock};

pub struct Runner {
    token_env: String,
}

impl Runner {
    pub fn new(token_env: &str) -> Self {
        Self {
            token_env: token_env.to_owned(),
        }
    }

    pub fn output(&self, program: &str, args: &[&str], cwd: &Path) -> Result<Vec<u8>, Error> {
        let output = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .env_remove("GITEA_TOKEN")
            .env_remove(&self.token_env)
            .output()
            .map_err(|e| Error(format!("cannot run {program}: {e}")))?;
        if !output.status.success() {
            let command = std::iter::once(program)
                .chain(args.iter().copied())
                .map(|arg| if arg.contains("://") { "[URL]" } else { arg })
                .collect::<Vec<_>>()
                .join(" ");
            let detail = self.redact(&String::from_utf8_lossy(&output.stderr));
            return Err(Error(format!(
                "command {} failed with {}: {}",
                self.redact(&command),
                output.status,
                detail.trim()
            )));
        }
        Ok(output.stdout)
    }

    pub fn text(&self, program: &str, args: &[&str], cwd: &Path) -> Result<String, Error> {
        let bytes = self.output(program, args, cwd)?;
        String::from_utf8(bytes)
            .map(|text| text.trim().to_owned())
            .map_err(|e| Error(format!("{program} returned non-UTF-8 output: {e}")))
    }

    pub fn redact(&self, text: &str) -> String {
        let token = std::env::var(&self.token_env)
            .ok()
            .filter(|value| !value.is_empty());
        let without_token = if let Some(secret) = token {
            text.replace(&secret, "[REDACTED]")
        } else {
            text.to_owned()
        };
        static URLS: OnceLock<Regex> = OnceLock::new();
        let pattern = URLS.get_or_init(|| {
            Regex::new(r#"(?i)(?:https?|ssh)://[^\s'"<>]+"#)
                .unwrap_or_else(|_| unreachable!("static URL regex must compile"))
        });
        pattern.replace_all(&without_token, "[URL]").into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_token_and_urls_and_keeps_token_out_of_child() {
        let runner = Runner::new("RUST_RELEASE_TEST_SECRET");
        let secret = "unit-test-secret-value";
        std::env::set_var("RUST_RELEASE_TEST_SECRET", secret);
        let message = runner
            .redact("failed https://user:password@example.com/repo and unit-test-secret-value");
        assert!(!message.contains(secret));
        assert!(!message.contains("password"));
        let child = runner.text("env", &[], Path::new(".")).unwrap();
        assert!(!child.contains("RUST_RELEASE_TEST_SECRET"));
        let error = runner
            .output("git", &["rev-parse", secret], Path::new("."))
            .unwrap_err()
            .to_string();
        assert!(!error.contains(secret));
        std::env::remove_var("RUST_RELEASE_TEST_SECRET");
    }
}
