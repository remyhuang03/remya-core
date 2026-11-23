//! Read-only snapshots of the connected Google account's calendars.
use super::{Account, Grant, flow, tasks};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, NaiveDate, NaiveDateTime, SecondsFormat, TimeDelta, Utc};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, SystemTime},
};

const API: &str = "https://www.googleapis.com/calendar/v3";
const MAX_CALENDARS: usize = 1000;
const MAX_EVENT_PAGES: usize = 20;
const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_CACHE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CALENDAR_BYTES: usize = 2 * 1024 * 1024;
const MAX_EVENT_BYTES: usize = 12 * 1024 * 1024;
const LIST_FIELDS: &str =
    "nextPageToken,items(id,summary,summaryOverride,backgroundColor,selected,primary)";
const EVENT_FIELDS: &str = concat!(
    "nextPageToken,nextSyncToken,items(id,status,summary,start(date,dateTime,timeZone),",
    "end(date,dateTime,timeZone),colorId,location,description,htmlLink,hangoutLink,",
    "conferenceData(entryPoints(entryPointType,uri)),attendees(self,responseStatus))",
);
pub(super) const BUSY: &str = "CalendarRetry:Another Google Calendar request is in progress.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Failure {
    Auth,
    Offline,
    Retry,
    Invalid,
    Disabled,
    Expired,
    Removed,
}

