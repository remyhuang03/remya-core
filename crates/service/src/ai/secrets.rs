//! Custom endpoint API keys in the Secret Service through `secret-tool`.
//! The secret travels only on stdin and is never returned or logged.
use gio::glib::variant::{FromVariant, ObjectPath, ToVariant};
use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    os::{fd::OwnedFd, unix::net::UnixStream},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

const APPLICATION: &str = "top.remya.settings";
const TIMEOUT: Duration = Duration::from_secs(3);
// Libsecret's secret-tool stdin reader is limited to an 8 KiB buffer.
const MAX_SECRET_BYTES: usize = 8191;

fn command(action: &str) -> Command {
    let mut command = Command::new("secret-tool");
    command.arg(action);
    command
}

fn attributes(id: &str) -> [&str; 4] {
    ["application", APPLICATION, "ai-provider", id]
}

fn unavailable(error: io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => {
            "Unavailable:Install libsecret (secret-tool) to store API keys.".into()
        }
        io::ErrorKind::TimedOut => "Unavailable:The Secret Service did not answer in time.".into(),
        _ => format!("Unavailable:Cannot run secret-tool: {error}"),
    }
}

#[derive(Clone, Copy)]
enum Capture {
    None,
    Stdout,
    Stderr,
}

/// Only the daemon's endpoint is nonblocking. A stuck child cannot stall the
/// deadline in a read or write, even if it passes its descriptors to descendants.
fn channel() -> io::Result<(UnixStream, Stdio)> {
    let (local, remote) = UnixStream::pair()?;
    local.set_nonblocking(true)?;
    Ok((local, Stdio::from(OwnedFd::from(remote))))
}

