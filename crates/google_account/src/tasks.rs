//! Account-bound calls to the Google Tasks v1 API.
use crate::{
    Grant,
    flow::{Credentials, TASKS, TOKEN_URL},
};
use chrono::{DateTime, NaiveDate};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    time::{Duration, Instant},
};
use ureq::{Agent, http::Request};

const API: &str = "https://tasks.googleapis.com/tasks/v1";
const REQUEST_LIMIT: usize = 64 * 1024;
const RESPONSE_LIMIT: u64 = 8 * 1024 * 1024;

pub(super) const AUTH: Failure = Failure(
    "TasksAuth",
    "Reconnect the same Google account to sync your tasks.",
);
const INVALID: Failure = Failure("InvalidArgument", "Invalid Google Tasks request.");
const OFFLINE: Failure = Failure("TasksOffline", "Could not reach Google Tasks.");
const RETRY: Failure = Failure("TasksRetry", "Google Tasks is busy. Try again later.");
const MISSING: Failure = Failure("TasksMissing", "This Google task or list no longer exists.");
const DENIED: Failure = Failure("TasksDenied", "Google Tasks did not allow this operation.");
const AMBIGUOUS_CREATE: Failure = Failure(
    "TasksAmbiguous",
    "Google may have created this item. Check the list before adding it again.",
);
const AMBIGUOUS_CHANGE: Failure = Failure(
    "TasksAmbiguous",
    "Google may have applied this change. Check the list before trying again.",
);
const ERROR: Failure = Failure("OperationFailed", "Unexpected response from Google Tasks.");
pub(super) const BUSY: Failure =
    Failure("TasksRetry", "Another Google Tasks request is in progress.");

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Failure(&'static str, &'static str);

impl Failure {
    pub(super) fn code(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.0, self.1)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Call {
    pub(super) account_id: String,
    operation: String,
    #[serde(default)]
    args: Map<String, Value>,
}

impl Call {
    pub(super) fn parse(raw: &str) -> Result<Self, Failure> {
        if raw.len() > REQUEST_LIMIT || !raw.trim_start().starts_with('{') {
            return Err(INVALID);
        }
        let call: Self = serde_json::from_str(raw).map_err(|_| INVALID)?;
        if !valid_id(&call.account_id) {
            return Err(INVALID);
        }
        Ok(call)
    }

    pub(super) fn prepare(self) -> Result<TaskRequest, Failure> {
        let mut parameters = self.args;
        let (resource, action) = self.operation.split_once('.').ok_or(INVALID)?;
        let lists = match resource {
            "lists" => true,
            "tasks" => false,
            _ => return Err(INVALID),
        };
        let method = match action {
            "get" | "list" => "GET",
            "insert" => "POST",
            "move" | "clear" if !lists => "POST",
            "patch" => "PATCH",
            "delete" => "DELETE",
            _ => return Err(INVALID),
        };
        let mut path = if lists {
            format!("{API}/users/@me/lists")
        } else {
            let list_id = required_id(&mut parameters, "listId")?;
            let suffix = if action == "clear" { "clear" } else { "tasks" };
            format!("{API}/lists/{list_id}/{suffix}")
        };
        match action {
            "get" | "patch" | "delete" | "move" => {
                let id = required_id(&mut parameters, if lists { "listId" } else { "taskId" })?;
                path.push('/');
                path.push_str(&id);
                if action == "move" {
                    path.push_str("/move");
                }
            }
            _ => {}
        }
        let mut query = form_urlencoded::Serializer::new(String::new());
        if action == "list" {
            query.append_pair("maxResults", if lists { "1000" } else { "100" });
            if let Some(page) = optional_text(&mut parameters, "pageToken", 4096)? {
                query.append_pair("pageToken", &page);
            }
            if !lists {
                for key in ["showCompleted", "showHidden", "showDeleted", "showAssigned"] {
                    let enabled = match parameters.remove(key) {
                        Some(Value::Bool(value)) => value,
                        None => true,
                        _ => return Err(INVALID),
                    };
                    query.append_pair(key, if enabled { "true" } else { "false" });
                }
                if let Some(updated) = optional_text(&mut parameters, "updatedMin", 64)? {
                    DateTime::parse_from_rfc3339(&updated).map_err(|_| INVALID)?;
                    query.append_pair("updatedMin", &updated);
                }
            }
        }
        if !lists && matches!(action, "insert" | "move") {
            for key in ["parent", "previous", "destinationTasklist"] {
                if key == "destinationTasklist" && action != "move" {
                    continue;
                }
                if let Some(id) = optional_text(&mut parameters, key, 512)? {
                    if !valid_id(&id) {
                        return Err(INVALID);
                    }
                    query.append_pair(key, &id);
                }
            }
        }
        let body = if matches!(action, "insert" | "patch") {
            let Some(Value::Object(body)) = parameters.remove("body") else {
                return Err(INVALID);
            };
            task_body(body, lists, action == "insert")?.to_string()
        } else {
            String::new()
        };
        if !parameters.is_empty() {
            return Err(INVALID);
        }
        let query = query.finish();
        if !query.is_empty() {
            path.push('?');
            path.push_str(&query);
        }
        Ok(TaskRequest {
            operation: format!("{resource}.{action}"),
            method,
            url: path,
            body,
            creates: action == "insert",
            empty_response: matches!(action, "delete" | "clear"),
            uncertain: match action {
                "insert" => Some(AMBIGUOUS_CREATE),
                "move" | "clear" => Some(AMBIGUOUS_CHANGE),
                _ => None,
            },
        })
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"@_=-+.".contains(&b))
        && value != "."
        && value != ".."
}

fn optional_text(
    args: &mut Map<String, Value>,
    key: &str,
    max: usize,
) -> Result<Option<String>, Failure> {
    match args.remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.is_empty() && value.len() <= max => Ok(Some(value)),
        _ => Err(INVALID),
    }
}

