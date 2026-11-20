//! Core's shared Google identity and OAuth grant for desktop applications.
mod avatar;
mod flow;
mod keyring;
mod tasks;

pub use avatar::AvatarSnapshot;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use flow::Credentials;
use keyring::Keyring;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc::Sender},
    thread,
};

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Capabilities {
    calendar: bool,
    tasks: bool,
    profile: bool,
}

impl Capabilities {
    fn with(self, requested: Self) -> Self {
        Self {
            calendar: self.calendar || requested.calendar,
            tasks: self.tasks || requested.tasks,
            profile: self.profile || requested.profile,
        }
    }

    fn includes(self, requested: Self) -> bool {
        (!requested.calendar || self.calendar)
            && (!requested.tasks || self.tasks)
            && (!requested.profile || self.profile)
    }
}

#[derive(Deserialize, Serialize)]
struct Grant {
    client_id: String,
    subject: String,
    email: String,
    capabilities: Capabilities,
    refresh_token: String,
    #[serde(default)]
    avatar_sync: bool,
}

#[derive(Serialize)]
pub struct Status {
    configured: bool,
    pending: bool,
    email: Option<String>,
    #[serde(rename = "accountId")]
    account_id: Option<String>,
    #[serde(flatten)]
    capabilities: Capabilities,
    #[serde(rename = "avatarSync")]
    avatar_sync: bool,
    error: Option<String>,
}

#[derive(Default)]
struct State {
    avatar: AvatarSnapshot,
    pending: bool,
    error: Option<String>,
}

pub struct Account {
    config: PathBuf,
    avatar_cache: PathBuf,
    keyring: Keyring,
    state: Mutex<State>,
    tasks: Mutex<Option<tasks::AccessToken>>,
    tasks_agent: ureq::Agent,
    tasks_fresh_agent: ureq::Agent,
    notifier: Mutex<Option<Sender<()>>>,
}

impl Account {
    pub fn new(config: PathBuf, avatar_cache: PathBuf) -> Self {
        let namespace =
            URL_SAFE_NO_PAD.encode(Sha256::digest(config.as_os_str().as_encoded_bytes()));
        Self {
            config,
            avatar_cache,
            keyring: Keyring::new(namespace),
            state: Mutex::default(),
            tasks: Mutex::default(),
            tasks_agent: flow::google_https(false),
            tasks_fresh_agent: flow::google_https_fresh(false),
            notifier: Mutex::default(),
        }
    }

    pub fn set_notifier(&self, notifier: Sender<()>) {
        *self.notifier.lock().unwrap() = Some(notifier);
    }

    fn notify(&self) {
        if let Some(notifier) = self.notifier.lock().unwrap().as_ref() {
            let _ = notifier.send(());
        }
    }

    fn snapshot(
        state: &State,
        credentials: Result<Option<Credentials>, String>,
        grant: Result<Option<Grant>, String>,
    ) -> Status {
        let configured = credentials.as_ref().is_ok_and(Option::is_some);
        let config_error = match (&credentials, &grant) {
            (Err(error), _) => Some(error.clone()),
            (Ok(Some(client)), Ok(Some(grant))) if client.client_id != grant.client_id => Some(
                "Disconnect the previous Google authorization before using the new client.".into(),
            ),
            _ => None,
        };
        let (email, account_id, capabilities, avatar_sync, keyring_error) = match grant {
            Ok(Some(grant)) => (
                Some(grant.email),
                Some(grant.subject),
                grant.capabilities,
                grant.avatar_sync,
                None,
            ),
            Ok(None) => (None, None, Capabilities::default(), false, None),
            Err(error) => (None, None, Capabilities::default(), false, Some(error)),
        };
        Status {
            configured,
            pending: state.pending,
            email,
            account_id,
            capabilities,
            avatar_sync,
            error: config_error
                .or_else(|| state.error.clone())
                .or(keyring_error),
        }
    }

    pub fn status(&self) -> Status {
        let state = self.state.lock().unwrap();
        let credentials = Credentials::load(&self.config);
        let grant = if credentials.as_ref().is_ok_and(Option::is_none) {
            Ok(None)
        } else {
            self.keyring.load()
        };
        Self::snapshot(&state, credentials, grant)
    }

    pub fn connect(self: &Arc<Self>, requested: Capabilities) -> Result<Status, String> {
        self.authorize(requested, false)
    }

