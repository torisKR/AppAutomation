# Changelog

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