fn required_id(args: &mut Map<String, Value>, key: &str) -> Result<String, Failure> {
    let id = optional_text(args, key, 512)?
        .filter(|id| valid_id(id))
        .ok_or(INVALID)?;
    Ok(if id == "@default" {
        id
    } else {
        utf8_percent_encode(&id, NON_ALPHANUMERIC).to_string()
    })
}

fn task_body(mut body: Map<String, Value>, lists: bool, insert: bool) -> Result<Value, Failure> {
    if body.is_empty() || (insert && !body.contains_key("title")) {
        return Err(INVALID);
    }
    for (key, value) in &mut body {
        let valid = match key.as_str() {
            "title" => value
                .as_str()
                .is_some_and(|title| !title.trim().is_empty() && title.chars().count() <= 1024),
            "notes" if !lists => {
                value.is_null()
                    || value
                        .as_str()
                        .is_some_and(|notes| notes.chars().count() <= 8192)
            }
            "status" if !lists => matches!(value.as_str(), Some("needsAction" | "completed")),
            "due" if !lists && value.is_null() => true,
            "due" if !lists => {
                let raw = value.as_str().ok_or(INVALID)?;
                let day = NaiveDate::parse_from_str(raw, "%Y-%m-%d")
                    .or_else(|_| DateTime::parse_from_rfc3339(raw).map(|time| time.date_naive()))
                    .map_err(|_| INVALID)?;
                if raw.get(..10) != Some(day.format("%Y-%m-%d").to_string().as_str()) {
                    return Err(INVALID);
                }
                *value = format!("{day}T00:00:00Z").into();
                true
            }
            _ => false,
        };
        if !valid {
            return Err(INVALID);
        }
    }
    Ok(Value::Object(body))
}

pub(super) struct TaskRequest {
    operation: String,
    method: &'static str,
    url: String,
    body: String,
    creates: bool,
    empty_response: bool,
    uncertain: Option<Failure>,
}

fn describe_transport_error(error: &ureq::Error) -> String {
    match error {
        ureq::Error::Io(io) => format!("io:{:?}", io.kind()),
        ureq::Error::Timeout(timeout) => format!("timeout:{timeout:?}"),
        ureq::Error::Protocol(_) => "protocol".into(),
        ureq::Error::HostNotFound => "host_not_found".into(),
        ureq::Error::ConnectionFailed => "connection_failed".into(),
        ureq::Error::Tls(detail) => format!("tls:{detail}"),
        ureq::Error::Rustls(detail) => format!("rustls:{detail}"),
        ureq::Error::ConnectProxyFailed(_) => "connect_proxy_failed".into(),
        other => format!("other:{other:?}"),
    }
}

fn is_stale_pool_error(error: &ureq::Error) -> bool {
    match error {
        ureq::Error::ConnectionFailed => true,
        ureq::Error::Io(io) => matches!(
            io.kind(),
            std::io::ErrorKind::UnexpectedEof
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::BrokenPipe
        ),
        _ => false,
    }
}

