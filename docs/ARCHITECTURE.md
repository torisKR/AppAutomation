# Architecture

## Control plane

AppForge is a Rust control-plane binary. It owns:

- first-run configuration
- provider detection and auth diagnostics
- stage scheduling
- primary/secondary role routing
- automatic/manual mode
- worker stdout/stderr streaming
- generated-project workspace creation
- Aside Browser research lanes
- release-oriented prompts

It does not proxy model HTTP APIs.

## Provider boundary

Each provider is treated as an opaque executable.

AppForge passes:

- current project directory
- stage prompt
- safe non-interactive flags
- a reduced environment that removes API-key variables for OAuth/subscription-first providers

The provider CLI owns authentication, model selection defaults, quota accounting, and its own secure credential storage.

This is deliberate. Reusing or extracting OAuth tokens from another product would couple AppForge to private credential formats and can violate provider expectations.

## Role routing

Default role assignment:

- Plan: primary
- Design: secondary[0]
- Build: secondary[1], falling back to secondary[0]
- QA: primary
- Store: secondary[0]
- Release: primary

If strict subscription auth is enabled and OpenCode is selected for a stage, AppForge falls back to a non-OpenCode enabled provider because OpenCode Go currently uses a subscription key rather than OAuth.

## Aside lane

Aside runs once per pipeline stage in Guard mode.

Outputs are stored under docs/aside/<stage>.md and become input to the coding agent at that stage.

Aside is best suited to:

- current official docs and policy checks
- product/store reference research
- logged-in read-only console verification
- release-page verification

Irreversible store submission remains manual.

## Generated project

Every generated project gets:

- README.md
- AGENTS.md
- docs/
- docs/aside/
- .appforge/project.conf
- stage status markers under .appforge/
- a local git repository

The generated project's implementation stack is selected by the agents. Expo + React Native + TypeScript is the default preference for casual cross-platform games.

## Automation state

The terminal control loop has two modes:

- AUTO: successful completion advances immediately to the next stage
- MANUAL: a stage starts only after the user sends run/Enter

Ctrl+C requests MANUAL mode. If an external CLI also reacts to terminal SIGINT, the current stage may fail and can then be rerun manually; AppForge does not conceal the failure.