    fn authorize(
        self: &Arc<Self>,
        requested: Capabilities,
        enable_avatar: bool,
    ) -> Result<Status, String> {
        let mut state = self.state.lock().unwrap();
        if state.pending {
            return Err("A Google authorization is already in progress.".into());
        }
        let client = Credentials::load(&self.config)?
            .ok_or("Unavailable:Google OAuth client credentials are missing.")?;
        let mut previous = self
            .keyring
            .load()
            .map_err(|error| format!("Unavailable:{error}"))?;
        if let Some(previous) = &previous
            && previous.client_id != client.client_id
        {
            return Err(
                "Disconnect the previous Google authorization before using the new client.".into(),
            );
        }
        if enable_avatar {
            let grant = previous
                .as_mut()
                .ok_or("Unavailable:Connect a Google account first.")?;
            if grant.capabilities.profile {
                grant.avatar_sync = true;
                self.keyring.store(grant)?;
                state.avatar.enabled = true;
                state.error = None;
                self.notify();
                return Ok(Self::snapshot(&state, Ok(Some(client)), Ok(previous)));
            }
        }
        let requested = previous
            .as_ref()
            .map_or(requested, |grant| grant.capabilities.with(requested));
        let account = self.clone();
        let client_for_worker = client.clone();
        let old_identity = previous
            .as_ref()
            .map(|grant| (grant.subject.clone(), grant.capabilities, grant.avatar_sync));
        thread::Builder::new()
            .name("google-oauth".into())
            .spawn(move || {
                let result = flow::authorize(client_for_worker, requested);
                let mut tasks = account.tasks.lock().unwrap();
                let mut state = account.state.lock().unwrap();
                state.error = match result.and_then(|mut grant| {
                    if let Some((subject, capabilities, avatar_sync)) = old_identity {
                        if grant.subject != subject {
                            return Err("Select the connected Google account or disconnect it before switching accounts.".into());
                        }
                        if !grant.capabilities.includes(capabilities) {
                            return Err("Google did not grant all previously enabled permissions. The existing connection was kept.".into());
                        }
                        grant.avatar_sync = avatar_sync;
                    }
                    grant.avatar_sync |= enable_avatar && grant.capabilities.profile;
                    account.keyring.store(&grant)?;
                    state.avatar.enabled = grant.avatar_sync;
                    *tasks = None;
                    Ok(grant)
                }) {
                    Ok(grant) => (!grant.capabilities.includes(requested))
                        .then(|| "Some Google permissions were declined.".into()),
                    Err(error) => Some(error),
                };
                state.pending = false;
                account.notify();
            })
            .map_err(|_| "Could not start Google authorization.")?;
        state.pending = true;
        state.error = None;
        self.notify();
        Ok(Self::snapshot(&state, Ok(Some(client)), Ok(previous)))
    }

    pub fn disconnect(&self) -> Result<Status, String> {
        let mut tasks = self.tasks.try_lock().map_err(|_| tasks::BUSY.to_string())?;
        let mut state = self.state.lock().unwrap();
        if state.pending {
            return Err("Wait for Google authorization to finish before disconnecting.".into());
        }
        let result = (|| {
            if let Some(grant) = self.keyring.load()? {
                flow::revoke(&grant.refresh_token)?;
                self.keyring.clear()?;
                self.clear_avatar(&mut state, &grant.subject);
                *tasks = None;
            }
            Ok(())
        })();
        if let Err(error) = &result {
            state.error = Some(String::clone(error));
        } else {
            state.error = None;
        }
        self.notify();
        result.map(|_| Self::snapshot(&state, Credentials::load(&self.config), Ok(None)))
    }
    pub fn tasks_request(&self, raw: &str) -> Result<serde_json::Value, String> {
        let call = tasks::Call::parse(raw).map_err(|error| error.to_string())?;
        let account_id = call.account_id.clone();
        let request = call.prepare().map_err(|error| error.to_string())?;
        let mut tasks = self.tasks.try_lock().map_err(|_| tasks::BUSY.to_string())?;
        let Some(client) =
            Credentials::load(&self.config).map_err(|error| format!("Unavailable:{error}"))?
        else {
            return Err("Unavailable:Google OAuth client credentials are missing.".into());
        };
        let Some(grant) = self
            .keyring
            .load()
            .map_err(|error| format!("Unavailable:{error}"))?
            .filter(|grant| {
                grant.client_id == client.client_id
                    && grant.subject == account_id
                    && grant.capabilities.tasks
            })
        else {
            return Err(tasks::AUTH.to_string());
        };
        tasks::execute(
            &self.tasks_agent,
            &self.tasks_fresh_agent,
            &mut tasks,
            &client,
            &grant,
            &request,
        )
        .map_err(|error| error.to_string())
    }
}
