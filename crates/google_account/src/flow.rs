use super::{Capabilities, Grant};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::signature::{RSA_PKCS1_2048_8192_SHA256, RsaPublicKeyComponents};
use serde::{Deserialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::Path,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub(super) const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const JWKS_URL: &str = "https://www.googleapis.com/oauth2/v3/certs";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
const CALENDAR: &str = "https://www.googleapis.com/auth/calendar.events";
pub(super) const TASKS: &str = "https://www.googleapis.com/auth/tasks";

#[derive(Clone, Deserialize)]
pub(super) struct Credentials {
    pub(super) client_id: String,
    pub(super) client_secret: String,
}

impl Credentials {
    pub(super) fn load(path: &Path) -> Result<Option<Self>, String> {
        #[derive(Deserialize)]
        struct DesktopClient {
            installed: Credentials,
        }

        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("Cannot open the Google OAuth Desktop app credentials.".into()),
        };
        let invalid = "Set up Google OAuth with a downloaded Desktop app credentials JSON file.";
        let DesktopClient { installed } =
            serde_json::from_reader(file.take(16_385)).map_err(|_| invalid)?;
        if !installed.client_id.ends_with(".apps.googleusercontent.com")
            || !valid_credential(&installed.client_id)
            || !valid_credential(&installed.client_secret)
        {
            return Err(invalid.into());
        }
        Ok(Some(installed))
    }
}

fn valid_credential(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c))
}

fn random() -> Result<String, String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| "Could not generate OAuth security parameters.")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(super) fn google_https(http_status_as_error: bool) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(8)))
        .http_status_as_error(http_status_as_error)
        .build()
        .into()
}

fn response_json<T: DeserializeOwned>(
    mut response: ureq::http::Response<ureq::Body>,
) -> Result<T, String> {
    if !response.status().is_success() {
        return Err("Google returned an unexpected response.".into());
    }
    serde_json::from_reader(response.body_mut().with_config().limit(64 * 1024).reader())
        .map_err(|_| "Could not read the Google authorization response.".into())
}

#[derive(Deserialize)]
struct Tokens {
    refresh_token: Option<String>,
    scope: String,
    id_token: String,
}

pub(super) fn authorize(client: Credentials, requested: Capabilities) -> Result<Grant, String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .map_err(|_| "Could not listen for the Google sign-in response.")?;
    listener
        .set_nonblocking(true)
        .map_err(|_| "Could not configure the Google sign-in listener.")?;
    let address = listener
        .local_addr()
        .map_err(|_| "Could not determine the Google sign-in callback address.")?
        .to_string();
    let redirect = format!("http://{address}/");
    let verifier = random()?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random()?;
    let nonce = random()?;

    let mut scopes = vec!["openid", "email"];
    if requested.calendar {
        scopes.push(CALENDAR);
    }
    if requested.tasks {
        scopes.push(TASKS);
    }
    let query = form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("client_id", client.client_id.as_str()),
            ("redirect_uri", &redirect),
            ("response_type", "code"),
            ("scope", &scopes.join(" ")),
            ("access_type", "offline"),
            ("prompt", "consent select_account"),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", &state),
            ("nonce", &nonce),
        ])
        .finish();
    gio::AppInfo::launch_default_for_uri(
        &format!("{AUTH_URL}?{query}"),
        gio::AppLaunchContext::NONE,
    )
    .map_err(|_| "Could not open the system browser for Google sign-in.")?;

    let (code, mut browser) = wait_for_callback(listener, &address, &state)?;
    let result = (|| {
        let agent = google_https(true);
        let response = agent
            .post(TOKEN_URL)
            .send_form([
                ("client_id", client.client_id.as_str()),
                ("client_secret", &client.client_secret),
                ("redirect_uri", &redirect),
                ("code_verifier", &verifier),
                ("code", &code),
                ("grant_type", "authorization_code"),
            ])
            .map_err(|_| "Google did not accept the authorization code. Try connecting again.")?;
        let tokens: Tokens = response_json(response)?;
        let identity = validate_identity(&agent, &tokens.id_token, &client.client_id, &nonce)?;
        let refresh_token = tokens
            .refresh_token
            .filter(|token| !token.is_empty())
            .ok_or("Google did not provide a refresh token. Try connecting again.")?;
        Ok(Grant {
            client_id: client.client_id,
            subject: identity.sub,
            email: identity.email,
            capabilities: granted_capabilities(&tokens.scope, requested),
            refresh_token,
        })
    })();
    reply(
        &mut browser,
        "200 OK",
        "You can close this tab and check the result in Remya Apps.",
    );
    result
}

