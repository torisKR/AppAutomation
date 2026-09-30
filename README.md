# AppForge

AppForge is a local, subscription-first multi-agent factory for repeatedly producing mobile apps and casual games from a terminal.

You type the product/game idea. A primary AI CLI orchestrates the pipeline, secondary AI CLIs handle design and implementation, Aside Browser supplies browser-backed research/console context, and the terminal dashboard shows the stages and live worker output.

## Why Rust

The orchestrator is intentionally a native Rust binary:

- one distributable binary with low idle memory
- strong process isolation and explicit child-process control
- no Node/Python runtime required for the factory itself
- easy macOS/Linux/Windows GitHub Release artifacts
- straightforward Homebrew packaging
- good fit for a long-running terminal control plane

Generated mobile projects are separate from the orchestrator. For casual mobile games, agents are instructed to prefer Expo + React Native + TypeScript unless the requirements justify a different engine.

## Auth model: use your subscriptions, not hidden API billing

AppForge does **not** read OAuth tokens, keychains, browser cookies, or provider credential files.

It shells out to the official installed CLI:

| Provider | AppForge command path | Subscription/auth policy |
| --- | --- | --- |
| Codex | codex exec | ChatGPT login forced per run; API/access-token env overrides removed |
| Claude Code | claude -p | Claude subscription login; API/third-party provider env overrides removed |
| Cursor | agent -p | Browser-authenticated Cursor session; CURSOR_API_KEY removed |
| Antigravity | agy -p | Cached Google OAuth; GEMINI_API_KEY/GOOGLE_API_KEY removed |
| OpenCode Go | opencode run | Subscription product, but official connection uses a key; blocked when strict subscription auth is on |

The default is strict_subscription_auth=true. This keeps OpenCode Go available as a configured adapter without pretending its current credential mechanism is OAuth.

## Pipeline

1. **Product plan** — primary orchestrator
2. **UX / game design** — secondary worker
3. **Development** — secondary worker
4. **Functional & performance repair** — primary controller, with CUA runtime checks when available
5. **QA & review** — primary orchestrator
6. **Store & policy readiness** — secondary worker + Aside research
7. **Notion & store draft upload** — Codex or Claude + CUA Driver
8. **Release** — primary orchestrator + Aside verification lane

The quality stage is a repair gate, not a report-only pass: it finds functional defects and measurable performance risks, fixes verified issues, then reruns checks. Final App Store / Google Play review submission and production rollout remain human approval points.

## Build locally

Rust 1.82+ is sufficient for the current dependency-free build.

    cargo test --locked
    cargo build --release --locked
    ./target/release/appforge doctor

## First run

    ./target/release/appforge

or:

    ./target/release/appforge setup

The wizard:

1. detects installed Codex / Claude / Cursor / Antigravity / OpenCode CLIs
2. asks which subscriptions/providers you want enabled
3. selects one primary orchestrator
4. assigns the remaining providers as secondary workers
5. chooses the directory where generated apps will live
6. enables/disables automatic continuation and Aside Browser
7. optionally chooses a Notion page or database URL for privacy-policy/terms/support documents
8. chooses Auto, Codex+CUA, or Claude+CUA for Computer Use
9. optionally enables draft-only Play Console / App Store Connect uploads
10. checks CUA Driver and offers installation/OS permission setup when needed
11. checks provider login state and can launch official login flows

Configuration is stored at:

    ~/.config/appforge/config

## TUI / control loop

Start:

    appforge

Controls:

- type a game/app brief to create a new generated project
- auto — continue stages automatically
- manual — pause before the next stage
- run — run one stage manually
- Ctrl+C — switch to manual mode
- quit — exit when a worker is not active

The dashboard is split into provider status, pipeline state, and live worker logs. On Unix terminals it also enables clickable [Manual], [Auto], [Run], and [Quit] controls; typed commands remain the fallback.

### Headless mode

Create and run everything:

    appforge new "one-thumb neon snake roguelite with 60-second runs"

Create only:

    appforge create "daily two-player reaction duel"

Resume an existing generated project:

    appforge run ~/projects/appforge-games/neon-snake-roguelite

Repair one generated app's functional/performance weaknesses:

    appforge repair ~/projects/appforge-games/neon-snake-roguelite

Find and repair every generated AppForge project under the configured root:

    appforge repair-all

## Provider login

    appforge login codex
    appforge login claude
    appforge login cursor
    appforge login antigravity
    appforge login opencode

Diagnostics:

    appforge doctor
    appforge providers
    appforge computer status

Install/configure CUA Driver only when needed:

    appforge computer setup

## Aside Browser

AppForge expects the Aside CLI if the browser lane is enabled.

Before relying on it, the project setup process follows Aside's own guidance and uses its authenticated browser session rather than copying browser credentials.

Each generated project stores browser research notes in:

    docs/aside/

The release/store prompts deliberately prohibit purchases, legal acceptance, final review submission, and production rollout.

## Notion policy publishing

