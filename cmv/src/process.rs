//! The [`ProcessRunner`] gateway: run a validator executable with arguments, a
//! working directory, and a timeout, and report how it ended. This is cmv's
//! one effect beyond the filesystem, so it is a trait with a real
//! implementation ([`RealProcessRunner`]) and a scripted fake
//! ([`FakeProcessRunner`]) rather than a bare `std::process::Command` call.
//!
//! It lives here rather than in `cmx-core` because cmx-core is twinned with a
//! TypeScript port under a conformance suite and released in lockstep; a new
//! gateway there would trigger a coordinated release for a capability only cmv
//! uses.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Everything needed to start one validator process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRequest {
    /// The executable to run.
    pub program: PathBuf,
    /// Arguments, in order, excluding the program name.
    pub args: Vec<OsString>,
    /// Working directory for the child.
    pub cwd: PathBuf,
    /// How long to wait before killing the child.
    pub timeout: Duration,
}

/// How a process run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessOutcome {
    /// The child ran to completion within the timeout.
    Exited {
        /// Exit code, or `None` when the child was terminated by a signal.
        code: Option<i32>,
        /// Captured stdout, lossily decoded as UTF-8.
        stdout: String,
        /// Captured stderr, lossily decoded as UTF-8.
        stderr: String,
    },
    /// The child was still running when the timeout elapsed and was killed.
    TimedOut,
    /// The child could not be started (missing executable, permission
    /// denied, …).
    FailedToStart {
        /// The operating system's reason.
        reason: String,
    },
}

/// Runs one process to completion, or to its timeout.
pub trait ProcessRunner {
    /// Run `request` and report how it ended. Never panics on a misbehaving
    /// child: every failure mode is a [`ProcessOutcome`] variant.
    fn run(&self, request: &ProcessRequest) -> ProcessOutcome;
}

/// Production runner backed by `std::process::Command`.
///
/// stdout and stderr are drained on their own threads so a chatty child cannot
/// block on a full pipe while the parent waits for it. On timeout the child is
/// killed; on unix the child is started in its own process group and the whole
/// group is signalled, so helper processes a validator spawned (a fact
/// extractor, an interpreter) do not outlive it and keep the pipes open.
pub struct RealProcessRunner;

impl ProcessRunner for RealProcessRunner {
    fn run(&self, request: &ProcessRequest) -> ProcessOutcome {
        let mut command = Command::new(&request.program);
        command
            .args(&request.args)
            .current_dir(&request.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                return ProcessOutcome::FailedToStart {
                    reason: error.to_string(),
                };
            }
        };
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return ProcessOutcome::Exited {
                        code: status.code(),
                        stdout: stdout.join().unwrap_or_default(),
                        stderr: stderr.join().unwrap_or_default(),
                    };
                }
                Ok(None) if started.elapsed() >= request.timeout => {
                    kill_process_group(&mut child);
                    // The reader threads end when the last pipe closes; they
                    // are not joined so an escaped grandchild cannot hang cmv.
                    return ProcessOutcome::TimedOut;
                }
                Ok(None) => thread::sleep(POLL_INTERVAL),
                Err(error) => {
                    kill_process_group(&mut child);
                    return ProcessOutcome::FailedToStart {
                        reason: error.to_string(),
                    };
                }
            }
        }
    }
}

const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Read a pipe to exhaustion on a background thread.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            // A read error mid-stream leaves whatever arrived; the exit status
            // still tells the caller what happened.
            let _ = pipe.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// Kill the child and, on unix, every process in its group.
