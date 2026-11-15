# Security Policy

## Supported versions

Security fixes are prioritized for the latest release and the default branch. Older
releases may receive fixes where practical.

## Reporting a vulnerability

Use [GitHub private vulnerability reporting](https://github.com/remyhuang03/remya-core/security/advisories/new).
Keep vulnerability details out of issues and pull requests. Include the affected version
or commit, environment, expected security boundary, observed impact, and sanitized
evidence. Remove credentials, private data, and sensitive logs.

Coordinate disclosure with the maintainer so they can verify the report and prepare a
fix.

## Scope

Remy'a Core provides the per-user daemon, CLI, session D-Bus API and shared client
packages. Report boundary violations involving authorization for system changes,
privileged helper inputs, saved preferences, disclosure of private settings, or
reconciliation that changes system state without the expected user action. The packaged
root updater is limited to its fixed system-package upgrade operation.

Report standalone dependency vulnerabilities to the upstream project. Report exposure
specific to this application's integration here.
