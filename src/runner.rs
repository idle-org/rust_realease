use crate::error::Error;
use regex::Regex;
use std::{path::Path, process::Command, sync::OnceLock};

pub struct Runner {
    token_envs: Vec<String>,
    secrets: Vec<String>,
    verbose: bool,
}

impl Runner {
    pub fn new(token_env: &str) -> Self {
        let mut token_envs = vec!["GITEA_TOKEN".to_owned(), "GITHUB_TOKEN".to_owned()];
        if !token_envs.iter().any(|name| name == token_env) {
            token_envs.push(token_env.to_owned());
        }
        Self {
            token_envs,
            secrets: Vec::new(),
            verbose: false,
        }
    }

    pub fn with_token_envs(mut self, names: impl IntoIterator<Item = String>) -> Self {
        for name in names {
            if !self.token_envs.contains(&name) {
                self.token_envs.push(name);
            }
        }
        self
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
        process.args(args).current_dir(cwd);
        for name in &self.token_envs {
            process.env_remove(name);
        }
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

    pub fn hook(
        &self,
        argv: &[String],
        cwd: &Path,
        paths: (&Path, &Path, &Path),
        timeout: u64,
        label: &str,
    ) -> Result<(), Error> {
        let (context, manifest, tree) = paths;
        use std::{
            io::{BufRead, BufReader},
            process::Stdio,
            sync::{
                atomic::{AtomicBool, Ordering},
                Arc,
            },
            time::{Duration, Instant},
        };
        let args = argv
            .iter()
            .map(|a| {
                a.replace("{context-file}", &context.to_string_lossy())
                    .replace("{manifest-file}", &manifest.to_string_lossy())
                    .replace("{bundle-directory}", &tree.to_string_lossy())
            })
            .collect::<Vec<_>>();
        let mut command = Command::new(&args[0]);
        command
            .args(&args[1..])
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in &self.token_envs {
            command.env_remove(name);
        }
        for (name, _) in std::env::vars_os() {
            let upper = name.to_string_lossy().to_uppercase();
            if ["TOKEN", "PASSWORD", "SECRET", "CREDENTIAL"]
                .iter()
                .any(|s| upper.contains(s))
            {
                command.env_remove(name);
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        #[cfg(unix)]
        let registrations = [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM]
            .into_iter()
            .map(|signal| signal_hook::flag::register(signal, Arc::clone(&cancelled)))
            .collect::<Result<Vec<_>, _>>()?;
        let result = (|| {
            let mut child = command
                .spawn()
                .map_err(|e| Error(format!("Cannot start hook [{label}] {args:?}: {e}")))?;
            let stdout = child.stdout.take().unwrap();
            let stderr = child.stderr.take().unwrap();
            let started = Instant::now();
            std::thread::scope(|scope| {
                let stream = |reader: &mut dyn BufRead| {
                    let mut line = String::new();
                    loop {
                        line.clear();
                        match reader.read_line(&mut line) {
                            Ok(0) | Err(_) => break,
                            Ok(_) => eprintln!("[{label}] {}", self.redact(line.trim_end())),
                        }
                    }
                };
                scope.spawn(move || stream(&mut BufReader::new(stdout)));
                scope.spawn(move || stream(&mut BufReader::new(stderr)));
                let outcome = loop {
                    if cancelled.load(Ordering::Relaxed)
                        || started.elapsed() >= Duration::from_secs(timeout)
                    {
                        break Err(Error(format!(
                            "Hook [{label}] {:?} {}",
                            args,
                            if cancelled.load(Ordering::Relaxed) {
                                "cancelled"
                            } else {
                                "timed out"
                            }
                        )));
                    }
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            break if status.success() {
                                Ok(())
                            } else {
                                Err(Error(format!(
                                    "Hook [{label}] {:?} failed with {status}",
                                    args
                                )))
                            }
                        }
                        Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                        Err(e) => break Err(Error(e.to_string())),
                    }
                };
                // Kill the group even after the leader exits: descendants may retain pipes.
                #[cfg(unix)]
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                outcome
            })
        })();
        #[cfg(unix)]
        for id in registrations {
            signal_hook::low_level::unregister(id);
        }
        result.map_err(|e: Error| Error(self.redact(&e.to_string())))
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
        for name in &self.token_envs {
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
