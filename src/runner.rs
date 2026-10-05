use crate::error::Error;
use regex::Regex;
use std::{path::Path, process::Command, sync::OnceLock};

pub struct Runner {
    token_env: String,
    secrets: Vec<String>,
    verbose: bool,
}

impl Runner {
    pub fn new(token_env: &str) -> Self {
        Self {
            token_env: token_env.to_owned(),
            secrets: Vec::new(),
            verbose: false,
        }
    }

    pub fn with_secret(mut self, secret: Option<&str>) -> Self {
        if let Some(secret) = secret.filter(|secret| !secret.is_empty()) {
            self.secrets.push(secret.to_owned());
        }
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    pub fn output(&self, program: &str, args: &[&str], cwd: &Path) -> Result<Vec<u8>, Error> {
        self.output_with(program, args, cwd, &[], &[])
    }

    pub fn output_with(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
        additions: &[(&str, &str)],
        removals: &[&str],
    ) -> Result<Vec<u8>, Error> {
        let command = std::iter::once(program)
            .chain(args.iter().copied())
            .map(|arg| if arg.contains("://") { "[URL]" } else { arg })
            .collect::<Vec<_>>()
            .join(" ");
        if self.verbose {
            eprintln!("$ {}", self.redact(&command));
        }
        let mut process = Command::new(program);
        process
            .args(args)
            .current_dir(cwd)
            .env_remove("GITEA_TOKEN")
            .env_remove(&self.token_env);
        for name in removals {
            process.env_remove(name);
        }
        for (name, value) in additions {
            process.env(name, value);
        }
        let output = process
            .output()
            .map_err(|e| Error(format!("cannot run {program}: {e}")))?;
        if !output.status.success() {
            let detail = self.redact(&format!(
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
            return Err(Error(format!(
                "command {} failed with {}: {}",
                self.redact(&command),
                output.status,
                detail.trim()
            )));
        }
        if self.verbose {
            let output_text = self.redact(&String::from_utf8_lossy(&output.stdout));
            if !output_text.trim().is_empty() {
                eprintln!("{}", output_text.trim());
            }
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
        let mut without_token = text.to_owned();
        for secret in &self.secrets {
            without_token = without_token.replace(secret, "[REDACTED]");
        }
        for name in ["GITEA_TOKEN", self.token_env.as_str()] {
            if let Ok(secret) = std::env::var(name) {
                if !secret.is_empty() {
                    without_token = without_token.replace(&secret, "[REDACTED]");
                }
            }
        }
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
        let runner =
            Runner::new("RUST_RELEASE_TEST_SECRET").with_secret(Some("token-loaded-from-file"));
        let secret = "unit-test-secret-value";
        std::env::set_var("RUST_RELEASE_TEST_SECRET", secret);
        let message = runner
            .redact("failed https://user:password@example.com/repo and unit-test-secret-value");
        assert!(!message.contains(secret));
        assert!(!message.contains("password"));
        assert_eq!(runner.redact("token-loaded-from-file"), "[REDACTED]");
        let child = runner.text("env", &[], Path::new(".")).unwrap();
        assert!(!child.contains("RUST_RELEASE_TEST_SECRET"));
        std::env::set_var("RUST_RELEASE_TEST_REMOVED", "remove-me");
        let child = runner
            .output_with(
                "env",
                &[],
                Path::new("."),
                &[("RUST_RELEASE_TEST_ADDED", "added-value")],
                &["RUST_RELEASE_TEST_REMOVED"],
            )
            .unwrap();
        let child = String::from_utf8(child).unwrap();
        assert!(child.contains("RUST_RELEASE_TEST_ADDED=added-value"));
        assert!(!child.contains("RUST_RELEASE_TEST_REMOVED"));
        let error = runner
            .output("git", &["rev-parse", secret], Path::new("."))
            .unwrap_err()
            .to_string();
        assert!(!error.contains(secret));
        std::env::remove_var("RUST_RELEASE_TEST_SECRET");
        std::env::remove_var("RUST_RELEASE_TEST_REMOVED");
    }
}
