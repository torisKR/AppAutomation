# Provider authentication policy

## Non-negotiable rule

AppForge never reads or exports another AI product's OAuth token, browser cookie, keychain item, or internal credential database.

The integration boundary is the provider's official CLI.

## Codex

- binary: codex
- login: codex login
- status: codex login status
- execution: codex exec
- child environment removes OPENAI_API_KEY, CODEX_API_KEY, and CODEX_ACCESS_TOKEN
- per-run config forces `forced_login_method="chatgpt"`

Goal: use the ChatGPT-authenticated Codex CLI session rather than an ambient API key.

For computer-use tasks, AppForge supplies ephemeral Codex config overrides for `mcp_servers.computer.command="cua-driver"` and `args=["mcp"]`. It does not write this server into the user's global Codex config.

## Claude Code

- binary: claude
- login: claude auth login
- status: claude auth status
- execution: claude -p
- child environment removes ANTHROPIC_API_KEY and ANTHROPIC_AUTH_TOKEN

This matters because Claude Code can otherwise prefer an API key and bill API usage instead of subscription allocation.

For computer-use tasks, AppForge supplies an ephemeral `--mcp-config` containing `cua-driver mcp`. It does not add a persistent Claude MCP server.

## Cursor

- binary: agent or cursor-agent
- login: agent login
- status: agent status
- execution: agent -p
- child environment removes CURSOR_API_KEY

## Antigravity

- binary: agy
- first interactive launch performs Google OAuth if needed
- execution: agy -p
- child environment removes GEMINI_API_KEY and GOOGLE_API_KEY

Automatic runs use sandbox mode and keep Antigravity's configured permission policy. AppForge does not pass `--dangerously-skip-permissions`. Workspace file reads/writes are available in headless mode; commands that still require approval can be granted with scoped Antigravity permission rules.

## OpenCode Go

OpenCode Go is a paid subscription, but its documented connection mechanism is an API key pasted into OpenCode.

Therefore:

- the adapter exists
- the first-run wizard labels it Subscription key
- strict_subscription_auth=true blocks it from automatic task execution
- changing strict_subscription_auth=false is an explicit choice to accept that credential model

AppForge does not mislabel this flow as OAuth.
