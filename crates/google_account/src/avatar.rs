use super::{Account, Capabilities, Grant, State, Status, flow, tasks};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

const USER_INFO: &str = "https://openidconnect.googleapis.com/v1/userinfo";
const MAX_PHOTO_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AvatarSnapshot {
    pub enabled: bool,
    pub path: Option<PathBuf>,
    pub error: Option<String>,
}

impl Account {
    pub fn set_avatar_sync(self: &Arc<Self>, enabled: bool) -> Result<Status, String> {
        if enabled {
            return self.authorize(
                Capabilities {
                    profile: true,
                    ..Capabilities::default()
                },
                true,
            );
        }
        let mut state = self.state.lock().unwrap();
        if state.pending {
            return Err("Wait for Google authorization to finish.".into());
        }
        let mut grant = self.keyring.load()?;
        if let Some(grant) = grant.as_mut() {
            grant.avatar_sync = false;
            self.keyring.store(grant)?;
            self.clear_avatar(&mut state, &grant.subject);
        }
        self.notify();
        Ok(Self::snapshot(
            &state,
            flow::Credentials::load(&self.config),
            Ok(grant),
        ))
    }

    pub fn avatar(&self) -> AvatarSnapshot {
        self.state.lock().unwrap().avatar.clone()
    }

    pub(super) fn clear_avatar(&self, state: &mut State, subject: &str) {
        state.avatar = AvatarSnapshot::default();
        let _ = fs::remove_file(self.avatar_path(subject));
    }

    fn avatar_path(&self, subject: &str) -> PathBuf {
        let id = URL_SAFE_NO_PAD.encode(Sha256::digest(subject.as_bytes()));
        self.avatar_cache.join(format!("google-avatar-{id}.png"))
    }

    fn selected_grant(&self) -> Result<Option<(flow::Credentials, Grant)>, String> {
        let Some(client) = flow::Credentials::load(&self.config)? else {
            return Ok(None);
        };
        Ok(self
            .keyring
            .load()?
            .filter(|grant| {
                grant.client_id == client.client_id
                    && grant.capabilities.profile
                    && grant.avatar_sync
            })
            .map(|grant| (client, grant)))
    }

    pub fn refresh_avatar(
        &self,
        normalize: fn(&[u8]) -> Result<Vec<u8>, String>,
        write: fn(&Path, &[u8]) -> Result<(), String>,
    ) -> bool {
        let (client, grant) = {
            let mut state = self.state.lock().unwrap();
            match self.selected_grant() {
                Ok(Some(selection)) => selection,
                Ok(None) => return update(&mut state, AvatarSnapshot::default()),
                Err(error) => return update_avatar_error(&mut state, error),
            }
        };
        let photo = fetch(&client, &grant)
            .and_then(|photo| photo.map(|bytes| normalize(&bytes)).transpose());
        let mut state = self.state.lock().unwrap();
        match self.selected_grant() {
            Ok(Some((_, current))) if current.subject == grant.subject => {}
            Ok(_) => return false,
            Err(error) => return update_avatar_error(&mut state, error),
        }
        let path = self.avatar_path(&grant.subject);
        let saved = fs::symlink_metadata(&path)
            .ok()
            .filter(|metadata| metadata.is_file());
        let mut changed = false;
        let result = match photo {
            Ok(Some(photo)) if !path.is_symlink() => {
                if !saved
                    .as_ref()
                    .is_some_and(|metadata| metadata.len() == photo.len() as u64)
                    || fs::read(&path).ok().as_ref() != Some(&photo)
                {
                    write(&path, &photo).map(|()| changed = true)
                } else {
                    Ok(())
                }
            }
            Ok(Some(_)) => Err("Google avatar cache path is a symbolic link.".into()),
            Ok(None) if saved.is_some() => fs::remove_file(&path)
                .map(|()| changed = true)
                .map_err(|error| error.to_string()),
            Ok(None) => Ok(()),
            Err(error) => Err(error),
        };
        let snapshot = AvatarSnapshot {
            enabled: true,
            path: (!path.is_symlink() && path.is_file()).then_some(path),
            error: result.err(),
        };
        update(&mut state, snapshot) || changed
    }
}

fn update(state: &mut State, snapshot: AvatarSnapshot) -> bool {
    if state.avatar == snapshot {
        return false;
    }
    state.avatar = snapshot;
    true
}

fn update_avatar_error(state: &mut State, error: String) -> bool {
    if !state.avatar.enabled || state.avatar.error.as_ref() == Some(&error) {
        return false;
    }
    state.avatar.error = Some(error);
    true
}

fn photo_uri(value: &str) -> Result<ureq::http::Uri, String> {
    let error = "Google returned an unsupported profile picture URL.";
    let uri: ureq::http::Uri = value.parse().map_err(|_| error)?;
    match uri.authority().map(|authority| authority.as_str()) {
        Some(
            "lh3.googleusercontent.com"
            | "lh4.googleusercontent.com"
            | "lh5.googleusercontent.com"
            | "lh6.googleusercontent.com",
        ) if uri.scheme_str() == Some("https") => Ok(uri),
        _ => Err(error.into()),
    }
}

fn fetch(client: &flow::Credentials, grant: &Grant) -> Result<Option<Vec<u8>>, String> {
    #[derive(Deserialize)]
    struct Profile {
        sub: String,
        picture: Option<String>,
    }
    let agent = flow::google_https_fresh(true);
    let token = tasks::refresh(
        &flow::google_https_fresh(false),
        client,
        grant,
        [0; 32],
        flow::profile_granted,
    )
    .map_err(|error| {
        if error == tasks::AUTH {
            "Reconnect your Google account to refresh your profile picture."
        } else {
            "Could not reach Google to refresh your profile picture."
        }
    })?;
    let profile: Profile = flow::response_json(
        agent
            .get(USER_INFO)
            .header("Authorization", format!("Bearer {}", token.value))
            .call()
            .map_err(|_| "Could not read your Google profile.")?,
    )?;
    if profile.sub != grant.subject {
        return Err("Google returned a profile for a different account.".into());
    }
    let Some(picture) = profile.picture else {
        return Ok(None);
    };
    let mut response = agent
        .get(photo_uri(&picture)?)
        .call()
        .map_err(|_| "Could not download your Google profile picture.")?;
    if !response.status().is_success() {
        return Err("Google returned an unexpected profile picture response.".into());
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_PHOTO_BYTES)
        .read_to_vec()
        .map_err(|_| "Could not read your Google profile picture.")?;
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_direct_https_google_photo_urls() {
        assert!(photo_uri("https://lh3.googleusercontent.com/example=s96-c").is_ok());
        for url in [
            "http://lh3.googleusercontent.com/example",
            "https://lh3.googleusercontent.com:444/example",
            "https://other@lh3.googleusercontent.com/example",
            "https://lh3.googleusercontent.com.example.net/example",
            "https://127.0.0.1/example",
        ] {
            assert!(photo_uri(url).is_err(), "{url}");
        }
    }

    #[test]
    fn existing_grants_start_with_avatar_sync_off() {
        let grant: Grant = serde_json::from_value(serde_json::json!({
            "client_id":"client", "subject":"sub", "email":"user@example.com",
            "capabilities":{"calendar":false,"tasks":true}, "refresh_token":"token"
        }))
        .unwrap();
        assert!(grant.capabilities.tasks);
        assert!(!grant.capabilities.profile);
        assert!(!grant.avatar_sync);
    }
}
