//! Keep the refresh token and its identity metadata together in Secret Service.
use super::Grant;
use std::{
    io::{Read, Write},
    process::{Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

const LIMIT: u64 = 16_384;
const UNAVAILABLE: &str = "The system keyring could not be accessed. Unlock it and retry.";

pub(super) struct Keyring {
    namespace: String,
}

impl Keyring {
    pub(super) fn new(namespace: String) -> Self {
        Self { namespace }
    }

    fn run(
        &self,
        args: &[&str],
        input: Option<&[u8]>,
    ) -> Result<(ExitStatus, Vec<u8>, bool), String> {
        let mut child = Command::new("secret-tool")
            .args(args)
            .args([
                "application",
                "top.remya.Core1",
                "google-account",
                &self.namespace,
            ])
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => "Install libsecret to use Google accounts.",
                _ => UNAVAILABLE,
            })?;
        let mut stdout = child.stdout.take().expect("piped stdout").take(LIMIT + 1);
        let mut stderr = child.stderr.take().expect("piped stderr").take(1);
        let output = thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .read_to_end(&mut bytes)
                .ok()
                .filter(|&size| size as u64 <= LIMIT)?;
            Some(bytes)
        });
        let diagnostic = thread::spawn(move || stderr.read(&mut [0]).is_ok_and(|size| size > 0));
        let writer = child.stdin.take().zip(input).map(|(mut stdin, input)| {
            let input = input.to_vec();
            thread::spawn(move || stdin.write_all(&input).is_ok())
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let bytes = output.join().ok().flatten().ok_or(UNAVAILABLE)?;
                    let diagnostic = diagnostic.join().unwrap_or(true);
                    if writer.is_some_and(|writer| !writer.join().unwrap_or(false)) {
                        return Err(UNAVAILABLE.into());
                    }
                    return Ok((status, bytes, diagnostic));
                }
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(UNAVAILABLE.into());
                }
            }
        }
    }

    pub(super) fn load(&self) -> Result<Option<Grant>, String> {
        let (status, bytes, diagnostic) = self.run(&["lookup"], None)?;
        if status.success() {
            return serde_json::from_slice::<Grant>(&bytes)
                .map(|mut grant| {
                    // The legacy events grant cannot list the user's calendars.
                    grant.capabilities.calendar &= grant.calendar_readonly;
                    Some(grant)
                })
                .map_err(|_| "The stored Google authorization is unreadable.".into());
        }
        if status.code() == Some(1) && !diagnostic {
            return Ok(None);
        }
        Err(UNAVAILABLE.into())
    }

    pub(super) fn store(&self, record: &Grant) -> Result<(), String> {
        let bytes = serde_json::to_vec(record)
            .map_err(|_| "Could not prepare the Google authorization for storage.")?;
        if bytes.len() as u64 > LIMIT
            || !self
                .run(
                    &["store", "--label", "Remya Apps Google account"],
                    Some(&bytes),
                )?
                .0
                .success()
        {
            return Err("The system keyring did not store the Google authorization.".into());
        }
        Ok(())
    }

    pub(super) fn clear(&self) -> Result<(), String> {
        if self.run(&["clear"], None)?.0.success() || self.load()?.is_none() {
            return Ok(());
        }
        Err("The system keyring did not remove the Google authorization.".into())
    }
}
