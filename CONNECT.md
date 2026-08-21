# Connecting `hetzner-mcp` to your client

`hetzner-mcp` is a **local stdio MCP server** - a single binary that speaks the
Model Context Protocol over stdin/stdout. Almost every MCP-capable CLI, agent,
and editor can use it. This guide gives the exact config for the popular ones.

## Prerequisites

1. **Install the binary** so it's on your `PATH`:
   ```bash
   cargo install hetzner-mcp
   ```
   Check the location with `which hetzner-mcp` (e.g. `~/.cargo/bin/hetzner-mcp`).
   If a client can't find it on `PATH`, use that **absolute path** as the `command`.
2. **Get an API token** per
   [Hetzner's guide](https://docs.hetzner.com/cloud/api/getting-started/generating-api-token).
   A **read-only** token is enough for every `list_*`/`get_*` tool; only create
   a read-write token if you need the mutating tools.

## The launch contract (same everywhere)

| Field | Value |
| --- | --- |
| command | `hetzner-mcp` |
| args | (none) |
| env | `HCLOUD_TOKEN=your-token` |

The binary starts the stdio server directly - there is no subcommand. The token
must come from the client's `env` block or the process environment; the server
does **not** read `.env` files. The canonical JSON shape - used by **Claude
Code, Cursor, Windsurf, Cline, Roo Code, Gemini CLI, Antigravity** - is:

```json
{
  "mcpServers": {
    "hetzner": {
      "command": "hetzner-mcp",
      "env": { "HCLOUD_TOKEN": "your-token" }
    }
  }
}
```

> 🔐 **Keep your token out of committed files.** Prefer a client's "add" CLI or
> shell-variable expansion (noted per client) over hardcoding the token into a
> config you might check into git.

---

## CLI agents

### Claude Code (CLI)
One command (no file editing):
```bash
# this project only (default)
claude mcp add hetzner --env HCLOUD_TOKEN=your-token -- hetzner-mcp
# all your projects
claude mcp add --scope user hetzner --env HCLOUD_TOKEN=your-token -- hetzner-mcp
# shared with your team (writes a committable .mcp.json)
claude mcp add --scope project hetzner --env HCLOUD_TOKEN=your-token -- hetzner-mcp
```
Project scope writes `.mcp.json` (canonical `mcpServers` shape) and prompts for
approval on first use. Docs: <https://code.claude.com/docs/en/mcp-quickstart>

### OpenAI Codex CLI
**TOML**, in `~/.codex/config.toml` (note the `mcp_servers` underscore + plural):
```toml
[mcp_servers.hetzner]
command = "hetzner-mcp"

[mcp_servers.hetzner.env]
HCLOUD_TOKEN = "your-token"
```
Or: `codex mcp add hetzner --env HCLOUD_TOKEN=your-token -- hetzner-mcp`.
Docs: <https://developers.openai.com/codex/mcp>

### Google Gemini CLI
`~/.gemini/settings.json` (or `.gemini/settings.json` per project), key `mcpServers`.
Gemini CLI expands `$VAR` inside `env`, so you can avoid hardcoding:
```json
{
  "mcpServers": {
    "hetzner": {
      "command": "hetzner-mcp",
      "env": { "HCLOUD_TOKEN": "$HCLOUD_TOKEN" }
    }
  }
}
```
Or: `gemini mcp add hetzner hetzner-mcp -e HCLOUD_TOKEN=your-token`.
Docs: <https://google-gemini.github.io/gemini-cli/docs/tools/mcp-server.html>

### Google Antigravity
Shared config at `~/.gemini/config/mcp_config.json`, key `mcpServers` (canonical
shape above). Add via Settings -> Customizations -> **Open MCP Config**, then hit
refresh in *Installed MCP Servers*.

### opencode
`opencode.json` (project) or `~/.config/opencode/opencode.json`. **Different shape**:
key is `mcp`, `type` is `local`, `command` is a **single array**, and env is
`environment`:
```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "hetzner": {
      "type": "local",
      "command": ["hetzner-mcp"],
      "enabled": true,
      "environment": { "HCLOUD_TOKEN": "your-token" }
    }
  }
}
```
Docs: <https://opencode.ai/docs/mcp-servers/>

### Crush (Charm)
`crush.json` / `.crush.json` (project) or `~/.config/crush/crush.json`, key `mcp`,
`type: "stdio"`:
```json
{
  "$schema": "https://charm.land/crush.json",
  "mcp": {
    "hetzner": {
      "type": "stdio",
      "command": "hetzner-mcp",
      "env": { "HCLOUD_TOKEN": "your-token" }
    }
  }
}
```
Docs: <https://github.com/charmbracelet/crush>

### Goose (Block)
**YAML**, `~/.config/goose/config.yaml`, under `extensions` (note `cmd`, not
`command`, and `envs`, not `env`):
```yaml
extensions:
  hetzner:
    name: Hetzner Cloud
    type: stdio
    cmd: hetzner-mcp
    enabled: true
    timeout: 300
    envs:
      HCLOUD_TOKEN: "your-token"
```
Easiest: run `goose configure` -> Add Extension -> Command-line Extension (stdio) ->
command `hetzner-mcp` -> add `HCLOUD_TOKEN`.
Docs: <https://goose-docs.ai/docs/getting-started/using-extensions>

### Amp (Sourcegraph)
`~/.config/amp/settings.json`, key `amp.mcpServers`:
```json
{
  "amp.mcpServers": {
    "hetzner": {
      "command": "hetzner-mcp",
      "env": { "HCLOUD_TOKEN": "your-token" }
    }
  }
}
```
Docs: <https://ampcode.com/manual>

### OpenHands (All Hands AI)
`config.toml` (project or `~/.openhands/config.toml`), `[mcp]` -> `stdio_servers`:
```toml
[mcp]
stdio_servers = [
  { name = "hetzner", command = "hetzner-mcp", env = { HCLOUD_TOKEN = "your-token" } }
]
```
Docs: <https://docs.openhands.dev/openhands/usage/settings/mcp-settings>

---

## Editors & IDEs

### Cursor
`.cursor/mcp.json` (project) or `~/.cursor/mcp.json` (global) - canonical
`mcpServers` shape. Docs: <https://cursor.com/docs/mcp>

### Windsurf
`~/.codeium/windsurf/mcp_config.json` (same path on macOS and Linux) -
canonical `mcpServers` shape; Windsurf hot-reloads on save.
Docs: <https://docs.devin.ai/desktop/cascade/mcp>

### VS Code (GitHub Copilot, agent mode) ⚠️
`.vscode/mcp.json` - **the odd one out**: top-level key is `servers` (not
`mcpServers`); `type: "stdio"` is accepted but optional:
```json
{
  "servers": {
    "hetzner": {
      "type": "stdio",
      "command": "hetzner-mcp",
      "env": { "HCLOUD_TOKEN": "your-token" }
    }
  }
}
```
Docs: <https://code.visualstudio.com/docs/copilot/customization/mcp-servers>

### Zed ⚠️
`settings.json` (`cmd-,`) - key is `context_servers` (not `mcpServers`):
```json
{
  "context_servers": {
    "hetzner": {
      "command": "hetzner-mcp",
      "env": { "HCLOUD_TOKEN": "your-token" }
    }
  }
}
```
Docs: <https://zed.dev/docs/ai/mcp>

---

## VS Code extensions

### Cline
Click the **MCP Servers** icon -> *Configure MCP Servers* to open
`cline_mcp_settings.json` (key `mcpServers`, canonical shape; supports `disabled`
and `autoApprove` fields).

### Roo Code
`.roo/mcp.json` (project) or the global settings file via the MCP UI - key
`mcpServers`, canonical shape.
Docs: <https://docs.roocode.com/features/mcp/using-mcp-in-roo>

### Continue
**YAML** - one file per server under `.continue/mcpServers/*.yaml` (or the
legacy `~/.continue/config.yaml`); `mcpServers` is a **list**:
```yaml
mcpServers:
  - name: hetzner
    type: stdio
    command: hetzner-mcp
    env:
      HCLOUD_TOKEN: ${{ secrets.HCLOUD_TOKEN }}
```
Docs: <https://docs.continue.dev/customize/deep-dives/mcp>

---

## Multiple projects

`HCLOUD_TOKEN` also accepts comma-separated `name=token` pairs to configure
more than one Hetzner project at once; `HCLOUD_PROJECT` optionally sets the
default project. The multi-value form belongs in your MCP client's
`env` block - the official `hcloud` CLI reads the same variable, cannot parse
it, and fails with a bare `401`:

```json
{
  "mcpServers": {
    "hetzner": {
      "command": "hetzner-mcp",
      "env": {
        "HCLOUD_TOKEN": "prod=<64-char-token>,staging=<64-char-token>",
        "HCLOUD_PROJECT": "prod"
      }
    }
  }
}
```

Full syntax, startup-rejection rules, and the new `list_projects` tool:
[README.md](README.md#multiple-projects).

## Format gotchas at a glance

Most clients use `command` + `env` under a `mcpServers` object (`hetzner-mcp`
needs no `args`). The exceptions:

| Client | Key | Notable difference |
| --- | --- | --- |
| VS Code (Copilot) | `servers` | `type: "stdio"` accepted, optional |
| Zed | `context_servers` | - |
| Goose | `extensions` | YAML; `cmd` not `command`; `envs` not `env` |
| opencode | `mcp` | `type: "local"`; `command` is one array; env is `environment` |
| Crush | `mcp` | `type: "stdio"` |
| Codex CLI | `[mcp_servers.*]` | TOML; `env` is a sub-table |
| OpenHands | `[mcp] stdio_servers` | TOML array of inline tables |
| Continue | `mcpServers` (list) | YAML list of `{name, ...}` |

## Verify the connection

Most clients list discovered tools after connecting. You should see 93 tools
(`list_servers`, `get_server`, `create_server`, ..., `get_pricing`). Ask the
agent to *"list my Hetzner servers"* to confirm `list_servers` runs.

You can also sanity-check the binary by hand:
```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2026-07-28","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}' \
  | HCLOUD_TOKEN=your-token hetzner-mcp
```
A JSON line answering with `"name":"hetzner-mcp"` means stdio is healthy.
