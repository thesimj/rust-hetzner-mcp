# hetzner-mcp

[![crates.io](https://img.shields.io/crates/v/hetzner-mcp.svg)](https://crates.io/crates/hetzner-mcp)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/thesimj/rust-hetzner-mcp/blob/main/LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue.svg)](https://github.com/thesimj/rust-hetzner-mcp/blob/main/Cargo.toml)

An MCP (stdio) server for the [Hetzner Cloud API](https://docs.hetzner.cloud/),
in a single Rust binary: 93 tools covering every resource group of
`api.hetzner.cloud/v1`, from listing servers to managing DNS RRSets.

## Features

- **Full API coverage** - every resource group on `api.hetzner.cloud/v1`:
  servers, images, server types, SSH keys, locations, datacenters, volumes,
  networks, firewalls, Floating IPs, Primary IPs, Load Balancers, certificates,
  ISOs, placement groups, DNS zones and RRSets, action polling, and pricing.
- **Multiple projects** - one `HCLOUD_TOKEN` can hold several projects'
  tokens; a `project` argument and the `list_projects` tool select and list
  them. See [Multiple projects](#multiple-projects).
- **Safety-annotated tools** - every tool declares `readOnlyHint` and
  `destructiveHint` on the wire; billable creates and permanent deletes say so
  in their descriptions, so MCP clients can gate confirmations correctly.
- **Latest MCP revision** - speaks protocol `2026-07-28` (with automatic
  negotiation down to older client revisions), pinned by tests.
- **Guarded inputs** - action names are allowlisted against the official API
  spec (all 11 allowlists match the spec exactly, in order), path segments are validated before any URL is built, and an update
  call with no fields set is rejected locally instead of sent.
- **Local-only, zero state** - stdio transport, TLS via
  [rustls](https://github.com/rustls/rustls), no telemetry, no files
  written, nothing persisted. See [Privacy Policy](#privacy-policy).

## Install

```sh
cargo install hetzner-mcp
```

Build from source:

```sh
git clone https://github.com/thesimj/rust-hetzner-mcp
cd rust-hetzner-mcp
cargo install --path .
```

## Configure for Claude Code

```sh
claude mcp add hetzner -e HCLOUD_TOKEN=your-token-here -- hetzner-mcp
```

For any other MCP client, add it to your server config:

```json
{
  "mcpServers": {
    "hetzner": {
      "command": "hetzner-mcp",
      "env": {
        "HCLOUD_TOKEN": "your-token-here"
      }
    }
  }
}
```

## Connect another client (CLI, IDE, agent)

`hetzner-mcp` is a universal local stdio MCP server. **[CONNECT.md](CONNECT.md)**
has copy-paste setup for Claude Code, Codex CLI, Gemini CLI, Antigravity,
Cursor, Windsurf, VS Code (Copilot), Zed, Cline, Roo Code, Continue, Goose,
opencode, Crush, Amp, and OpenHands.

## Credentials

Create an API token in the Hetzner Cloud Console, per
[Hetzner's guide](https://docs.hetzner.com/cloud/api/getting-started/generating-api-token).
Tokens are per-project and come in two flavors - **read-only** covers every
`list_*`/`get_*` tool; pick **read & write** only if you need the mutating
tools. Supply it as `HCLOUD_TOKEN` in your MCP client's `env` block, or export
it in the shell that starts the client:

```bash
export HCLOUD_TOKEN="your-token-here"
```

On PowerShell:

```powershell
$env:HCLOUD_TOKEN = "your-token-here"
```

The server reads the token from the environment only - it does **not** load
`.env` files. The token is sent solely to the Hetzner API as a bearer header;
it is never logged or written to disk. Do not commit tokens; this repo's
`.gitignore` already excludes `.env` and `.env.*`.

## Environment variables

| Variable          | Required | Default                          | Notes                                                                                                                    |
| ------------------ | -------- | --------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `HCLOUD_TOKEN`    | yes      | -                                 | Hetzner Cloud API token(s). A single 64-character token names one project `default`; comma-separated `name=token` pairs (names `[a-z0-9._-]{1,64}`, each token exactly 64 characters) configure several - see [Multiple projects](#multiple-projects). Create one per [Hetzner's docs](https://docs.hetzner.com/cloud/api/getting-started/generating-api-token). |
| `HCLOUD_PROJECT`  | no       | -                                 | Sets a default project by name when `HCLOUD_TOKEN` configures more than one. With a default project configured, read-only tools may omit `project`; mutating tools still require it. |
| `HCLOUD_ENDPOINT` | no       | `https://api.hetzner.cloud/v1` | Overrides the API base URL, same convention as the official `hcloud` CLI.                                                |

## Multiple projects

`HCLOUD_TOKEN` accepts two forms:

- **Form A** - a single 64-character token: the one project is named `default`.
- **Form B** - comma-separated `name=token` pairs, one per project: names
  match `[a-z0-9._-]{1,64}`, and each token is exactly 64 characters.

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

With more than one project configured, every pre-existing tool gains a
`project` argument ("Which configured Hetzner project to act on; call
list_projects if unsure."); a successful result is wrapped
as `{"project": ..., "result": ...}`, while an error result instead gets a
prepended `project: <name>` text line. `HCLOUD_PROJECT` sets a default
project: read-only tools may then omit `project`, but mutating tools always
require it explicitly. With one project, nothing about the tools or their output
changes. Use `list_projects` to confirm names and catch a mislabeled token -
it returns each project's name, whether it's the default, and a lazy
fingerprint (server count and up to two server names, or
`unreachable: <error>`); it never returns a token.

At startup, the server rejects (non-zero exit) an empty entry, a name outside
`[a-z0-9._-]{1,64}`, a token that isn't exactly 64 characters, a duplicate
name, a duplicate token, mixing named and unnamed entries, or an
`HCLOUD_PROJECT` that names a project that isn't configured. Every Form-B
per-entry rejection cites that entry's 1-based index (the Form-A length error
and an unconfigured `HCLOUD_PROJECT` carry no index); no rejection ever
quotes a token.

The multi-value form belongs in your MCP client's `env` block: the official
`hcloud` CLI reads the same `HCLOUD_TOKEN` variable, cannot parse the
multi-value form, and fails with a bare `401`. One variable holding several
secrets also means per-project rotation is a whole-value rewrite.

## Tools

93 tools total, across 9 routers. Every `list_*`/`get_*` tool is read-only.
`*_action` tools take an allowlisted `action` name plus an optional `params`
object; the allowed actions are noted after each.

**Servers** (compute + servers_ops)
- `list_servers` - list servers, filterable by name, label selector, or status, with sort support
- `get_server` - get a server by ID
- `create_server` - create a server. **Billable.** Response includes `root_password` exactly once
- `delete_server` - delete a server and all its data. **Destructive**
- `power_server` - poweron/poweroff/reboot/shutdown a server. **Destructive** (interrupts workloads, except poweron)
- `update_server` - update a server's name and/or labels
- `get_server_metrics` - get CPU/disk/network metrics for a server over a time period
- `server_action` - run a server action. **Destructive.** Actions: add_to_placement_group, attach_iso, attach_to_network, change_alias_ips, change_dns_ptr, change_protection, change_type, create_image, detach_from_network, detach_iso, disable_backup, disable_rescue, enable_backup, enable_rescue, poweroff, poweron, reboot, rebuild, remove_from_placement_group, request_console, reset, reset_password, shutdown (23 total; poweron/poweroff/reboot/shutdown are also available via `power_server`)

**Images**
- `list_images` - list images, filterable by type
- `get_image` - get an image by ID
- `update_image` - update an image's description, type, or labels
- `delete_image` - delete an image (snapshot or backup) permanently. **Destructive**
- `image_action` - run an image action. Actions: change_protection (toggles delete protection)

**Server types**
- `list_server_types` - list available server types (plans)
- `get_server_type` - get a server type by ID

**SSH keys**
- `list_ssh_keys` - list SSH keys uploaded to the project
- `get_ssh_key` - get an SSH key by ID
- `create_ssh_key` - upload a new SSH key. Adds a persistent credential
- `delete_ssh_key` - delete an SSH key from the project. **Destructive**
- `update_ssh_key` - update an SSH key's name or labels

**Locations & datacenters**
- `list_locations` - list locations Hetzner resources can run in
- `get_location` - get a location by ID
- `list_datacenters` - list datacenters Hetzner resources can run in
- `get_datacenter` - get a datacenter by ID

**Volumes**
- `list_volumes` - list block storage volumes, filterable by label selector
- `get_volume` - get a volume by ID
- `create_volume` - create a volume. **Billable**
- `update_volume` - update a volume's name or labels
- `delete_volume` - delete a volume permanently (must be detached first). **Destructive**
- `volume_action` - run a volume action. **Destructive.** Actions: attach, detach, resize (grow-only, irreversible), change_protection

**Networks**
- `list_networks` - list private networks, filterable by label selector
- `get_network` - get a network by ID
- `create_network` - create a private network (free; billed resources attach to it)
- `update_network` - update a network's name, labels, or vSwitch route exposure
- `delete_network` - delete a network permanently. **Destructive**
- `network_action` - run a network action. **Destructive.** Actions: add_route, add_subnet, change_ip_range, change_protection, delete_route, delete_subnet

**Firewalls**
- `list_firewalls` - list firewalls, filterable by label selector
- `get_firewall` - get a firewall by ID
- `create_firewall` - create a firewall (free; billed servers it protects are not)
- `update_firewall` - update a firewall's name or labels
- `delete_firewall` - delete a firewall permanently. **Destructive**
- `firewall_action` - run a firewall action. **Destructive.** Actions: apply_to_resources, remove_from_resources, set_rules (replaces the entire rule set)

**Floating IPs**
- `list_floating_ips` - list Floating IPs, filterable by name, label selector, or sort order
- `get_floating_ip` - get a Floating IP by ID
- `create_floating_ip` - create a Floating IP. **Billable**
- `update_floating_ip` - update a Floating IP's description, labels, or name
- `delete_floating_ip` - delete a Floating IP permanently. **Destructive**
- `floating_ip_action` - run a Floating IP action. **Destructive.** Actions: assign, unassign, change_dns_ptr, change_protection

**Primary IPs**
- `list_primary_ips` - list Primary IPs, filterable by name, label selector, IP, or sort order
- `get_primary_ip` - get a Primary IP by ID
- `create_primary_ip` - create a Primary IP. **Billable**
- `update_primary_ip` - update a Primary IP's name, auto_delete flag, or labels
- `delete_primary_ip` - delete a Primary IP permanently. **Destructive**
- `primary_ip_action` - run a Primary IP action. **Destructive.** Actions: assign, unassign, change_dns_ptr, change_protection

**Load Balancers**
- `list_load_balancers` - list Load Balancers, filterable by name, label selector, or sort order
- `get_load_balancer` - get a Load Balancer by ID
- `create_load_balancer` - create a Load Balancer. **Billable**
- `update_load_balancer` - update a Load Balancer's name and/or labels
- `delete_load_balancer` - delete a Load Balancer permanently. **Destructive**
- `load_balancer_action` - run a Load Balancer action. **Destructive.** Actions: add_service, add_target, attach_to_network, change_algorithm, change_dns_ptr, change_protection, change_type, delete_service, detach_from_network, disable_public_interface, enable_public_interface, remove_target, update_service (13 total)
- `get_load_balancer_metrics` - get time-series metrics (open_connections, connections_per_second, requests_per_second, bandwidth) for a Load Balancer
- `list_load_balancer_types` - list available Load Balancer types, filterable by name
- `get_load_balancer_type` - get a Load Balancer type by ID

**Certificates**
- `list_certificates` - list TLS certificates, filterable by name, label selector, sort, or type
- `get_certificate` - get a certificate by ID
- `create_certificate` - upload a certificate, or request a managed Let's Encrypt certificate
- `update_certificate` - update a certificate's name or labels
- `delete_certificate` - delete a certificate permanently. **Destructive**
- `certificate_action` - run a certificate action. Actions: retry (retries issuance for a failed managed certificate)

**ISOs**
- `list_isos` - list ISOs available to attach to servers, filterable by name/architecture
- `get_iso` - get an ISO by ID

**Placement groups**
- `list_placement_groups` - list placement groups, filterable by name, label selector, sort, or type
- `get_placement_group` - get a placement group by ID
- `create_placement_group` - create a placement group
- `update_placement_group` - update a placement group's name or labels
- `delete_placement_group` - delete a placement group permanently. **Destructive**

**DNS zones & RRSets**
- `list_zones` - list DNS zones, filterable by name, label selector, sort, or mode
- `get_zone` - get a zone by ID or name
- `create_zone` - create a DNS zone
- `update_zone` - update a zone's labels
- `delete_zone` - delete a zone and all its RRSets permanently. **Destructive**
- `zone_action` - run a zone action. **Destructive.** Actions: change_primary_nameservers, change_protection, change_ttl, import_zonefile
- `get_zone_zonefile` - export a zone's zonefile in BIND format (the inverse is `zone_action`'s `import_zonefile`)
- `list_zone_rrsets` - list a zone's RRSets, filterable by name, type, or label selector
- `get_zone_rrset` - get a single RRSet by zone and name/type
- `create_zone_rrset` - create a new RRSet in a zone
- `update_zone_rrset` - update an RRSet's labels
- `delete_zone_rrset` - delete an RRSet from a zone permanently. **Destructive**
- `zone_rrset_action` - run an RRSet action. **Destructive.** Actions: change_protection, change_ttl, set_records, add_records, remove_records, update_records

**Global actions & pricing**
- `get_actions` - fetch one or more actions by ID (the API removed listing all actions; at least one ID is required)
- `get_action` - get a single action by ID; poll until status is success or error
- `get_pricing` - get current Hetzner Cloud pricing for all resource types

**Projects**
- `list_projects` - list every configured project by name, with `is_default` and a lazy fingerprint (server count and up to two server names, or `unreachable: <error>`). Never returns a token. See [Multiple projects](#multiple-projects)

## Coverage

Every resource group and every mutating action endpoint of
`api.hetzner.cloud/v1`: servers (full CRUD, metrics, the
23-action `server_action`, and `power_server`), images, server types, SSH
keys, locations, datacenters, volumes, networks, firewalls, Floating IPs,
Primary IPs, Load Balancers (+ types, metrics, the 13-action
`load_balancer_action`), certificates, ISOs, placement groups, DNS zones
(+ RRSets), global actions polling, and pricing.

The per-resource action-listing endpoints (`GET /<resource>/actions` and
friends) are not exposed as separate tools - poll actions with `get_actions`
/ `get_action` instead, which return the same action objects.

Storage Boxes are excluded: they live on a separate API
(`api.hetzner.com`), not `api.hetzner.cloud/v1`, so they are out of scope
for this server.

## Safety notes

- Every `list_*`/`get_*` tool is read-only and safe to call freely.
- Tools named `create_*`, `update_*`, `delete_*`, and `*_action` (including
  `power_server`) mutate real resources: creates may bill money, deletes are
  permanent, and actions can interrupt workloads - confirm with the user
  before calling any of them. The server never calls any of these on its own.
- Scope `HCLOUD_TOKEN` to a read-only API token unless you actually need the
  mutating tools - Hetzner Cloud tokens can be created as read-only per
  project.

## Verify the connection

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2026-07-28","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}' \
  | HCLOUD_TOKEN=your-token-here hetzner-mcp
```

A JSON line answering with `"name":"hetzner-mcp"` means stdio is healthy.
More per-client verification tips: [CONNECT.md](CONNECT.md).

## Development

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The test suite (160 unit tests + 1 integration test) runs entirely against a
local mock of the Hetzner API - no token and no network access to Hetzner are
needed to develop.

## Privacy Policy

`hetzner-mcp` runs entirely on your machine and collects no telemetry. The only
third party it contacts is Hetzner (`api.hetzner.cloud`), and only to fulfill
the requests you make. Your API token is sent solely to Hetzner to authenticate
those calls; the server writes no files and persists nothing. Full details:
[PRIVACY.md](PRIVACY.md).

## License

Licensed under the Apache License, Version 2.0 ([LICENSE](LICENSE)).

Copyright (c) 2026 Nick Bubelich
