# Google account

Remya Core owns one Google connection per desktop user's Core configuration.
Clients can connect with identity alone or authorize the Calendar and Tasks
read/write capabilities. The same shared identity is available from every
Remya application. The Bar Account page requests identity only. The Bar Tasks
connection requests Tasks read/write access; Calendar authorization remains
available for integration development.

## Desktop configuration

Create a **Desktop app** OAuth client in the Remya Apps Google Cloud project.
Enable Google Calendar API and Google Tasks API. Put the downloaded Desktop
client JSON at `$XDG_CONFIG_HOME/remya/shared/google-oauth-client.json` (by default
`~/.config/remya/shared/google-oauth-client.json`). Use mode `0600` and keep
the file out of source control. For `--debug-root <repo>`, place it at
`<repo>/config/shared/google-oauth-client.json` instead. Core reads the
`installed.client_id` and `installed.client_secret` fields and fixes the
Google endpoints internally. This source-build setup requires that file on
each installation. Desktop credentials identify a public client.

Core opens the default browser and uses a temporary `127.0.0.1` callback,
random state, PKCE S256, and OpenID Connect nonce and ID token verification.
It requests `openid email` and only the selected capabilities:

| Capability | Google OAuth scope |
| --- | --- |
| Profile picture | `profile` |
| Calendar read/write | `https://www.googleapis.com/auth/calendar.events` |
| Tasks read/write | `https://www.googleapis.com/auth/tasks` |

Core stores the refresh token, Google subject, email and granted capabilities
together in the user's Secret Service keyring with the avatar sync selection. Install libsecret (for
`secret-tool`) and run an unlocked Secret Service provider. Tokens stay out
of settings, API responses, signals, and logs. Debug configuration roots
use separate keyring entries.

## Authorization behavior

A connect request starts browser authorization and returns immediately.
Clients can read the current status on the `google-account` invalidation.
Authorization times out if the browser does not return within three minutes.
Only one connect or disconnect operation runs at a time.

Google's desktop authorization flow requires a new consent request when
requesting additional scopes. Core requests the currently granted capabilities
together with the new selection and saves the scopes actually returned.
If Google withholds a requested capability during first connection, Core
records the partial grant and reports the missing consent. If reauthorization
would remove a prior capability or switch identities, Core keeps the existing
grant and reports the issue. The status reports the last successful grant.
Core refreshes a short-lived access token when the Tasks API needs one. The
Google subject is exposed as an opaque account identifier for cache isolation.
Remote authorization failures tell the caller to reconnect the existing
account. Core holds the access token in memory and clears it when a grant is
replaced or disconnected.

Disconnect revokes the Google grant and removes the local Secret Service
entry. Google revocation affects the account's whole Remya Apps project.
A revocation error preserves the local entry so the user can retry.

## Profile picture sync

Bar's Account page can enable sync for the connected Google account. The first
enable may require authorizing the OpenID Connect `profile` scope with that same
account. Core refreshes the photo at startup, after account changes, and every
15 minutes while sync is enabled. Core verifies the UserInfo subject before
downloading the photo and stores a private, normalized PNG cache. A temporary
failure keeps the previously cached picture. Turning sync off or disconnecting
restores the local picture and clears the synced cache when it is writable.

## Publishing

Use separate Cloud projects for development/testing and production.
In Testing, grants that include Calendar or Tasks scopes are limited to test
accounts and expire after seven days. Google classifies both write scopes as
sensitive. The production sign-in requests `openid email`. The consent screen
uses verified branding, with a public homepage, privacy policy and terms on
the verified domain. Production use of Calendar and Tasks requires scope
approval.
A production scope submission should demonstrate the implemented Tasks
features and explain the user benefit.

References:

- [Google desktop OAuth](https://developers.google.com/identity/protocols/oauth2/native-app)
- [Google OpenID Connect](https://developers.google.com/identity/openid-connect/openid-connect)
- [Google OAuth scope verification](https://support.google.com/cloud/answer/15549135)
- [Google OAuth app publishing status](https://support.google.com/cloud/answer/15549945)