fn run(
    mut command: Command,
    secret: Option<&[u8]>,
    capture: Capture,
    timeout: Duration,
) -> io::Result<(ExitStatus, bool)> {
    let deadline = Instant::now() + timeout;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let input = if let Some(secret) = secret {
        let (local, remote) = channel()?;
        command.stdin(remote);
        Some((local, secret))
    } else {
        None
    };
    let output = match capture {
        Capture::None => None,
        Capture::Stdout | Capture::Stderr => {
            let (local, remote) = channel()?;
            match capture {
                Capture::Stdout => command.stdout(remote),
                Capture::Stderr => command.stderr(remote),
                Capture::None => unreachable!(),
            };
            Some(local)
        }
    };
    let mut child = command.spawn()?;
    drop(command);
    let result = exchange(&mut child, input, output, deadline);
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn exchange(
    child: &mut Child,
    mut input: Option<(UnixStream, &[u8])>,
    mut output: Option<UnixStream>,
    deadline: Instant,
) -> io::Result<(ExitStatus, bool)> {
    let mut saw_output = false;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        if let Some((stream, bytes)) = &mut input {
            match stream.write(bytes) {
                Ok(count) => *bytes = &bytes[count..],
                Err(error) if retryable(&error) => {}
                Err(error) => return Err(error),
            }
            if bytes.is_empty() {
                input = None;
            }
        }
        let status = child.try_wait()?;
        if let Some(stream) = &mut output {
            // The caller needs only the presence of output, never its content.
            let mut discard = [0u8; 4096];
            match stream.read(&mut discard) {
                Ok(count) => saw_output |= count != 0,
                Err(error) if retryable(&error) => {}
                Err(error) => return Err(error),
            }
        }
        if let Some(status) = status {
            if status.success() && input.is_some() {
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            return Ok((status, saw_output));
        }
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

fn retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
    )
}

pub fn store(id: &str, secret: &str) -> Result<(), String> {
    if secret.len() > MAX_SECRET_BYTES {
        return Err(format!(
            "InvalidArgument:Custom API keys must be at most {MAX_SECRET_BYTES} bytes."
        ));
    }
    let label = format!("Remy'a Settings AI provider {id}");
    let mut command = command("store");
    command.args(["--label", &label]).args(attributes(id));
    let (status, _) =
        run(command, Some(secret.as_bytes()), Capture::None, TIMEOUT).map_err(unavailable)?;
    if !status.success() {
        return Err("Unavailable:The Secret Service did not store the API key.".into());
    }
    Ok(())
}

pub fn clear(bus: &gio::DBusConnection, id: &str) -> Result<(), String> {
    let started = Instant::now();
    let mut command = command("clear");
    command.args(attributes(id));
    clear_result(run(command, None, Capture::Stderr, TIMEOUT))?;

    // clear only acts on unlocked items. Check both kinds without requesting
    // their secrets or prompting to unlock a collection.
    let remaining = TIMEOUT.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return Err(unavailable(io::ErrorKind::TimedOut.into()));
    }
    let [app, app_value, provider, provider_value] = attributes(id);
    let attributes = BTreeMap::from([(app, app_value), (provider, provider_value)]);
    let reply = bus
        .call_sync(
            Some("org.freedesktop.secrets"),
            "/org/freedesktop/secrets",
            "org.freedesktop.Secret.Service",
            "SearchItems",
            Some(&(attributes,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            remaining.as_millis().max(1) as i32,
            gio::Cancellable::NONE,
        )
        .map_err(|_| "Unavailable:The Secret Service could not verify credential removal.")?;
    if <(Vec<ObjectPath>, Vec<ObjectPath>)>::from_variant(&reply)
        .is_some_and(|(unlocked, locked)| unlocked.is_empty() && locked.is_empty())
    {
        Ok(())
    } else {
        Err("Unavailable:The Secret Service did not remove the API key.".into())
    }
}

fn clear_result(result: io::Result<(ExitStatus, bool)>) -> Result<(), String> {
    let (status, saw_error) = result.map_err(unavailable)?;
    // libsecret reports both no match and a service error with exit code 1.
    // Only the latter writes to stderr.
    if status.success() || (status.code() == Some(1) && !saw_error) {
        return Ok(());
    }
    Err("Unavailable:The Secret Service did not remove the API key.".into())
}

/// Unreachable, hung or locked keyrings report no key rather than blocking or
/// failing the whole read.
pub fn exists(id: &str) -> bool {
    let mut command = command("lookup");
    command.args(attributes(id));
    run(command, None, Capture::Stdout, TIMEOUT)
        .is_ok_and(|(status, has_output)| status.success() && has_output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn blocked_input_times_out_and_reaps_the_child() {
        let temp = tempfile::tempdir().unwrap();
        let pid_file = temp.path().join("pid");
        let mut child = shell("echo $$ > \"$1\"; exec sleep 60");
        child.arg("fixture").arg(&pid_file);
        let before = Instant::now();
        let input = vec![b'a'; 1024 * 1024];
        let error = run(
            child,
            Some(&input),
            Capture::None,
            Duration::from_millis(500),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(before.elapsed() < Duration::from_secs(4));
        let pid = std::fs::read_to_string(pid_file).unwrap();
        assert!(!std::path::Path::new("/proc").join(pid.trim()).exists());

        let (status, got_output) =
            run(shell("cat"), Some(b"test key"), Capture::Stdout, TIMEOUT).unwrap();
        assert!(status.success() && got_output);
    }

    #[test]
    fn clear_distinguishes_no_match_from_a_service_failure() {
        for (script, ok) in [
            ("exit 0", true),
            ("exit 1", true),
            ("echo 'Service unavailable' >&2; exit 1", false),
            ("echo 'Unlock cancelled' >&2; exit 1", false),
            ("exit 2", false),
        ] {
            assert_eq!(
                clear_result(run(shell(script), None, Capture::Stderr, TIMEOUT)).is_ok(),
                ok
            );
        }
        assert!(
            clear_result(run(
                shell("exec sleep 60"),
                None,
                Capture::Stderr,
                Duration::from_millis(50),
            ))
            .unwrap_err()
            .contains("in time")
        );
    }
}
