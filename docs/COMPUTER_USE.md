# Computer Use, Notion Policy Publishing, and Store Draft Upload

## Purpose

AppForge can use an authenticated Codex or Claude subscription session as the reasoning controller and CUA Driver as the local computer/browser transport.

This path is used for:

- runtime UI verification during the functional/performance repair stage
- publishing generated privacy/terms/support documents to a configured Notion location
- copying verified public policy URLs into store metadata
- uploading metadata, screenshots, and already-built artifacts to existing Google Play Console / App Store Connect draft records

It is not used for final review submission or production rollout.

## First-run configuration

The setup wizard can store:

- Notion publishing enabled/disabled
- target kind: page or database
- exact Notion target URL
- whether a public notion.site URL should be created
- Computer Use controller: Auto, Codex, or Claude
- whether store draft upload is enabled

Configuration is stored with the rest of AppForge settings under the normal AppForge config path.

## CUA Driver bootstrap

Run:

    appforge computer status
    appforge computer setup

If CUA Driver is missing, setup offers the official CUA installer and defaults to installing unless explicitly declined.

If the driver is installed but its daemon is stopped, setup and computer-use tasks start it in the default `standard` permission mode. On macOS AppForge launches `CuaDriver.app --args serve` before requesting TCC permissions so Accessibility and Screen Recording are attributed to the signed driver app.

If Accessibility or Screen Recording is missing on macOS, setup launches the official CUA permission flow. Login, passwords, passkeys, and MFA stay with the human.

AppForge does not start CUA in unrestricted mode.

## Codex

For a computer-use run, AppForge invokes Codex with per-process config only:

- ChatGPT login is forced
- API/access-token environment overrides are removed
- `mcp_servers.computer.command="cua-driver"`
- `mcp_servers.computer.args=["mcp"]`

The user's global Codex MCP configuration is not edited.

## Claude

For a computer-use run, AppForge passes a temporary `--mcp-config` value containing:

    {"mcpServers":{"computer":{"command":"cua-driver","args":["mcp"]}}}

Claude's global MCP configuration is not edited.

## Notion publication model

The store stage first creates local source-of-truth documents:

- `docs/policies/privacy-policy.md`
- `docs/policies/terms.md`
- `docs/policies/support-and-data-deletion.md`

The Computer Use stage then operates only on the configured Notion target.

For a page target, it creates a dedicated policy child page and never publishes the configured parent page itself.

For a database target, it creates or updates an app-specific record/page and does not modify unrelated rows.

When public publishing is enabled, the operator must verify the resulting public URL and store it in:

    .appforge/policy-links.conf

It then replaces these placeholders in `docs/06-store.md` when verified URLs exist:

- `PUBLIC_PRIVACY_POLICY_URL`
- `PUBLIC_TERMS_URL`
- `PUBLIC_SUPPORT_URL`

A private Notion workspace URL must never be substituted for a public policy URL.

## One-attempt external approval

Setup configures *what may be automated* but does not itself authorize a later external write. After Store preparation, AppForge requires an interactive approval immediately before Notion/store automation.

The Store stage writes `.appforge/store-upload-request.conf` when store draft upload is enabled. It contains the exact app identifier, bounded actions, and SHA-256 values for requested artifacts.

Run:

    appforge approve-publish <project>

AppForge displays the current Notion target/publication mode plus the exact store request, rejects path traversal/unsupported actions, verifies requested artifacts remain inside the project, and verifies their SHA-256 values. A `y` confirmation creates `.appforge/publish-approved.conf`.

Then run:

    appforge publish <project>

The approval must match the current config/request and is consumed after one Publish attempt, successful or not. A retry requires a new approval.

## Store automation boundary

Draft automation requires an exact existing app-record match by package/bundle identifier.

Allowed when enabled:

- draft listing metadata edits
- privacy-policy URL entry
- screenshots/assets upload
- upload of existing signed build artifacts
- saving editable draft state

Not allowed:

- creating ambiguous or irreversible store identifiers
- accepting legal agreements
- changing pricing
- submitting to App Review / Google Play review
- production rollout
- release-to-users actions
- account/security changes

If login or MFA is required, the automation records the blocker and stops. The one-attempt approval is still consumed; after the human completes authentication, a new `approve-publish` is required before retrying.

## Quality repair

`appforge repair <project>` runs:

1. functional/performance repair
2. QA re-verification

`appforge repair-all` discovers generated projects below the configured project root and runs the same repair gate for each one.

When CUA Driver and a Codex/Claude controller are ready, the quality stage may launch and inspect a runnable app UI in addition to CLI checks. It must record actual evidence in `docs/04-quality.md` rather than inventing benchmark results.