fn granted_capabilities(scope: &str, requested: Capabilities) -> Capabilities {
    Capabilities {
        calendar: requested.calendar && scope.split_ascii_whitespace().any(|s| s == CALENDAR),
        tasks: requested.tasks && scope.split_ascii_whitespace().any(|s| s == TASKS),
    }
}

#[derive(Debug, PartialEq)]
enum Callback {
    Code(String),
    Denied,
}

fn parse_callback(path: &str, expected_state: &str) -> Option<Callback> {
    let query = path.strip_prefix("/?")?;
    let (mut state, mut code, mut error) = (None, None, None);
    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        let field = match key.as_ref() {
            "state" => &mut state,
            "code" => &mut code,
            "error" => &mut error,
            _ => continue,
        };
        if field.replace(value.into_owned()).is_some() {
            return None;
        }
    }
    if state.as_deref() != Some(expected_state) {
        return None;
    }
    match (code, error) {
        (Some(code), None) if !code.is_empty() && code.len() <= 2048 => Some(Callback::Code(code)),
        (None, Some(_)) => Some(Callback::Denied),
        _ => None,
    }
}

fn wait_for_callback(
    listener: TcpListener,
    address: &str,
    state: &str,
) -> Result<(String, TcpStream), String> {
    let deadline = Instant::now() + Duration::from_secs(180);
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut browser, _)) => match read_callback(&mut browser, address, state) {
                Some(Callback::Code(code)) => return Ok((code, browser)),
                Some(Callback::Denied) => {
                    reply(
                        &mut browser,
                        "200 OK",
                        "Google authorization ended. You can return to Remya Apps.",
                    );
                    return Err("Google authorization was denied or could not be completed.".into());
                }
                None => reply(
                    &mut browser,
                    "400 Bad Request",
                    "Invalid Google sign-in callback.",
                ),
            },
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return Err("Could not receive the Google sign-in response.".into()),
        }
    }
    Err("Google authorization timed out. Try connecting again.".into())
}

fn read_callback(stream: &mut TcpStream, address: &str, state: &str) -> Option<Callback> {
    let mut bytes = [0; 8192];
    let mut used = 0;
    let deadline = Instant::now() + Duration::from_millis(500);
    while used < bytes.len() {
        stream
            .set_read_timeout(Some(deadline.checked_duration_since(Instant::now())?))
            .ok()?;
        let len = stream.read(&mut bytes[used..]).ok()?;
        if len == 0 {
            return None;
        }
        used += len;
        let mut headers = [httparse::EMPTY_HEADER; 32];
        let mut request = httparse::Request::new(&mut headers);
        if request.parse(&bytes[..used]).ok()?.is_complete() {
            let mut hosts = request
                .headers
                .iter()
                .filter(|header| header.name.eq_ignore_ascii_case("host"));
            if request.method != Some("GET")
                || hosts.next()?.value != address.as_bytes()
                || hosts.next().is_some()
            {
                return None;
            }
            return parse_callback(request.path?, state);
        }
    }
    None
}

fn reply(stream: &mut TcpStream, status: &str, message: &str) {
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{message}",
        message.len()
    );
}

#[derive(Deserialize)]
struct JwtHeader {
    alg: String,
    kid: String,
}

#[derive(Deserialize)]
struct JwtClaims {
    iss: String,
    aud: String,
    azp: Option<String>,
    nonce: String,
    exp: u64,
    iat: u64,
    sub: String,
    email: String,
}

#[derive(Deserialize)]
struct Jwks {
    keys: Vec<RsaKey>,
}

#[derive(Deserialize)]
struct RsaKey {
    kid: String,
    kty: String,
    alg: String,
    n: String,
    e: String,
}

impl JwtClaims {
    fn matches(&self, client_id: &str, nonce: &str, now: u64) -> bool {
        matches!(
            self.iss.as_str(),
            "https://accounts.google.com" | "accounts.google.com"
        ) && self.aud == client_id
            && self.azp.as_deref().is_none_or(|azp| azp == client_id)
            && self.nonce == nonce
            && self.exp > now
            && self.iat <= now.saturating_add(60)
            && !self.sub.is_empty()
            && !self.email.is_empty()
    }
}

