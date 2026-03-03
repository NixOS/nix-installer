use std::{process::Output, time::SystemTime};

use crate::util::which;
use std::process::Command;

const SELF_TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum SelfTestError {
    #[error("Shell `{shell}` failed self-test with command `{command}`, stderr:\n{}", String::from_utf8_lossy(&output.stderr))]
    ShellFailed {
        shell: Shell,
        command: String,
        output: Output,
    },
    /// Failed to execute command
    #[error("Failed to execute command `{command}`",
        command = .command,
    )]
    Command {
        shell: Shell,
        command: String,
        #[source]
        error: std::io::Error,
    },
    #[error(transparent)]
    SystemTime(#[from] std::time::SystemTimeError),
    #[error("command timed out")]
    TimedOut { shell: Shell, command: String },
}

#[derive(Clone, Copy, Debug)]
pub enum Shell {
    Sh,
    Bash,
    Fish,
    Zsh,
}

impl std::fmt::Display for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.executable())
    }
}

impl Shell {
    pub fn all() -> &'static [Shell] {
        &[Shell::Sh, Shell::Bash, Shell::Fish, Shell::Zsh]
    }
    pub fn executable(&self) -> &'static str {
        match &self {
            Shell::Sh => "sh",
            Shell::Bash => "bash",
            Shell::Fish => "fish",
            Shell::Zsh => "zsh",
        }
    }

    #[tracing::instrument(level = "debug", skip_all)]
    pub fn self_test(&self) -> Result<(), SelfTestError> {
        let executable = self.executable();

        tracing::info!("Running self test for shell {executable}");

        let mut command = match &self {
            // On Mac, `bash -ic nix` won't work, but `bash -lc nix` will.
            Shell::Sh | Shell::Bash => {
                let mut command = Command::new(executable);
                command.arg("-lc");
                command
            },
            Shell::Zsh | Shell::Fish => {
                let mut command = Command::new(executable);
                command.arg("-ic");
                command
            },
        };

        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        const SYSTEM: &str = "x86_64-linux";
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        const SYSTEM: &str = "aarch64-linux";
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        const SYSTEM: &str = "x86_64-darwin";
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        const SYSTEM: &str = "aarch64-darwin";

        let timestamp_millis = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_millis();

        command.arg(format!(
            r#"exec nix build --option substitute false --option post-build-hook '' --no-link --expr 'derivation {{ name = "self-test-{executable}-{timestamp_millis}"; system = "{SYSTEM}"; builder = "/bin/sh"; args = ["-c" "echo hello > \$out"]; }}'"#
        ));
        let command_str = format!("{:?}", command);

        tracing::debug!(
            command = command_str,
            "Testing Nix install via `{executable}`"
        );
        let mut child = command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env("NIX_REMOTE", "daemon")
            .spawn()
            .map_err(|error| SelfTestError::Command {
                shell: *self,
                command: command_str.clone(),
                error,
            })?;

        let deadline = std::time::Instant::now() + SELF_TEST_TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(SelfTestError::TimedOut {
                            shell: *self,
                            command: command_str,
                        });
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                },
                Err(error) => {
                    return Err(SelfTestError::Command {
                        shell: *self,
                        command: command_str,
                        error,
                    });
                },
            }
        };

        let output = Output {
            status,
            stdout: child.stdout.map_or_else(Vec::new, |mut s| {
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut s, &mut buf).unwrap_or(0);
                buf
            }),
            stderr: child.stderr.map_or_else(Vec::new, |mut s| {
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut s, &mut buf).unwrap_or(0);
                buf
            }),
        };

        if output.status.success() {
            Ok(())
        } else {
            Err(SelfTestError::ShellFailed {
                shell: *self,
                command: command_str,
                output,
            })
        }
    }

    #[tracing::instrument(level = "debug", skip_all)]
    pub fn discover() -> Vec<Shell> {
        let mut found_shells = vec![];
        for shell in Self::all() {
            if which(shell.executable()).is_some() {
                tracing::debug!("Discovered `{shell}`");
                found_shells.push(*shell)
            }
        }
        found_shells
    }
}

#[tracing::instrument(level = "debug", skip_all)]
pub fn self_test() -> Result<(), Vec<SelfTestError>> {
    let shells = Shell::discover();

    tracing::debug!(?shells, "Discovered shells to self test");

    let mut failures = vec![];

    for shell in shells {
        match shell.self_test() {
            Ok(()) => (),
            Err(err) => failures.push(err),
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}
