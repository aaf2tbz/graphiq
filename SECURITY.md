# Security Policy

## Supported Versions

Only the latest release receives security updates.

| Version | Supported          |
| ------- | ------------------ |
| latest  | :white_check_mark: |
| < latest| :x:                |

## Reporting a Vulnerability

**Do not report security vulnerabilities through public GitHub issues.**

Instead, use [GitHub's private vulnerability reporting](https://github.com/aaf2tbz/graphiq/security/advisories/new) for this repository.

### What to include

- Description of the vulnerability and its impact
- Steps to reproduce or a proof of concept
- Affected versions (if known)
- Any suggested mitigations or fixes

### Response timeline

- **Acknowledgment** within 48 hours
- **Initial assessment** within 5 business days
- **Fix or mitigation** depends on severity:
  - Critical: within 7 days
  - High: within 14 days
  - Moderate/Low: next release cycle

### Disclosure policy

We follow coordinated disclosure. Once a fix is released, we publish a GitHub Security Advisory with credit to the reporter (unless anonymity is requested).

## Security features in this repository

- **Dependabot alerts and auto-fixes** — enabled
- **Secret scanning** — enabled with push protection
- **Dependency review** — on all PRs
