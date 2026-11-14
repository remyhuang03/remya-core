# Power and energy saver

Core owns `power.json` and `power.runtime.json` under the system root in the
[XDG layout](xdg-layout.md). GUI clients, CLI and session D-Bus use the same
backend. No privileged helper or legacy PowerProfiles interface is used.

PowerProfiles is required at `org.freedesktop.UPower.PowerProfiles`, object
`/org/freedesktop/UPower/PowerProfiles`. Available profiles come from `Profiles`;
changes write `ActiveProfile` and verify fresh readback. UPower supplies aggregate
battery percentage, state and external-power status.

Screen, lock and sleep timeouts edit only unambiguous listeners in
`$XDG_CONFIG_HOME/hypr/hypridle.conf`: screen off is the `hl.dsp.dpms` disable
action, lock is `on-timeout = loginctl lock-session`, and sleep is a suspend
command. Commands remain intact. Never comments out the listener using
`# remya-disabled: ` prefixes, which Settings can reverse. A missing lock
listener reads as Never; choosing a time appends one. **Lock before sleep**
is the `before_sleep_cmd = loginctl lock-session` line in `general`; turning it
off comments it out the same way, turning it on restores it or adds it to
`general`, and a different `before_sleep_cmd` command is never replaced. The
locking options are shown only when `general` sets `lock_cmd`, reported as
`system.can_lock`, because `loginctl lock-session` does nothing otherwise. The
Power page draws all three timeouts on one idle timeline.
Includes, duplicate targets and unsupported shapes are refused. The current-user
hypridle instance is restarted; failure restores the previous file and attempts
recovery. Other idle daemons are not supported.

Settings startup and opening its GUI ensure hypridle is running. Remya Bar
requests the same startup operation. Settings reuses an existing daemon or
starts the packaged `hypridle.service` through the current user systemd session.
The Hyprland session environment and an existing hypridle configuration are
required to start the service. Timeout edits restart a systemd-managed daemon
through its user unit.

Keep awake holds a session D-Bus idle inhibitor while enabled. It requires
hypridle to own `org.freedesktop.ScreenSaver` with D-Bus inhibition enabled.
It pauses hypridle listeners that honor inhibition. Core owns
the cookie, so closing Settings or Bar leaves it active. Stopping Core
or hypridle releases it. Keep awake creates no files and starts disabled.

Automatic energy saver defaults to off. Thresholds are 10, 20, 30, 50 and 100
percent (Always), with an independent include-external-power preference (default
false). A manual profile change suppresses automatic changes until eligibility
changes. In Power & battery → Energy saver, **Turn off automatically at**
controls when an automatically enabled saver restores the previous profile.
**Follow turn-on threshold** is the default, including for existing preferences:
the previous profile returns once the battery rises above the turn-on threshold.
A separate percentage restores it as soon as the battery reaches that percentage.
The turn-off percentage must be higher than the turn-on percentage. **Never**
keeps an automatically enabled saver active across battery percentages. When
automatic activation is disabled or external power becomes ineligible, the
previous profile is still restored. The turn-off control is shown for
percentage-based activation. These managed changes are distinguished from
external configuration conflicts.

Optional dimming affects built-in kernel backlights only, reducing the entry
brightness to 70 percent, with a minimum of 1 percent. Exit restores only a
backlight still at the automatic level. Runtime restoration state survives a
service restart. Physical brightness is live state, not configuration to restore
on every startup.

## Battery history

SQLite 3 through MIT-licensed rusqlite stores append-only observations permanently:
`$XDG_STATE_HOME/remya/core/power.sqlite3`, defaulting to
`~/.local/state/remya/core/power.sqlite3`. Debug uses the repository `config/state/`
directory. WAL and SHM files belong beside the database. No automatic retention
limit is applied. Query ranges and page sizes bound memory use.

Every 60 seconds while awake the service records a real UPower observation or a
read error. Any active display lit means screen-on; all displays off means
screen-off; sleep is separate. Hyprland is checked each second. logind
PrepareForSleep records sleep/resume boundaries. Session boundaries, unknown
states and sampling interruptions remain explicit; gaps are never filled with
invented battery levels or sleep. Timestamps are UTC Unix seconds. A clock jump
creates a gap, and cursor order follows insertion IDs.

History queries return raw events plus preceding state/session/battery context for range clipping. The battery usage page shows the last 24 hours or 7 days with a theme-colored fl_chart line/area chart and observed-duration bars. Unobserved time is reported separately; gaps break the curve. Per-application battery usage remains planned. fl_chart uses the MIT license.

## Public operations

See the canonical [Power API](public-api.md#power) and
[live display controls](public-api.md#live-display-controls) for payloads,
history queries and change notifications.

## Verification

`cargo test -p remya_power -p remya_service` covers storage,
policy predicates, safe timeout transformations and service dispatch.
`cargo run -p remya_power --example probe -- <isolated-dir> <hypridle.conf>`
is read-only for host settings. Add `exercise` to switch profiles, exercise the
automatic include-AC policy and built-in dimming, restoring the starting profile.
This workstation passed that roundtrip on 2026-11-04, and the first SQLite sample
was read back at 96 percent with external power. The hypridle timeout/restart roundtrip also passed and restored the original file byte-for-byte; on 2026-11-09 it also changed the lock timeout and toggled lock before sleep on the live configuration before restoring it. Real suspend/resume and actual
idle timeout expiry require separate desktop verification.

On a running Hyprland session, `dbus-run-session -- python tests/power_interface_smoke.py --with-hypridle`
checks CLI/D-Bus parity and the inhibitor lifecycle using a temporary hypridle
configuration with harmless listeners.
