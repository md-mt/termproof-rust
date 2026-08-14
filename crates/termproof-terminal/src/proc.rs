//! Running child processes with a deadline.
//!
//! `std::process` has no timeout. A tool that hangs — a renderer waiting on a
//! font server, a converter on a truncated input — otherwise hangs the run with
//! it, and the diagnostic is a job that never finished rather than a named
//! failure.

use std::io;
use std::io::Read;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;
use std::thread;
use std::time::Duration;

/// Current local time formatted as `%Y-%m-%d %H:%M:%S` (matches the Python).
pub fn timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Sleep for a fractional number of seconds.
pub fn sleep_secs(secs: f64) {
    thread::sleep(Duration::from_secs_f64(secs));
}

/// Run `cmd` to completion, capturing stdout/stderr, killing it if it runs
/// longer than `timeout`.
///
/// Returns `io::ErrorKind::TimedOut` if the process had to be killed. stdin is
/// closed (`/dev/null`) so a child that reads from stdin cannot hang the run.
pub fn run_with_timeout(cmd: Command, timeout: Duration) -> io::Result<Output> {
    let (output, timed_out) = run_capturing_timeout(cmd, timeout)?;
    if timed_out {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "process exceeded timeout",
        ));
    }
    Ok(output)
}

/// Like [`run_with_timeout`], but returns whatever the process produced before
/// it was killed, alongside whether it had to be.
///
/// A timed-out run's partial output is still diagnostic — it is the difference
/// between "the tool hung having printed nothing" and "it hung two thirds of
/// the way through" — so callers that report on failures want it rather than a
/// bare error.
pub fn run_capturing_timeout(mut cmd: Command, timeout: Duration) -> io::Result<(Output, bool)> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;

    // Drain both pipes on their own threads, starting now rather than after
    // the wait loop. A pipe holds about one buffer's worth (64 KiB is typical,
    // less on some platforms); past that the child blocks in write() until
    // someone reads. Reading only after the loop would therefore make any
    // child that produces more than a bufferful look exactly like a hang, and
    // it would be killed at the deadline no matter how fast it really was.
    // `ffmpeg` encoding a few hundred frames clears that in stderr alone.
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let drain_stdout = thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = stdout_pipe.as_mut() {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let drain_stderr = thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = stderr_pipe.as_mut() {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    // Poll with try_wait() instead of a separate watchdog thread signaling by
    // raw pid: once the child is reaped (wait() below), its pid can be recycled
    // by the kernel, so a thread still holding that pid would risk killing an
    // unrelated process. try_wait() only ever acts on the live Child handle we
    // own, and we never touch it again after reaping.
    let step = Duration::from_millis(50);
    let mut waited = Duration::ZERO;
    let timed_out = loop {
        if child.try_wait()?.is_some() {
            break false;
        }
        if waited >= timeout {
            child.kill()?;
            break true;
        }
        thread::sleep(step);
        waited += step;
    };

    // Killing the child closes its ends of the pipes, so both readers finish
    // either way and a timed-out run still yields whatever it managed to emit.
    let status = child.wait()?;
    let output = Output {
        status,
        stdout: drain_stdout.join().unwrap_or_default(),
        stderr: drain_stderr.join().unwrap_or_default(),
    };
    Ok((output, timed_out))
}

/// Combine stdout+stderr of an `Output` into one decoded, trimmed string.
pub fn combined_output(output: &Output) -> String {
    let mut parts: Vec<String> = Vec::new();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !stdout.trim().is_empty() {
        parts.push(stdout);
    }
    if !stderr.trim().is_empty() {
        parts.push(stderr);
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_has_expected_shape() {
        let ts = timestamp();
        // YYYY-MM-DD HH:MM:SS is 19 chars.
        assert_eq!(ts.len(), 19);
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[10..11], " ");
    }

    #[test]
    fn quick_command_succeeds() {
        let mut cmd = Command::new("true");
        let out = run_with_timeout(cmd, Duration::from_secs(5)).unwrap();
        assert!(out.status.success());
        cmd = Command::new("echo");
        cmd.arg("hi");
        let out = run_with_timeout(cmd, Duration::from_secs(5)).unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hi");
    }

    #[test]
    fn output_larger_than_a_pipe_buffer_does_not_look_like_a_hang() {
        // A pipe drained only after the wait loop fills at ~64 KiB and blocks
        // the child, which then gets killed at the deadline. 1 MB is well past
        // that on every platform, and 5s is far longer than `head` needs.
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("yes hello | head -c 1000000");
        let out = run_with_timeout(cmd, Duration::from_secs(5)).expect("should not time out");
        assert_eq!(out.stdout.len(), 1_000_000);
    }

    #[test]
    fn a_timed_out_run_still_yields_what_it_printed() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("echo partial; sleep 10");
        let (output, timed_out) =
            run_capturing_timeout(cmd, Duration::from_millis(300)).expect("spawned");
        assert!(timed_out);
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "partial");
    }

    #[test]
    fn slow_command_times_out() {
        let mut cmd = Command::new("sleep");
        cmd.arg("10");
        let err = run_with_timeout(cmd, Duration::from_millis(200)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    }
}
