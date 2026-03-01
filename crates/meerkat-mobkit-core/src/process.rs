use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessBoundaryError {
    SpawnFailed(String),
    MissingStdout,
    Io(String),
    Timeout { timeout_ms: u64 },
    EmptyOutput,
    InvalidJsonLine,
}

pub fn run_process_json_line(
    command: &str,
    args: &[String],
    env: &[(String, String)],
    timeout: Duration,
) -> Result<String, ProcessBoundaryError> {
    let mut child = Command::new(command)
        .args(args)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| ProcessBoundaryError::SpawnFailed(err.to_string()))?;

    let stdout = child
        .stdout
        .take()
        .ok_or(ProcessBoundaryError::MissingStdout)?;
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let read = reader.read_line(&mut line).map_err(|err| err.to_string());
        let _ = tx.send((read, line));
    });

    match rx.recv_timeout(timeout) {
        Ok((Ok(0), _)) => {
            let _ = child.wait();
            Err(ProcessBoundaryError::EmptyOutput)
        }
        Ok((Ok(_), mut line)) => {
            let _ = child.wait();
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            if serde_json::from_str::<serde_json::Value>(&line).is_err() {
                return Err(ProcessBoundaryError::InvalidJsonLine);
            }
            Ok(line)
        }
        Ok((Err(err), _)) => {
            let _ = child.wait();
            Err(ProcessBoundaryError::Io(err))
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(ProcessBoundaryError::Timeout {
                timeout_ms: timeout.as_millis() as u64,
            })
        }
    }
}