During first-run setup, AppForge can store a Notion **page URL** or **database URL** as the policy-document destination. The store stage generates verified local policy documents under `docs/policies/`; the Computer Use stage then creates app-specific Notion content, optionally publishes only the dedicated policy page(s), records public URLs in `.appforge/policy-links.conf`, and replaces policy URL placeholders in `docs/06-store.md`.

AppForge never publishes the configured parent page itself. This matters because Notion can publish subpages together with a published page. The Computer Use prompt creates a dedicated child/policy page so unrelated Notion content is not exposed.

## Computer Use and draft store upload

CUA Driver is the shared GUI/browser layer. AppForge does not modify global Codex or Claude MCP configuration:

- **Codex** receives a per-run `mcp_servers.computer = cua-driver mcp` override and continues to use the ChatGPT-authenticated Codex session.
- **Claude** receives a per-run `--mcp-config` pointing to `cua-driver mcp` and continues to use the Claude subscription session.
- If CUA Driver is missing, `appforge setup` / `appforge computer setup` can launch CUA's official installer after confirmation.
- Missing Accessibility/Screen Recording permission is surfaced and the official CUA permission flow can be launched.

When draft store upload is enabled, the Store stage writes an app-specific `.appforge/store-upload-request.conf` containing the exact package/bundle identifier, bounded draft actions, and SHA-256 values for requested artifacts. External actions do **not** run immediately.

Review and approve the exact Notion/store actions in a terminal:

    appforge approve-publish ~/projects/appforge-games/my-game

Then perform that one approved attempt:

    appforge publish ~/projects/appforge-games/my-game

The approval is consumed after the Publish attempt, even if the GUI flow fails, so retries require a fresh confirmation. Artifact paths must stay inside the generated project and AppForge verifies their SHA-256 before approval. The agent may update **existing, exactly matched** Google Play Console / App Store Connect app records, fill metadata/privacy-policy URLs, and upload only approved artifacts/screenshots. It does not create ambiguous app records or click final review/production submission buttons. Login/MFA/passkey steps remain with the human.

## GitHub Actions

Two workflows are included:

- ci.yml — test and release-mode build on pushes/PRs
- release.yml — tag-driven cross-platform build, GitHub Release upload, and same-repository Homebrew formula update

Create a release (the tag must match `Cargo.toml`):

    git tag v0.2.0
    git push origin v0.2.0

The workflow publishes:

- appforge_Darwin_arm64.tar.gz
- appforge_Darwin_x86_64.tar.gz
- appforge_Linux_x86_64.tar.gz
- appforge_Windows_x86_64.zip
- SHA256SUMS

## Homebrew

No second repository or PAT secret is required. After a tag release succeeds, the release workflow writes the checksummed formula back to `Formula/appforge.rb` on this repository's `main` branch.

Install this repository as an explicit-URL tap. Current Homebrew versions require a one-time trust grant for non-official tap formulae, so trust only AppForge rather than the whole tap:

    brew tap torisKR/appautomation https://github.com/torisKR/AppAutomation.git
    brew trust --formula torisKR/appautomation/appforge
    brew install appforge

Upgrade:

    brew update
    brew upgrade appforge

Remove:

    brew uninstall appforge
    brew untap torisKR/appautomation

## Generated-project safety boundary

Generated projects include an AGENTS.md contract. Coding agents are told not to scrape auth tokens, not to silently add paid SaaS, and not to perform irreversible mobile-store submissions.

The factory can prepare CI, store metadata, signing placeholders, screenshots/checklists, and GitHub Releases. Final App Store / Play Store submission remains an explicit human gate.

### Review safety gates and compatibility

Older config files keep their provider/project settings; absent Notion and store fields remain disabled. Setup requires a terminal and external publishing/upload options default to No. CUA discovery also checks `~/.local/bin`; readiness checks never grant permissions or start a daemon. Run `appforge computer setup` explicitly to install or grant permissions.

Quality and QA must write a fresh `.appforge/quality-decision` or `.appforge/qa-decision` containing exactly `PASS` or `BLOCKED`, plus their nonempty report. A zero CLI exit status alone does not pass the gate. Starting Quality invalidates prior QA approval. Publish and Release refuse missing or blocked decisions; old projects must run `repair` to produce these gates. `repair-all` continues after failures and returns a failing exit status with the affected projects.

Notion/store external actions additionally require `appforge approve-publish`, which generates a one-attempt `.appforge/publish-approved.conf` matching the current config and Store request. Store artifact paths are constrained to the project and their SHA-256 values are verified before approval. The approval is consumed after a Publish attempt. Final review submission, production rollout, agreements, pricing, and account changes remain prohibited. Humans must still review the resulting draft state before final submission.

Builds require Rust 1.82 or newer. Per-run computer wiring requires Codex supporting `exec --ignore-user-config` and Claude supporting `--setting-sources`, `--strict-mcp-config`, and the configured permission modes. Homebrew release URLs/checksums are updated only by the tag-driven release workflow.
