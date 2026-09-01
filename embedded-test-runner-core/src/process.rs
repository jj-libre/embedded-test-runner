//! Child processes, drained and bounded by a timeout.

use std::fmt;
use std::io::{self, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use thiserror::Error;
use wait_timeout::ChildExt;

/// A child that produced no status at all, so nothing was learned by running
/// it. Distinct from every `ProcessStatus`, each of which is an outcome.
#[derive(Debug, Error)]
pub enum ProcessError {
    /// The program never started. The message names the program alone;
    /// spelling the cause here too makes anyhow's `{:#}` print it twice.
    #[error("failed to spawn `{program}`")]
    Spawn { program: String, source: io::Error },
    /// The child started, but the runner could not bring it to an end and
    /// collect it.
    #[error("waiting on `{program}`")]
    Wait { program: String, source: io::Error },
}

/// How a child that ran ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    Exited(i32),
    /// Ended abnormally, carrying no exit code.
    Terminated,
    /// Still running at the timeout, then killed.
    TimedOut,
}

impl ProcessStatus {
    fn from_code(code: Option<i32>) -> Self {
        match code {
            Some(code) => ProcessStatus::Exited(code),
            None => ProcessStatus::Terminated,
        }
    }
}

/// What the child wrote. The streams stay apart: a venue that parses one of
/// them needs it on its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Captured {
    pub stdout: String,
    pub stderr: String,
}

impl fmt::Display for Captured {
    /// Trimmed, stdout before stderr, and nothing at all from a silent child.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.stdout.trim(), self.stderr.trim()) {
            ("", err) => f.write_str(err),
            (out, "") => f.write_str(out),
            (out, err) => write!(f, "{out}\n{err}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutcome {
    pub status: ProcessStatus,
    pub captured: Captured,
}

/// A child that started, with both its streams already draining.
#[derive(Debug)]
pub struct Running {
    child: Child,
    program: String,
    stdout: Drain,
    stderr: Drain,
}

/// Starts the child with both streams piped and draining. Returning says the
/// process exists, which is the one thing no status can say.
pub fn start(mut cmd: Command) -> Result<Running, ProcessError> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let program = cmd.get_program().to_string_lossy().into_owned();

    let mut child = cmd.spawn().map_err(|source| ProcessError::Spawn {
        program: program.clone(),
        source,
    })?;

    let stdout = Drain::start(child.stdout.take());
    let stderr = Drain::start(child.stderr.take());

    Ok(Running {
        child,
        program,
        stdout,
        stderr,
    })
}

impl Running {
    /// Waits for the child, killing it if it overruns. A `None` timeout waits
    /// as long as the child takes.
    pub fn finish(mut self, timeout: Option<Duration>) -> Result<ProcessOutcome, ProcessError> {
        let status = self.wait(timeout)?;

        Ok(ProcessOutcome {
            status,
            captured: Captured {
                stdout: self.stdout.finish(DRAIN_GRACE),
                stderr: self.stderr.finish(DRAIN_GRACE),
            },
        })
    }

    fn wait(&mut self, timeout: Option<Duration>) -> Result<ProcessStatus, ProcessError> {
        let waited = match timeout {
            Some(timeout) => self.child.wait_timeout(timeout),
            None => self.child.wait().map(Some),
        };

        let Some(status) = waited.map_err(|source| self.waiting(source))? else {
            self.stop()?;
            return Ok(ProcessStatus::TimedOut);
        };

        Ok(ProcessStatus::from_code(status.code()))
    }

    /// Ends a child that overran and collects it. A failed kill leaves a
    /// process holding its pipes: a venue fault, not a timeout.
    fn stop(&mut self) -> Result<(), ProcessError> {
        let killed = self.child.kill();
        killed.map_err(|source| self.waiting(source))?;

        let reaped = self.child.wait();
        reaped.map_err(|source| self.waiting(source))?;

        Ok(())
    }

    fn waiting(&self, source: io::Error) -> ProcessError {
        ProcessError::Wait {
            program: self.program.clone(),
            source,
        }
    }
}

/// Child run to completion or to the timeout. A `None` timeout waits as long as
/// the child takes.
pub fn run(cmd: Command, timeout: Option<Duration>) -> Result<ProcessOutcome, ProcessError> {
    start(cmd)?.finish(timeout)
}

/// How long a stream may take to end after the child has, before whatever is
/// left of it is given up on.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// A stream read on its own thread into a buffer the caller can take at any
/// time. Reading only after the wait would deadlock any child that writes more
/// than one pipe buffer.
#[derive(Debug)]
struct Drain {
    bytes: Arc<Mutex<Vec<u8>>>,
    ended: Receiver<()>,
}