impl Failure {
    fn status(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::Offline => "offline",
            _ => "retry",
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Auth => "CalendarAuth:Reconnect the same Google account to sync its calendars.",
            Self::Offline => "CalendarOffline:Could not reach Google Calendar.",
            Self::Invalid => "InvalidArgument:Invalid Google Calendar snapshot request.",
            Self::Disabled => "Unavailable:Google Calendar API is disabled for this OAuth client.",
            _ => "CalendarRetry:Could not refresh Google Calendar.",
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Call {
    account_id: String,
    time_min: String,
    time_max: String,
    calendar_ids: Option<BTreeSet<String>>,
    #[serde(default)]
    cache_only: bool,
}

impl Call {
    fn parse(raw: &str) -> Result<(Self, Window), Failure> {
        if raw.len() > 64 * 1024 || !raw.trim_start().starts_with('{') {
            return Err(Failure::Invalid);
        }
        let call: Self = serde_json::from_str(raw).map_err(|_| Failure::Invalid)?;
        if !valid_id(&call.account_id)
            || call
                .calendar_ids
                .as_ref()
                .is_some_and(|ids| ids.len() > MAX_CALENDARS || ids.iter().any(|id| !valid_id(id)))
        {
            return Err(Failure::Invalid);
        }
        let parse = |value| {
            DateTime::parse_from_rfc3339(value)
                .map(|time| time.to_utc())
                .map_err(|_| Failure::Invalid)
        };
        let window = Window {
            start: parse(&call.time_min)?,
            end: parse(&call.time_max)?,
        };
        if window.end <= window.start || window.end - window.start > TimeDelta::days(120) {
            return Err(Failure::Invalid);
        }
        Ok((call, window))
    }

    fn includes(&self, calendar: &Calendar) -> bool {
        self.calendar_ids
            .as_ref()
            .map_or(calendar.selected, |ids| ids.contains(&calendar.id))
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 1024 && value.bytes().all(|byte| byte.is_ascii_graphic())
}

#[derive(Clone, Copy, Deserialize, Serialize)]
struct Window {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
}

impl Window {
    fn covers(self, other: Self) -> bool {
        self.start <= other.start && self.end >= other.end
    }

    fn overlaps(self, event: &Value) -> bool {
        let start = &event["start"];
        let end = &event["end"];
        if let (Some(start), Some(end)) = (start["date"].as_str(), end["date"].as_str()) {
            let date = |text| NaiveDate::parse_from_str(text, "%Y-%m-%d");
            let (Ok(start), Ok(end)) = (date(start), date(end)) else {
                return false;
            };
            // Calendar dates have no UTC offset. Keep boundary dates for the local view.
            return start <= self.end.date_naive() && end >= self.start.date_naive();
        }
        let time = |value: &Value| DateTime::parse_from_rfc3339(value["dateTime"].as_str()?).ok();
        if let (Some(start), Some(end)) = (time(start), time(end)) {
            return start < self.end && (end > self.start || start == end && start == self.start);
        }
        // The API can return a local time with an IANA zone in a separate field.
        // Retain the boundary dates so the client can resolve daylight saving.
        let civil_date = |value: &Value| {
            let text = value["dateTime"].as_str()?;
            if let Ok(instant) = DateTime::parse_from_rfc3339(text) {
                return Some(instant.date_naive());
            }
            value["timeZone"].as_str()?;
            NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|time| time.date())
        };
        match (civil_date(start), civil_date(end)) {
            (Some(start), Some(end)) => {
                start <= self.end.date_naive() && end >= self.start.date_naive()
            }
            _ => false,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleCalendar {
    id: String,
    summary: Option<String>,
    summary_override: Option<String>,
    background_color: Option<String>,
    selected: Option<bool>,
    #[serde(default)]
    primary: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Calendar {
    id: String,
    summary: String,
    background_color: Option<String>,
    selected: bool,
}

impl From<GoogleCalendar> for Calendar {
    fn from(row: GoogleCalendar) -> Self {
        Self {
            summary: row
                .summary_override
                .or(row.summary)
                .filter(|title| !title.trim().is_empty() && title.len() <= 1024)
                .unwrap_or_else(|| row.id.clone()),
            id: row.id,
            background_color: row.background_color.filter(|color| valid_color(color)),
            selected: row.selected.unwrap_or(row.primary),
        }
    }
}

fn valid_color(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].bytes().all(|c| c.is_ascii_hexdigit())
}

#[derive(Deserialize, Serialize)]
struct Color {
    background: String,
}

#[derive(Default, Deserialize, Serialize)]
struct Colors {
    #[serde(default)]
    event: BTreeMap<String, Color>,
    fetched_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page<T> {
    items: Option<Vec<T>>,
    next_page_token: Option<String>,
    next_sync_token: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct EventWindow {
    window: Window,
    synced_at: DateTime<Utc>,
    sync_token: Option<String>,
    events: BTreeMap<String, Value>,
    payload_size: usize,
}

#[derive(Default, Deserialize, Serialize)]
struct Snapshot {
    calendars: Vec<Calendar>,
    colors: Colors,
    listed_at: Option<DateTime<Utc>>,
    slices: BTreeMap<String, EventWindow>,
}

impl Snapshot {
    fn reply(&self, call: &Call, window: Window, error: Option<Failure>) -> Value {
        let mut events = Vec::new();
        let mut synced_at = None;
        for calendar in self.calendars.iter().filter(|row| call.includes(row)) {
            let Some(slice) = self.slices.get(&calendar.id) else {
                continue;
            };
            synced_at = Some(synced_at.map_or(slice.synced_at, |other: DateTime<Utc>| {
                other.min(slice.synced_at)
            }));
            for event in slice.events.values().filter(|event| window.overlaps(event)) {
                let mut event = event.clone();
                let color = event["colorId"]
                    .as_str()
                    .and_then(|id| self.colors.event.get(id))
                    .map(|color| color.background.as_str())
                    .filter(|color| valid_color(color))
                    .or(calendar.background_color.as_deref());
                event["calendarId"] = json!(calendar.id);
                event["backgroundColor"] = json!(color);
                events.push(event);
            }
        }
        let mut value = json!({
            "calendars": self.calendars,
            "events": events,
            "syncedAt": synced_at.or(self.listed_at),
            "stale": error.is_some() || call.cache_only,
        });
        if let Some(error) = error {
            value["error"] = json!(error.status());
        }
        value
    }

    fn load(path: &Path) -> Option<Self> {
        let metadata = fs::symlink_metadata(path).ok()?;
        if !metadata.is_file()
            || metadata.len() > MAX_CACHE_BYTES
            || metadata.permissions().mode() & 0o077 != 0
        {
            return None;
        }
        let file = fs::File::open(path).ok()?;
        serde_json::from_reader(file.take(MAX_CACHE_BYTES + 1)).ok()
    }

    fn save(&self, path: &Path) -> Option<()> {
        let bytes = serde_json::to_vec(self).ok()?;
        if bytes.len() as u64 > MAX_CACHE_BYTES {
            return None;
        }
        let parent = path.parent()?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .ok()?;
        if !fs::symlink_metadata(parent).ok()?.is_dir() {
            return None;
        }
        let mut file = tempfile::NamedTempFile::new_in(parent).ok()?;
        file.write_all(&bytes).ok()?;
        file.as_file().sync_all().ok()?;
        file.persist(path).ok()?;
        Some(())
    }
}

fn cache_path(root: &Path, grant: &Grant) -> PathBuf {
    let digest = Sha256::new()
        .chain_update(grant.client_id.as_bytes())
        .chain_update(b"\0")
        .chain_update(grant.subject.as_bytes())
        .finalize();
    root.join("google-calendar")
        .join(format!("{}.json", URL_SAFE_NO_PAD.encode(digest)))
}

#[derive(Default)]
pub(super) struct State {
    token: Option<tasks::AccessToken>,
    cached: Option<(PathBuf, Snapshot)>,
}

impl State {
    pub(super) fn invalidate_token(&mut self) {
        self.token = None;
    }

    pub(super) fn clear(&mut self, root: &Path, grant: &Grant) {
        self.invalidate_token();
        self.cached = None;
        let _ = fs::remove_file(cache_path(root, grant));
    }
}

impl Account {
    pub fn calendar_request(&self, raw: &str) -> Result<Value, String> {
        let (call, window) = Call::parse(raw).map_err(|error| error.to_string())?;
        let mut state = self.calendar.try_lock().map_err(|_| BUSY.to_owned())?;
        let client = flow::Credentials::load(&self.config)
            .map_err(|error| format!("Unavailable:{error}"))?
            .ok_or("Unavailable:Google OAuth client credentials are missing.")?;
        let grant = self
            .keyring
            .load()
            .map_err(|error| format!("Unavailable:{error}"))?
            .filter(|grant| {
                grant.client_id == client.client_id
                    && grant.subject == call.account_id
                    && grant.capabilities.calendar
            })
            .ok_or_else(|| Failure::Auth.to_string())?;

        let path = cache_path(&self.cache, &grant);
        let State { token, cached } = &mut *state;
        if cached.as_ref().is_none_or(|(current, _)| *current != path) {
            *cached = Some((path.clone(), Snapshot::load(&path).unwrap_or_default()));
        }
        let (_, snapshot) = cached.as_mut().unwrap();
        if call.cache_only {
            return snapshot
                .listed_at
                .map(|_| snapshot.reply(&call, window, None))
                .ok_or_else(|| Failure::Retry.to_string());
        }

        let deadline = Instant::now() + Duration::from_secs(50);
        let previous_listed_at = snapshot.listed_at;
        let result = tasks::access_token(
            &self.tasks_fresh_agent,
            token,
            &client,
            &grant,
            flow::calendar_granted,
        )
        .map_err(|error| match error.code() {
            "TasksOffline" => Failure::Offline,
            "TasksAuth" => Failure::Auth,
            _ => Failure::Retry,
        })
        .and_then(|token| {
            let google = Google {
                agent: &self.tasks_agent,
                token,
                deadline,
            };
            snapshot.refresh(&google, &call, window)
        });
        if result == Err(Failure::Auth) {
            *token = None;
        }
        // Persist successful pages and revoked-calendar removals even after a partial refresh.
        if snapshot.listed_at != previous_listed_at && snapshot.save(&path).is_none() {
            eprintln!("google_account calendar: could not write the offline snapshot");
        }
        match result {
            Ok(()) => Ok(snapshot.reply(&call, window, None)),
            Err(error) if snapshot.listed_at.is_some() => {
                Ok(snapshot.reply(&call, window, Some(error)))
            }
            Err(error) => Err(error.to_string()),
        }
    }
}

struct Google<'a> {
    agent: &'a ureq::Agent,
    token: &'a str,
    deadline: Instant,
}

impl Google<'_> {
    fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T, Failure> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Failure::Retry);
        }
        let query = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(query)
            .finish();
        let mut response = self
            .agent
            .get(format!("{API}{path}?{query}"))
            .header("Authorization", &format!("Bearer {}", self.token))
            .config()
            .timeout_global(Some(remaining.min(Duration::from_secs(12))))
            .build()
            .call()
            .map_err(|error| match error {
                ureq::Error::Io(_) | ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => {
                    Failure::Offline
                }
                _ => Failure::Retry,
            })?;
        let status = response.status().as_u16();
        let reader = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .reader();
        match status {
            200 => serde_json::from_reader(reader).map_err(|_| Failure::Retry),
            401 => Err(Failure::Auth),
            403 => Err(permission(
                serde_json::from_reader(reader).unwrap_or(Value::Null),
            )),
            404 => Err(Failure::Removed),
            410 => Err(Failure::Expired),
            _ => Err(Failure::Retry),
        }
    }

    fn calendars(&self) -> Result<Vec<Calendar>, Failure> {
        let mut rows = Vec::new();
        let mut next = None::<String>;
        for _ in 0..8 {
            let mut query = vec![
                ("minAccessRole", "reader"),
                ("maxResults", "250"),
                ("fields", LIST_FIELDS),
            ];
            if let Some(token) = next.as_deref() {
                query.push(("pageToken", token));
            }
            let page: Page<GoogleCalendar> = self.get("/users/me/calendarList", &query)?;
            rows.extend(
                page.items
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|row| valid_id(&row.id))
                    .map(Calendar::from),
            );
            if rows.len() > MAX_CALENDARS {
                return Err(Failure::Retry);
            }
            next = page.next_page_token;
            if next.is_none() {
                return Ok(rows);
            }
        }
        Err(Failure::Retry)
    }

    fn events(
        &self,
        calendar: &Calendar,
        window: Window,
        previous: Option<&EventWindow>,
    ) -> Result<EventWindow, Failure> {
        let id = utf8_percent_encode(&calendar.id, NON_ALPHANUMERIC);
        let path = format!("/calendars/{id}/events");
        let start = window.start.to_rfc3339_opts(SecondsFormat::Secs, true);
        let end = window.end.to_rfc3339_opts(SecondsFormat::Secs, true);
        let mut query = vec![
            ("maxResults", "500"),
            ("singleEvents", "true"),
            ("showDeleted", "true"),
            ("maxAttendees", "1"),
            ("fields", EVENT_FIELDS),
        ];
        if let Some(token) = previous.and_then(|slice| slice.sync_token.as_deref()) {
            // The token already defines its original resource window.
            query.push(("syncToken", token));
        } else {
            query.extend([
                ("timeMin", start.as_str()),
                ("timeMax", end.as_str()),
                ("orderBy", "startTime"),
            ]);
        }
        let mut events = previous.map_or_else(BTreeMap::new, |slice| slice.events.clone());
        let mut next = None::<String>;
        for _ in 0..MAX_EVENT_PAGES {
            let mut page_query = query.clone();
            if let Some(token) = next.as_deref() {
                page_query.push(("pageToken", token));
            }
            let page: Page<Value> = self.get(&path, &page_query)?;
            merge_events(&mut events, page.items.unwrap_or_default(), window);
            next = page.next_page_token;
            let payload_size = serde_json::to_vec(&events)
                .map_err(|_| Failure::Retry)?
                .len()
                + events.len() * (calendar.id.len() * 2 + 70);
            if payload_size > MAX_CALENDAR_BYTES {
                return Err(Failure::Retry);
            }
            if next.is_none() {
                return Ok(EventWindow {
                    window,
                    events,
                    payload_size,
                    synced_at: SystemTime::now().into(),
                    sync_token: page.next_sync_token,
                });
            }
        }
        Err(Failure::Retry)
    }
}

fn merge_events(events: &mut BTreeMap<String, Value>, updates: Vec<Value>, window: Window) {
    for mut event in updates {
        let Some(id) = event["id"]
            .as_str()
            .filter(|id| valid_id(id))
            .map(str::to_owned)
        else {
            continue;
        };
        if event["status"] == "cancelled" || !window.overlaps(&event) {
            events.remove(&id);
        } else {
            if let Some(attendees) = event["attendees"].as_array_mut() {
                attendees.retain(|person| person["self"] == true);
            }
            events.insert(id, event);
        }
    }
}

fn permission(value: Value) -> Failure {
    for entries in [&value["error"]["errors"], &value["error"]["details"]] {
        for row in entries.as_array().into_iter().flatten() {
            match row["reason"].as_str() {
                Some("accessNotConfigured" | "SERVICE_DISABLED") => return Failure::Disabled,
                Some("insufficientPermissions" | "ACCESS_TOKEN_SCOPE_INSUFFICIENT") => {
                    return Failure::Auth;
                }
                Some("forbidden") => return Failure::Removed,
                _ => {}
            }
        }
    }
    Failure::Retry
}

impl Snapshot {
    fn refresh(&mut self, google: &Google<'_>, call: &Call, window: Window) -> Result<(), Failure> {
        let calendars = google.calendars()?;
        self.slices
            .retain(|id, _| calendars.iter().any(|row| &row.id == id));
        self.calendars = calendars;
        let list_time = SystemTime::now().into();
        self.listed_at = Some(list_time);
        let selected: Vec<_> = self
            .calendars
            .iter()
            .filter(|row| call.includes(row))
            .collect();
        if selected.is_empty() {
            return Ok(());
        }
        let mut error = None;
        if self
            .colors
            .fetched_at
            .is_none_or(|last| list_time - last >= TimeDelta::hours(24))
        {
            match google.get::<Colors>("/colors", &[("fields", "event")]) {
                Ok(mut colors) => {
                    colors.fetched_at = Some(list_time);
                    self.colors = colors;
                }
                Err(Failure::Auth) => return Err(Failure::Auth),
                Err(cause) => error = Some(cause),
            }
        }
        for chunk in selected.chunks(6) {
            if Instant::now() >= google.deadline {
                return Err(error.unwrap_or(Failure::Retry));
            }
            let updates = thread::scope(|scope| {
                let initial = &self.slices;
                chunk
                    .iter()
                    .map(|row| {
                        let previous = initial.get(&row.id).filter(|slice| {
                            slice.sync_token.is_some() && slice.window.covers(window)
                        });
                        scope.spawn(move || {
                            let first = google.events(
                                row,
                                previous.map_or(window, |slice| slice.window),
                                previous,
                            );
                            match first {
                                Err(Failure::Expired) if previous.is_some() => {
                                    (true, google.events(row, window, None))
                                }
                                _ => (false, first),
                            }
                        })
                    })
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|worker| worker.join().unwrap_or((false, Err(Failure::Retry))))
                    .collect::<Vec<_>>()
            });
            for (row, (invalidated, result)) in chunk.iter().zip(updates) {
                if invalidated {
                    self.slices.remove(&row.id);
                }
                match result {
                    Ok(events) => {
                        let occupied: usize = self
                            .slices
                            .iter()
                            .filter(|(id, _)| *id != &row.id)
                            .map(|(_, slice)| slice.payload_size)
                            .sum();
                        if occupied + events.payload_size > MAX_EVENT_BYTES {
                            error = Some(Failure::Retry);
                        } else {
                            self.slices.insert(row.id.clone(), events);
                        }
                    }
                    Err(Failure::Removed) => {
                        self.slices.remove(&row.id);
                    }
                    Err(cause) => {
                        if error.is_none() || cause == Failure::Auth {
                            error = Some(cause);
                        }
                    }
                }
            }
            if error == Some(Failure::Auth) {
                break;
            }
        }
        error.map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests;
