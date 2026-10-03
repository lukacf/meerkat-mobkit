//! One owner for a started module process.
//!
//! A module process is started for its first event line and, when healthy, kept
//! as the module's live child. Every path that stops owning it ends in this
//! owner: a failed start, a failed bootstrap after modules started, an aborted
//! respawn, shutdown and dropping the runtime handle. Releasing an owner whose
//! termination was not confirmed kills the direct child and waits for its exit,
//! so no module process is left running or unreaped, and no reaper thread is
//! detached. Descendants of the module are not owned.
//!
//! On Linux and macOS the first line is read on the calling thread from a
//! non-blocking stdout, bounded by the start timeout, and the read end is
//! closed on return. No reader thread exists, so none can stay blocked on a
//! pipe a descendant still holds. Other targets keep the legacy reader thread.

use std::io;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::Duration;

use crate::process::ProcessBoundaryError;

/// How a start attempt's first stdout line ended.
#[derive(Debug)]
pub(super) enum FirstLine {
    Line(String),
    /// Stdout reached end of file before a complete line.
    Closed,
    /// The start timeout elapsed first.
    TimedOut,
    ReadFailed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessState {
    Running,
    Reaped,
}

#[derive(Debug)]
pub(super) struct ModuleProcess {
    child: Child,
    state: ProcessState,
}

impl ModuleProcess {
    /// Spawn with piped stdout. The returned stdout is the module's only
    /// output channel; pass it to [`read_first_line`].
    pub(super) fn spawn(mut command: Command) -> Result<(Self, ChildStdout), ProcessBoundaryError> {
        let child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| ProcessBoundaryError::SpawnFailed(err.to_string()))?;
        let mut process = Self {
            child,
            state: ProcessState::Running,
        };
        // `process` owns the child from here on, so an early return reaps it.
        let stdout = process
            .child
            .stdout
            .take()
            .ok_or(ProcessBoundaryError::MissingStdout)?;
        Ok((process, stdout))
    }

    /// Kill (unless already exited) and reap the direct child. On failure the
    /// process stays owned here and is killed and reaped when the owner is
    /// released.
    pub(super) fn terminate(&mut self, force_failure: bool) -> Result<(), String> {
        if self.state == ProcessState::Reaped {
            return Ok(());
        }
        if force_failure || test_seam::termination_fails() {
            return Err("forced terminate failure for testing".to_string());
        }
        let result = match self.child.try_wait() {
            Ok(Some(_)) => Ok(()),
            Ok(None) => match self.child.kill() {
                Ok(()) => self
                    .child
                    .wait()
                    .map(|_| ())
                    .map_err(|err| format!("wait after kill failed: {err}")),
                Err(kill_err) => match self.child.try_wait() {
                    Ok(Some(_)) => Ok(()),
                    Ok(None) => Err(format!(
                        "kill failed while process still running: {kill_err}"
                    )),
                    Err(probe_err) => Err(format!(
                        "kill failed and process status probe failed: {kill_err}; {probe_err}"
                    )),
                },
            },
            Err(err) => Err(format!("try_wait failed: {err}")),
        };
        if result.is_ok() {
            self.state = ProcessState::Reaped;
        }
        result
    }
}

impl Drop for ModuleProcess {
    fn drop(&mut self) {
        if self.state == ProcessState::Reaped {
            return;
        }
        // Kill fails only when the child already exited; wait reaps it either
        // way. After SIGKILL a direct child's wait returns promptly.
        let _ = self.child.kill();
        match self.child.wait() {
            Ok(_) => self.state = ProcessState::Reaped,
            Err(err) => tracing::warn!(
                pid = self.child.id(),
                error = %err,
                "module process wait failed while releasing its owner"
            ),
        }
    }
}

/// Read one line within `timeout`. `stdout` is closed when this returns.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn read_first_line(stdout: ChildStdout, timeout: Duration) -> FirstLine {
    use std::io::Read;
    use std::time::Instant;

    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};

    let mut stdout = stdout;
    if let Err(err) =
        fcntl_getfl(&stdout).and_then(|flags| fcntl_setfl(&stdout, flags | OFlags::NONBLOCK))
    {
        return FirstLine::ReadFailed(format!("failed to configure stdout: {err}"));
    }
    let started = Instant::now();
    let mut line = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let Some(remaining) = timeout
            .checked_sub(started.elapsed())
            .filter(|remaining| !remaining.is_zero())
        else {
            return FirstLine::TimedOut;
        };
        // macOS poll has a signed-millisecond ceiling; every pass still uses
        // the same start deadline.
        let wait = match Timespec::try_from(remaining.min(Duration::from_millis(i32::MAX as u64))) {
            Ok(wait) => wait,
            Err(err) => return FirstLine::ReadFailed(err.to_string()),
        };
        let mut fds = [PollFd::new(&stdout, PollFlags::IN)];
        match poll(&mut fds, Some(&wait)) {
            Ok(0) => continue,
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => continue,
            Err(err) => return FirstLine::ReadFailed(err.to_string()),
        }
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) => return FirstLine::Closed,
                Ok(read) => {
                    let chunk = &chunk[..read];
                    if let Some(end) = chunk.iter().position(|byte| *byte == b'\n') {
                        line.extend_from_slice(&chunk[..=end]);
                        return decode_line(line);
                    }
                    line.extend_from_slice(chunk);
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return FirstLine::ReadFailed(err.to_string()),
            }
        }
    }
}

/// Legacy reader thread. It is not joined: a descendant holding stdout can
/// keep it blocked after the module is reaped.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn read_first_line(stdout: ChildStdout, timeout: Duration) -> FirstLine {
    use std::io::{BufRead, BufReader};

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let result = reader.read_line(&mut line).map_err(|err| err.to_string());
        let _ = tx.send((result, line));
    });
    match rx.recv_timeout(timeout) {
        Ok((Ok(0), _)) => FirstLine::Closed,
        Ok((Ok(_), line)) => FirstLine::Line(line),
        Ok((Err(err), _)) => FirstLine::ReadFailed(err),
        Err(_) => FirstLine::TimedOut,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn decode_line(bytes: Vec<u8>) -> FirstLine {
    match String::from_utf8(bytes) {
        Ok(line) => FirstLine::Line(line),
        Err(err) => FirstLine::ReadFailed(err.to_string()),
    }
}

/// Crate-internal fault injection for terminate paths that the public
/// `supervisor_test_force_terminate_failure` option does not reach.
#[cfg(test)]
pub(super) mod test_seam {
    use std::cell::Cell;

    thread_local! {
        static FAIL_TERMINATIONS: Cell<bool> = const { Cell::new(false) };
    }

    pub(in crate::runtime) fn termination_fails() -> bool {
        FAIL_TERMINATIONS.with(Cell::get)
    }

    /// Make every termination on this thread fail until the guard drops.
    pub(in crate::runtime) fn fail_terminations() -> FailTerminations {
        FAIL_TERMINATIONS.with(|value| value.set(true));
        FailTerminations
    }

    pub(in crate::runtime) struct FailTerminations;

    impl Drop for FailTerminations {
        fn drop(&mut self) {
            FAIL_TERMINATIONS.with(|value| value.set(false));
        }
    }
}

#[cfg(not(test))]
mod test_seam {
    pub(super) fn termination_fails() -> bool {
        false
    }
}