impl Drain {
    fn start<R: Read + Send + 'static>(stream: Option<R>) -> Self {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let into = Arc::clone(&bytes);
        let (sender, ended) = mpsc::channel();

        thread::spawn(move || {
            // Declared first, so it outlives the read and is dropped however
            // the thread ends. Dropping it is what `finish` waits for.
            let _ended = sender;

            let Some(mut stream) = stream else { return };
            let mut chunk = [0u8; 8 << 10];
            while let Ok(read) = stream.read(&mut chunk) {
                if read == 0 {
                    return;
                }
                lock(&into).extend_from_slice(&chunk[..read]);
            }
        });

        Self { bytes, ended }
    }

    /// Everything read by the time the stream ends or the grace runs out.
    /// Joining instead would hang: a descendant that inherited the write end
    /// keeps the pipe open after the child itself is gone.
    fn finish(self, grace: Duration) -> String {
        let _ = self.ended.recv_timeout(grace);
        // Lossy on purpose: read_to_string leaves its buffer empty on invalid
        // UTF-8, which would discard every diagnostic line the guest wrote.
        String::from_utf8_lossy(&lock(&self.bytes)).into_owned()
    }
}

/// A panicking drain thread cannot have left the buffer inconsistent; it only
/// ever appends.
fn lock(bytes: &Mutex<Vec<u8>>) -> MutexGuard<'_, Vec<u8>> {
    bytes.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    // The trait, not the derive macro `super::*` brings in.
    use std::error::Error as _;
    use std::time::Instant;

    use super::*;

    fn captured(stdout: &str, stderr: &str) -> Captured {
        Captured {
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        }
    }

    /// Hands over what it has and then blocks, the way a pipe reads when a
    /// descendant of the child still holds the write end open.
    struct NeverEnds(Vec<u8>);

    impl Read for NeverEnds {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.0.is_empty() {
                loop {
                    thread::park();
                }
            }
            let read = self.0.len().min(buffer.len());
            buffer[..read].copy_from_slice(&self.0[..read]);
            self.0.drain(..read);
            Ok(read)
        }
    }

    #[test]
    fn test_stream_that_was_never_piped_drains_to_nothing() {
        let drain = Drain::start(None::<std::process::ChildStdout>);
        assert_eq!(drain.finish(DRAIN_GRACE), "");
    }

    #[test]
    fn test_a_stream_that_never_ends_gives_up_what_it_read() {
        const GRACE: Duration = Duration::from_millis(200);

        let drain = Drain::start(Some(NeverEnds(b"partial".to_vec())));

        let started = Instant::now();
        let read = drain.finish(GRACE);
        let waited = started.elapsed();

        assert_eq!(read, "partial");
        assert!(waited >= GRACE, "{waited:?}");
        assert!(waited < Duration::from_secs(5), "{waited:?}");
    }

    #[test]
    fn test_a_failure_names_the_program_and_leaves_the_cause_alone() {
        let error = ProcessError::Wait {
            program: "qemu-system-arm".to_string(),
            source: io::Error::other("handle went away"),
        };

        assert_eq!(error.to_string(), "waiting on `qemu-system-arm`");
        assert_eq!(
            error.source().unwrap().to_string(),
            "handle went away",
            "the cause belongs to the chain, not inlined into the message"
        );
    }

    #[test]
    fn test_a_spawn_failure_keeps_the_io_kind() {
        let error = ProcessError::Spawn {
            program: "no-such-binary".to_string(),
            source: io::Error::from(io::ErrorKind::NotFound),
        };

        let source: &io::Error = error.source().unwrap().downcast_ref().unwrap();
        assert_eq!(source.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn test_child_with_an_exit_code_has_exited() {
        assert_eq!(
            ProcessStatus::from_code(Some(134)),
            ProcessStatus::Exited(134)
        );
    }

    #[test]
    fn test_child_without_an_exit_code_was_terminated() {
        assert_eq!(ProcessStatus::from_code(None), ProcessStatus::Terminated);
    }

    #[test]
    fn test_stdout_alone_is_shown() {
        assert_eq!(captured("  out  ", "").to_string(), "out");
    }

    #[test]
    fn test_stderr_alone_is_shown() {
        assert_eq!(captured("", "  err  ").to_string(), "err");
    }

    #[test]
    fn test_both_streams_are_shown_stdout_first() {
        assert_eq!(
            captured("out", "err").to_string(),
            "out
err"
        );
    }

    #[test]
    fn test_silent_process_shows_nothing() {
        assert_eq!(
            captured(
                "  ", "
"
            )
            .to_string(),
            ""
        );
    }

    #[test]
    fn test_the_streams_are_kept_apart() {
        let captured = captured("out", "err");
        assert_eq!(captured.stdout, "out");
        assert_eq!(captured.stderr, "err");
    }
}
