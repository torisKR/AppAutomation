# Changelog

## 0.2.2 — 2026-09-30

### Fixed

- Windows CI now passes clippy with `-D warnings` by scoping the terminal `Write` import to Unix and avoiding a unit-value SIGINT guard binding on non-Unix platforms.
- Release verification now covers the same cross-platform lint path that previously failed only on GitHub-hosted Windows runners.

## 0.2.1 — 2026-09-30

### Fixed

- CUA Driver now self-starts in standard mode before computer-use tasks instead of failing after reboot when the daemon is stopped.
- macOS permission setup now starts `CuaDriver.app` first so Accessibility and Screen Recording prompts are attributed to the signed driver app.
- `appforge computer setup` defaults to installing CUA Driver and granting required OS permissions unless the user explicitly declines.
- CUA daemon readiness polling now checks only daemon state instead of repeatedly probing both daemon and permissions, reducing subprocess churn during startup.

### Verified

- Current CUA Driver installer and permission flow match the official `cua.ai` documentation.
- Existing GUI automation is live on macOS: driver daemon, Accessibility, Screen Recording, and app enumeration are all operational.

## 0.2.0 — 2026-09-30

### Added

- First-run Notion policy destination configuration for a page or database.
- Verified privacy policy, terms, support, and data-deletion document generation under `docs/policies/`.
- Codex/ChatGPT and Claude subscription sessions can use CUA Driver through ephemeral per-run MCP configuration.
- `appforge computer status` and `appforge computer setup` for CUA readiness, installation, and OS permission setup.
- Draft-only Google Play Console / App Store Connect computer-use workflow with exact app-record matching.
- One-attempt `appforge approve-publish` confirmation before Notion/store external writes.
- `appforge publish` for the approved Notion/store draft attempt plus the release gate.
- Functional/performance repair stage and `appforge repair` / `appforge repair-all`.
- Quality and QA PASS/BLOCKED decision files that gate publishing and release.

### Changed

- Expanded the pipeline to eight stages: plan, design, build, quality repair, QA, store/policies, publish, release.
- External actions are opt-in and default off for legacy and new configurations.
- Store artifact approvals are project-scoped, path-bounded, and SHA-256 verified before approval.
- CUA/AI CLI diagnostics now use bounded status probes to avoid hanging setup or doctor.
- TUI pipeline display follows the current stage when terminal height is limited.
- Aside research runs only on stages where fresh browser context is useful.

### Safety

- Notion parent pages and databases are never published by the automation; dedicated app-policy content is used.
- Final store review submission, production rollout, pricing, agreements, account/security changes, passwords, passkeys, and MFA remain human-controlled.
- Publish approval is consumed after one attempt, including failed attempts.
