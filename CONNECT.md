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
3. **Create the config file** `~/.config/hetzner-mcp/config.toml`
   (`%USERPROFILE%\.config\hetzner-mcp\config.toml` on Windows) with at least
   one project, and keep it private:
   ```bash
   mkdir -p ~/.config/hetzner-mcp
   $EDITOR ~/.config/hetzner-mcp/config.toml
   chmod 600 ~/.config/hetzner-mcp/config.toml
   ```
   ```toml
   [[projects]]
   name = "main"
   token = "<64-char token>"
   ```
   Full syntax (several projects, `default`, `endpoint`, validation rules):
   [README > Configuration](README.md#configuration). This file is the only
   place credentials come from - every client snippet below is command-only.

## The launch contract (same everywhere)

| Field | Value |
| --- | --- |
| command | `hetzner-mcp` |
| args | (none) - or `["--config", "/abs/path/config.toml"]` to use another file |
| env | (none) - credentials live in `~/.config/hetzner-mcp/config.toml` |

The binary starts the stdio server directly - there is no subcommand. All
credentials come from the config file ([README > Configuration](README.md#configuration));
the server reads no environment variables and no `.env` files. The canonical
JSON shape - used by **Claude Code, Cursor, Windsurf, Cline, Roo Code, Gemini
CLI, Antigravity** - is:

```json
{
  "mcpServers": {
    "hetzner": {
      "command": "hetzner-mcp"
    }
  }
}
```

> 🔐 **Your client config holds no secrets.** Since 0.4.0 the token never
> appears in a client config, so `.mcp.json`, `.cursor/mcp.json` and friends are
> safe to commit. Keep `config.toml` itself out of git and at mode `0600`.

---

## CLI agents

### Claude Code (CLI)
One command (no file editing):
```bash
# this project only (default)
claude mcp add hetzner -- hetzner-mcp
# all your projects
claude mcp add --scope user hetzner -- hetzner-mcp
# shared with your team (writes a committable .mcp.json)
claude mcp add --scope project hetzner -- hetzner-mcp
```
Project scope writes `.mcp.json` (canonical `mcpServers` shape) and prompts for
approval on first use. Docs: <https://code.claude.com/docs/en/mcp-quickstart>

### OpenAI Codex CLI
**TOML**, in `~/.codex/config.toml` (note the `mcp_servers` underscore + plural):
```toml
[mcp_servers.hetzner]
command = "hetzner-mcp"
```
Or: `codex mcp add hetzner -- hetzner-mcp`.
Docs: <https://developers.openai.com/codex/mcp>

### Google Gemini CLI
`~/.gemini/settings.json` (or `.gemini/settings.json` per project), key
`mcpServers`, canonical shape above.
Or: `gemini mcp add hetzner hetzner-mcp`.
Docs: <https://google-gemini.github.io/gemini-cli/docs/tools/mcp-server.html>

### Google Antigravity
Shared config at `~/.gemini/config/mcp_config.json`, key `mcpServers` (canonical
shape above). Add via Settings -> Customizations -> **Open MCP Config**, then hit
refresh in *Installed MCP Servers*.

### opencode
`opencode.json` (project) or `~/.config/opencode/opencode.json`. **Different shape**:
key is `mcp`, `type` is `local`, and `command` is a **single array**:
```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "hetzner": {
      "type": "local",
      "command": ["hetzner-mcp"],
      "enabled": true
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
      "command": "hetzner-mcp"
    }
  }
}
```
Docs: <https://github.com/charmbracelet/crush>

### Goose (Block)
**YAML**, `~/.config/goose/config.yaml`, under `extensions` (note `cmd`, not
`command`):
```yaml
extensions:
  hetzner:
    name: Hetzner Cloud
    type: stdio
    cmd: hetzner-mcp
    enabled: true
    timeout: 300
```
Easiest: run `goose configure` -> Add Extension -> Command-line Extension (stdio) ->
command `hetzner-mcp` (no environment variables needed).
Docs: <https://goose-docs.ai/docs/getting-started/using-extensions>

### Amp (Sourcegraph)
`~/.config/amp/settings.json`, key `amp.mcpServers`:
```json
{
  "amp.mcpServers": {
    "hetzner": {
      "command": "hetzner-mcp"
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
  { name = "hetzner", command = "hetzner-mcp" }
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
      "command": "hetzner-mcp"
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
      "command": "hetzner-mcp"
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
```
Docs: <https://docs.continue.dev/customize/deep-dives/mcp>

---

## CI runners and containers

There is no home directory with a hand-written `config.toml` on a fresh runner,
so write the file from a secret before the agent starts. Store the **whole TOML
file** in one secret (e.g. `HETZNER_MCP_CONFIG`) and materialise it at the
default path:

```bash
mkdir -p ~/.config/hetzner-mcp
printf '%s\n' "$HETZNER_MCP_CONFIG" > ~/.config/hetzner-mcp/config.toml
chmod 600 ~/.config/hetzner-mcp/config.toml
```

In a container, mount the file read-only instead and point the server at it:

```json
{
  "mcpServers": {
    "hetzner": {
      "command": "hetzner-mcp",
      "args": ["--config", "/run/secrets/hetzner-mcp.toml"]
    }
  }
}
```

Use an absolute path with `--config` - MCP clients start the server from an
arbitrary working directory, and every startup error names the resolved path.

## Multiple projects

Add one `[[projects]]` table per Hetzner project to `config.toml`; nothing
changes on the client side. The `project` argument, the optional `default`
project, descriptions, and the `list_projects` tool are documented in
[README.md](README.md#multiple-projects).

## Format gotchas at a glance

Most clients use `command` under a `mcpServers` object (`hetzner-mcp` needs no
`args` and no `env`). The exceptions:

| Client | Key | Notable difference |
| --- | --- | --- |
| VS Code (Copilot) | `servers` | `type: "stdio"` accepted, optional |
| Zed | `context_servers` | - |
| Goose | `extensions` | YAML; `cmd` not `command` |
| opencode | `mcp` | `type: "local"`; `command` is one array |
| Crush | `mcp` | `type: "stdio"` |
| Codex CLI | `[mcp_servers.*]` | TOML table |
| OpenHands | `[mcp] stdio_servers` | TOML array of inline tables |
| Continue | `mcpServers` (list) | YAML list of `{name, ...}` |

## Verify the connection

Most clients list discovered tools after connecting. You should see 93 tools
(`list_servers`, `get_server`, `create_server`, ..., `get_pricing`). Ask the
agent to *"list my Hetzner servers"* to confirm `list_servers` runs.

You can also sanity-check the binary by hand:
```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2026-07-28","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}' \
  | hetzner-mcp
```
A JSON line answering with `"name":"hetzner-mcp"` means stdio is healthy. If
the client reports *disconnected* instead, run the same line in a terminal: a
missing or invalid `config.toml` is reported on stderr with the exact path the
server looked at.