fn classify_transport_error(error: &ureq::Error, uncertain: Option<Failure>) -> Failure {
    match error {
        ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed
        | ureq::Error::Tls(_)
        | ureq::Error::Timeout(ureq::Timeout::Resolve | ureq::Timeout::Connect) => OFFLINE,
        ureq::Error::Io(io)
            if matches!(
                io.kind(),
                std::io::ErrorKind::NetworkUnreachable
                    | std::io::ErrorKind::HostUnreachable
                    | std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::AddrNotAvailable
            ) =>
        {
            OFFLINE
        }
        _ => uncertain.unwrap_or(OFFLINE),
    }
}

impl TaskRequest {
    fn run_http(
        &self,
        agent: &Agent,
        token: &str,
        close: bool,
    ) -> Result<Result<ureq::http::Response<ureq::Body>, ureq::Error>, Failure> {
        let mut request = Request::builder()
            .method(self.method)
            .uri(&self.url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json");
        if close {
            request = request.header("Connection", "close");
        }
        if !self.body.is_empty() {
            request = request.header("Content-Type", "application/json");
        }
        if self.body.is_empty() && matches!(self.method, "GET" | "DELETE") {
            let request = request.body(()).map_err(|_| ERROR)?;
            Ok(agent.run(request))
        } else {
            let request = request.body(self.body.as_str()).map_err(|_| ERROR)?;
            Ok(agent.run(request))
        }
    }

    fn send_once(
        &self,
        agent: &Agent,
        token: &str,
        close: bool,
    ) -> Result<ureq::http::Response<ureq::Body>, Failure> {
        self.run_http(agent, token, close)?.map_err(|error| {
            eprintln!(
                "google_account tasks {} {} transport failed: {}",
                self.method,
                self.operation,
                describe_transport_error(&error)
            );
            classify_transport_error(&error, self.uncertain)
        })
    }

    fn send(&self, pooled: &Agent, fresh: &Agent, token: &str) -> Result<Value, Failure> {
        let mut response = if self.uncertain.is_some() {
            self.send_once(fresh, token, true)?
        } else {
            match self.run_http(pooled, token, false)? {
                Ok(response) => response,
                Err(ref error) if is_stale_pool_error(error) => {
                    eprintln!(
                        "google_account tasks {} {} pooled transport failed ({}), retrying fresh",
                        self.method,
                        self.operation,
                        describe_transport_error(error)
                    );
                    self.send_once(fresh, token, true)?
                }
                Err(error) => {
                    eprintln!(
                        "google_account tasks {} {} transport failed: {}",
                        self.method,
                        self.operation,
                        describe_transport_error(&error)
                    );
                    return Err(classify_transport_error(&error, self.uncertain));
                }
            }
        };
        let status = response.status().as_u16();
        let body = response.body_mut();
        match status {
            200..=299 if self.empty_response => Ok(serde_json::json!({})),
            200..=299 => {
                let value = serde_json::from_reader::<_, Value>(
                    body.with_config().limit(RESPONSE_LIMIT).reader(),
                )
                .map_err(|error| {
                    eprintln!(
                        "google_account tasks {} {} body decode failed (io={})",
                        self.method,
                        self.operation,
                        error.is_io()
                    );
                    self.uncertain
                        .unwrap_or(if error.is_io() { RETRY } else { ERROR })
                })?;
                if self.creates && value["id"].as_str().is_none_or(str::is_empty) {
                    eprintln!(
                        "google_account tasks {} {} missing created id",
                        self.method, self.operation
                    );
                    return Err(AMBIGUOUS_CREATE);
                }
                value.is_object().then_some(value).ok_or(ERROR)
            }
            401 => {
                eprintln!(
                    "google_account tasks {} {} http 401",
                    self.method, self.operation
                );
                Err(AUTH)
            }
            403 => {
                let error = serde_json::from_reader::<_, Value>(
                    body.with_config().limit(16 * 1024).reader(),
                )
                .unwrap_or(Value::Null);
                let failure = permission(&error);
                eprintln!(
                    "google_account tasks {} {} http 403 -> {}",
                    self.method, self.operation, failure.0
                );
                Err(failure)
            }
            404 | 410 => {
                eprintln!(
                    "google_account tasks {} {} http {status}",
                    self.method, self.operation
                );
                Err(MISSING)
            }
            400 | 405 | 422 => {
                eprintln!(
                    "google_account tasks {} {} http {status}",
                    self.method, self.operation
                );
                Err(DENIED)
            }
            409 | 429 => {
                eprintln!(
                    "google_account tasks {} {} http {status}",
                    self.method, self.operation
                );
                Err(RETRY)
            }
            408 | 500..=599 => {
                eprintln!(
                    "google_account tasks {} {} http {status}",
                    self.method, self.operation
                );
                Err(self.uncertain.unwrap_or(RETRY))
            }
            _ => {
                eprintln!(
                    "google_account tasks {} {} unexpected http {status}",
                    self.method, self.operation
                );
                Err(self.uncertain.unwrap_or(ERROR))
            }
        }
    }
}

fn permission(value: &Value) -> Failure {
    for entries in [&value["error"]["errors"], &value["error"]["details"]] {
        if let Some(items) = entries.as_array() {
            for item in items {
                match item["reason"].as_str() {
                    Some("accessNotConfigured" | "SERVICE_DISABLED") => {
                        return Failure(
                            "Unavailable",
                            "Google Tasks API is disabled for this OAuth client.",
                        );
                    }
                    Some("insufficientPermissions" | "ACCESS_TOKEN_SCOPE_INSUFFICIENT") => {
                        return AUTH;
                    }
                    Some("rateLimitExceeded" | "userRateLimitExceeded" | "RATE_LIMIT_EXCEEDED") => {
                        return RETRY;
                    }
                    _ => {}
                }
            }
        }
    }
    DENIED
}

pub(super) struct AccessToken {
    fingerprint: [u8; 32],
    pub(super) value: String,
    expires: Instant,
}

pub(super) fn refresh(
    agent: &Agent,
    client: &Credentials,
    grant: &Grant,
    fingerprint: [u8; 32],
    required_scope: fn(&str) -> bool,
) -> Result<AccessToken, Failure> {
    #[derive(Deserialize)]
    struct Token {
        access_token: String,
        token_type: String,
        expires_in: u64,
        scope: Option<String>,
    }
    let mut response = agent
        .post(TOKEN_URL)
        .header("Connection", "close")
        .send_form([
            ("client_id", client.client_id.as_str()),
            ("client_secret", client.client_secret.as_str()),
            ("refresh_token", grant.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .map_err(|error| {
            eprintln!(
                "google_account token refresh transport failed: {}",
                describe_transport_error(&error)
            );
            OFFLINE
        })?;
    let code = response.status().as_u16();
    let reader = response.body_mut().with_config().limit(64 * 1024).reader();
    if code == 400 || code == 401 {
        let error = serde_json::from_reader::<_, Value>(reader).unwrap_or(Value::Null);
        eprintln!("google_account token refresh http {code}");
        return Err(match error["error"].as_str() {
            Some("invalid_grant" | "invalid_scope" | "unauthorized_client" | "invalid_client") => {
                AUTH
            }
            _ => ERROR,
        });
    }
    if code == 429 || code >= 500 {
        eprintln!("google_account token refresh http {code}");
        return Err(RETRY);
    }
    if code != 200 {
        eprintln!("google_account token refresh http {code}");
        return Err(ERROR);
    }
    let token: Token = serde_json::from_reader(reader).map_err(|_| ERROR)?;
    if !token.token_type.eq_ignore_ascii_case("Bearer")
        || token.access_token.is_empty()
        || token.access_token.len() > 8192
        || !token.access_token.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(ERROR);
    }
    if token.scope.is_some_and(|scope| !required_scope(&scope)) {
        return Err(AUTH);
    }
    Ok(AccessToken {
        fingerprint,
        value: token.access_token,
        expires: Instant::now()
            + Duration::from_secs(token.expires_in.min(3600).saturating_sub(30)),
    })
}

pub(super) fn access_token<'a>(
    agent: &Agent,
    cache: &'a mut Option<AccessToken>,
    client: &Credentials,
    grant: &Grant,
    accepts: fn(&str) -> bool,
) -> Result<&'a str, Failure> {
    let fingerprint = Sha256::digest(grant.refresh_token.as_bytes()).into();
    let cached = cache
        .as_ref()
        .is_some_and(|token| token.fingerprint == fingerprint && token.expires > Instant::now());
    if !cached {
        *cache = None;
        *cache = Some(refresh(agent, client, grant, fingerprint, accepts)?);
    }
    Ok(&cache.as_ref().unwrap().value)
}

pub(super) fn execute(
    pooled: &Agent,
    fresh: &Agent,
    cache: &mut Option<AccessToken>,
    client: &Credentials,
    grant: &Grant,
    request: &TaskRequest,
) -> Result<Value, Failure> {
    let token = access_token(fresh, cache, client, grant, |scope| {
        scope.split_ascii_whitespace().any(|s| s == TASKS)
    })?;
    let result = request.send(pooled, fresh, token);
    // Authentication failure from a cached token invalidates it before the next request.
    // Avoid replaying a write while the client may already be nearing its D-Bus timeout.
    if result == Err(AUTH) {
        *cache = None;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(operation: &str, args: Value) -> Result<TaskRequest, Failure> {
        Call::parse(&json!({"accountId":"123","operation":operation,"args":args}).to_string())?
            .prepare()
    }

    #[test]
    fn pages_and_moves_use_the_documented_endpoints() {
        let request_list = request(
            "tasks.list",
            json!({"listId":"@default","pageToken":"a+b", "updatedMin":"2026-10-01T00:00:00.000Z"}),
        )
        .unwrap();
        assert_eq!(request_list.method, "GET");
        assert!(
            request_list
                .url
                .starts_with(&format!("{API}/lists/@default/tasks?maxResults=100&"))
        );
        assert!(request_list.url.contains("pageToken=a%2Bb"));
        for field in ["showCompleted", "showHidden", "showDeleted", "showAssigned"] {
            assert!(request_list.url.contains(&format!("{field}=true")));
        }

        let moved = request(
            "tasks.move",
            json!({"listId":"list","taskId":"task","destinationTasklist":"other","previous":"sibling"}),
        )
        .unwrap();
        assert_eq!(moved.method, "POST");
        assert_eq!(
            moved.url,
            format!("{API}/lists/list/tasks/task/move?previous=sibling&destinationTasklist=other")
        );
        assert!(moved.body.is_empty());

        let cleared = request("tasks.clear", json!({"listId":"list"})).unwrap();
        assert_eq!(cleared.method, "POST");
        assert_eq!(cleared.url, format!("{API}/lists/list/clear"));
        assert!(cleared.empty_response);
    }

    #[test]
    fn updates_preserve_calendar_day_and_reject_unknown_writes() {
        let updated = request(
            "tasks.patch",
            json!({"listId":"list","taskId":"task","body":{"due":"2026-01-01T23:30:00-08:00"}}),
        )
        .unwrap();
        assert_eq!(updated.body, r#"{"due":"2026-01-01T00:00:00Z"}"#);
        assert!(
            request(
                "tasks.patch",
                json!({"listId":"a","taskId":"b","body":{"due":null}})
            )
            .is_ok()
        );
        assert!(
            request(
                "tasks.patch",
                json!({"listId":"a","taskId":"b","body":{"title":" ","hidden":true}})
            )
            .is_err()
        );
        assert!(request("tasks.get", json!({"listId":"a","taskId":"../../users"})).is_err());
        assert!(request("lists.list", json!({"url":"https://example.com"})).is_err());
    }

    #[test]
    fn permission_errors_distinguish_reconnect_and_quota() {
        assert_eq!(
            permission(
                &json!({"error":{"details":[{"reason":"ACCESS_TOKEN_SCOPE_INSUFFICIENT"}]}})
            ),
            AUTH
        );
        assert_eq!(
            permission(&json!({"error":{"errors":[{"reason":"rateLimitExceeded"}]}})),
            RETRY
        );
        assert_eq!(permission(&json!({})), DENIED);
    }

    #[test]
    fn pre_send_transport_errors_are_offline_and_pool_errors_are_detected() {
        for err in [
            ureq::Error::HostNotFound,
            ureq::Error::ConnectionFailed,
            ureq::Error::Tls("handshake"),
            ureq::Error::Timeout(ureq::Timeout::Resolve),
            ureq::Error::Timeout(ureq::Timeout::Connect),
            ureq::Error::Io(std::io::Error::from(std::io::ErrorKind::AddrNotAvailable)),
        ] {
            assert_eq!(
                classify_transport_error(&err, Some(AMBIGUOUS_CREATE)),
                OFFLINE
            );
        }

        assert_eq!(
            classify_transport_error(
                &ureq::Error::Timeout(ureq::Timeout::Global),
                Some(AMBIGUOUS_CREATE)
            ),
            AMBIGUOUS_CREATE
        );
        assert_eq!(
            classify_transport_error(
                &ureq::Error::Io(std::io::Error::from(std::io::ErrorKind::UnexpectedEof)),
                Some(AMBIGUOUS_CREATE),
            ),
            AMBIGUOUS_CREATE
        );

        assert!(is_stale_pool_error(&ureq::Error::ConnectionFailed));
        for kind in [
            std::io::ErrorKind::UnexpectedEof,
            std::io::ErrorKind::ConnectionReset,
            std::io::ErrorKind::BrokenPipe,
        ] {
            assert!(is_stale_pool_error(&ureq::Error::Io(std::io::Error::from(
                kind
            ))));
        }
        assert!(!is_stale_pool_error(&ureq::Error::HostNotFound));
    }
}
