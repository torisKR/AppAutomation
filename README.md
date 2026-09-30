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
4. **QA & review** — primary orchestrator
5. **Store readiness** — secondary worker + Aside research
6. **Release** — primary orchestrator + Aside verification lane

Aside is used in Guard mode for browser-side research. Irreversible App Store / Google Play submission remains a human approval point.

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
7. checks provider login state and can launch official login flows

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

## Provider login

    appforge login codex
    appforge login claude
    appforge login cursor
    appforge login antigravity
    appforge login opencode

Diagnostics:

    appforge doctor
    appforge providers

## Aside Browser

AppForge expects the Aside CLI if the browser lane is enabled.

Before relying on it, the project setup process follows Aside's own guidance and uses its authenticated browser session rather than copying browser credentials.

Each generated project stores browser research notes in:

    docs/aside/

The release/store prompts deliberately prohibit purchases, legal acceptance, and irreversible store submission.

## GitHub Actions

Two workflows are included:

- ci.yml — test and release-mode build on pushes/PRs
- release.yml — tag-driven cross-platform build, GitHub Release upload, and same-repository Homebrew formula update

Create a release:

    git tag v0.1.0
    git push origin v0.1.0

The workflow publishes:

- appforge_Darwin_arm64.tar.gz
- appforge_Darwin_x86_64.tar.gz
- appforge_Linux_x86_64.tar.gz
- appforge_Windows_x86_64.zip
- SHA256SUMS

## Homebrew

No second repository or PAT secret is required. After a tag release succeeds, the release workflow writes the checksummed formula back to `Formula/appforge.rb` on this repository's `main` branch.

Install this repository as an explicit-URL tap:

    brew tap torisKR/appautomation https://github.com/torisKR/AppAutomation.git
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