///
/// The group is signalled through `kill(1)` because the workspace denies
/// `unsafe_code`, which a direct `libc::kill` would need; `kill` is part of
/// POSIX and present wherever `/bin/sh` is. The direct `Child::kill` follows
/// as a fallback so the child itself dies even if `kill(1)` is unavailable,
/// and `wait` reaps it.
fn kill_process_group(child: &mut Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{}", child.id())])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// In-memory runner scripted by executable path; records every request so
/// tests can assert on argv, working directory, and timeout.
#[derive(Default)]
pub struct FakeProcessRunner {
    scripts: HashMap<PathBuf, ProcessOutcome>,
    calls: RefCell<Vec<ProcessRequest>>,
}

impl FakeProcessRunner {
    /// A runner with no scripts; every program fails to start until scripted.
    pub fn new() -> Self {
        Self::default()
    }

    /// Script what running `program` yields.
    #[must_use]
    pub fn script(mut self, program: impl Into<PathBuf>, outcome: ProcessOutcome) -> Self {
        self.scripts.insert(program.into(), outcome);
        self
    }

    /// Script `program` to exit 0 with `stdout`, the shape of a well-behaved
    /// validator.
    #[must_use]
    pub fn script_stdout(self, program: impl Into<PathBuf>, stdout: impl Into<String>) -> Self {
        self.script(program, exited(0, stdout, ""))
    }

    /// Every request made so far, in order.
    pub fn calls(&self) -> Vec<ProcessRequest> {
        self.calls.borrow().clone()
    }
}

impl ProcessRunner for FakeProcessRunner {
    fn run(&self, request: &ProcessRequest) -> ProcessOutcome {
        self.calls.borrow_mut().push(request.clone());
        self.scripts.get(&request.program).cloned().unwrap_or_else(|| {
            ProcessOutcome::FailedToStart {
                reason: format!(
                    "No such file or directory (fake: {} is not scripted)",
                    request.program.display()
                ),
            }
        })
    }
}

/// Build an [`ProcessOutcome::Exited`] outcome.
pub fn exited(code: i32, stdout: impl Into<String>, stderr: impl Into<String>) -> ProcessOutcome {
    ProcessOutcome::Exited {
        code: Some(code),
        stdout: stdout.into(),
        stderr: stderr.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(program: &str) -> ProcessRequest {
        ProcessRequest {
            program: PathBuf::from(program),
            args: vec![OsString::from("--flag")],
            cwd: PathBuf::from("/kb"),
            timeout: Duration::from_secs(1),
        }
    }

    #[test]
    fn fake_returns_scripted_outcome_and_records_the_request() {
        let runner = FakeProcessRunner::new().script_stdout("/kb/check", "{}\n");
        let outcome = runner.run(&request("/kb/check"));
        assert_eq!(outcome, exited(0, "{}\n", ""));
        assert_eq!(runner.calls(), vec![request("/kb/check")]);
    }

    #[test]
    fn fake_fails_to_start_unscripted_programs() {
        let runner = FakeProcessRunner::new();
        let outcome = runner.run(&request("/kb/missing"));
        assert!(
            matches!(outcome, ProcessOutcome::FailedToStart { ref reason } if reason.contains("/kb/missing")),
            "{outcome:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn real_runner_captures_exit_code_and_streams() {
        let outcome = RealProcessRunner.run(&ProcessRequest {
            program: PathBuf::from("/bin/sh"),
            args: ["-c", "echo out; echo err >&2; exit 3"].iter().map(OsString::from).collect(),
            cwd: std::env::temp_dir(),
            timeout: Duration::from_secs(5),
        });
        assert_eq!(outcome, exited(3, "out\n", "err\n"));
    }

    #[cfg(unix)]
    #[test]
    fn real_runner_passes_arguments_and_working_directory() {
        let cwd = std::env::temp_dir().canonicalize().unwrap();
        let outcome = RealProcessRunner.run(&ProcessRequest {
            program: PathBuf::from("/bin/sh"),
            args: ["-c", "printf '%s|%s' \"$1\" \"$(pwd -P)\"", "sh", "value"]
                .iter()
                .map(OsString::from)
                .collect(),
            cwd: cwd.clone(),
            timeout: Duration::from_secs(5),
        });
        assert_eq!(outcome, exited(0, format!("value|{}", cwd.display()), ""));
    }

    #[cfg(unix)]
    #[test]
    fn real_runner_kills_on_timeout_even_with_a_lingering_grandchild() {
        let started = Instant::now();
        let outcome = RealProcessRunner.run(&ProcessRequest {
            program: PathBuf::from("/bin/sh"),
            args: ["-c", "sleep 30 & sleep 30"].iter().map(OsString::from).collect(),
            cwd: std::env::temp_dir(),
            timeout: Duration::from_millis(200),
        });
        assert_eq!(outcome, ProcessOutcome::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "timeout must not wait for the grandchild"
        );
    }

    #[test]
    fn real_runner_reports_a_missing_executable() {
        let outcome = RealProcessRunner.run(&request("/definitely/not/here"));
        assert!(matches!(outcome, ProcessOutcome::FailedToStart { .. }), "{outcome:?}");
    }
}