fn validate_identity(
    agent: &ureq::Agent,
    id_token: &str,
    client_id: &str,
    nonce: &str,
) -> Result<JwtClaims, String> {
    let invalid = "Could not verify the Google account identity.";
    let (signed, signature) = id_token.rsplit_once('.').ok_or(invalid)?;
    let (header, claims) = signed.split_once('.').ok_or(invalid)?;
    let decode = |data| URL_SAFE_NO_PAD.decode(data).map_err(|_| invalid);
    let header: JwtHeader = serde_json::from_slice(&decode(header)?).map_err(|_| invalid)?;
    if header.alg != "RS256" {
        return Err(invalid.into());
    }
    let response = agent
        .get(JWKS_URL)
        .call()
        .map_err(|_| "Could not download Google's identity verification keys.")?;
    let keys: Jwks = response_json(response)?;
    let key = keys
        .keys
        .iter()
        .find(|key| key.kid == header.kid && key.kty == "RSA" && key.alg == header.alg)
        .ok_or(invalid)?;
    RsaPublicKeyComponents {
        n: decode(&key.n)?,
        e: decode(&key.e)?,
    }
    .verify(
        &RSA_PKCS1_2048_8192_SHA256,
        signed.as_bytes(),
        &decode(signature)?,
    )
    .map_err(|_| invalid)?;
    let claims: JwtClaims = serde_json::from_slice(&decode(claims)?).map_err(|_| invalid)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid)?
        .as_secs();
    claims
        .matches(client_id, nonce, now)
        .then_some(claims)
        .ok_or_else(|| invalid.into())
}

pub(super) fn revoke(token: &str) -> Result<(), String> {
    #[derive(Deserialize)]
    struct RevokeError {
        error: String,
    }
    let agent = google_https(true);
    if let Ok(mut response) = agent
        .post(REVOKE_URL)
        .config()
        .http_status_as_error(false)
        .build()
        .send_form([("token", token)])
    {
        if response.status().is_success() {
            return Ok(());
        }
        let body = response.body_mut().with_config().limit(8192).reader();
        if serde_json::from_reader::<_, RevokeError>(body)
            .is_ok_and(|result| result.error == "invalid_token")
        {
            return Ok(());
        }
    }
    Err("Google authorization could not be revoked. Try disconnecting again.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_requires_exact_path_state_and_unambiguous_result() {
        assert_eq!(
            parse_callback("/?code=a%2Bb&state=session", "session"),
            Some(Callback::Code("a+b".into()))
        );
        assert_eq!(
            parse_callback("/?error=access_denied&state=session", "session"),
            Some(Callback::Denied)
        );
        for request in [
            "/?code=a",
            "/?code=a&state=other",
            "/?code=a&state=session&state=other",
            "/?code=a&state=session&code=b",
            "/?code=a&state=session&error=access_denied",
            "/other?code=a&state=session",
        ] {
            assert_eq!(parse_callback(request, "session"), None);
        }
    }

    #[test]
    fn incomplete_scope_grant_does_not_infer_write_access() {
        let requested = Capabilities {
            calendar: true,
            tasks: true,
        };
        let granted =
            granted_capabilities(&format!("openid {CALENDAR} {TASKS}.readonly"), requested);
        assert!(granted.calendar);
        assert!(!granted.tasks);
        assert!(!granted.includes(requested));
        assert!(!granted_capabilities(TASKS, Capabilities::default()).tasks);
    }

    #[test]
    fn identity_requires_expected_issuer_audience_nonce_and_current_claims() {
        let mut claims = JwtClaims {
            iss: "https://accounts.google.com".into(),
            aud: "client".into(),
            azp: None,
            nonce: "request".into(),
            exp: 150,
            iat: 99,
            sub: "subject".into(),
            email: "user@example.com".into(),
        };
        assert!(claims.matches("client", "request", 100));
        assert!(!claims.matches("different client", "request", 100));
        assert!(!claims.matches("client", "different request", 100));
        assert!(!claims.matches("client", "request", 150));
        claims.azp = Some("other client".into());
        assert!(!claims.matches("client", "request", 100));
        claims.azp = None;
        claims.iss = "https://example.com".into();
        assert!(!claims.matches("client", "request", 100));
    }
}
