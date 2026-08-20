# hetzner-mcp

An MCP server for the Hetzner Cloud API: servers, images, SSH keys, locations, datacenters, volumes, networks, and firewalls, over stdio.

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

## Environment variables

| Variable          | Required | Default                          | Notes                                                                                                                    |
| ------------------ | -------- | --------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `HCLOUD_TOKEN`    | yes      | -                                 | Hetzner Cloud API token, scoped to one project. Create one per [Hetzner's docs](https://docs.hetzner.com/cloud/api/getting-started/generating-api-token). |
| `HCLOUD_ENDPOINT` | no       | `https://api.hetzner.cloud/v1` | Overrides the API base URL, same convention as the official `hcloud` CLI.                                                |

## Tools

23 tools total. Every `list_*`/`get_*` tool is read-only.

**Servers**
- `list_servers` - list servers, filterable by name, label selector, or status
- `get_server` - get a server by ID
- `create_server` - create a server. **Billable.** Response includes `root_password` exactly once
- `delete_server` - delete a server and all its data. **Destructive**
- `power_server` - poweron/poweroff/reboot/shutdown a server. **Destructive** (interrupts workloads, except poweron)

**Images & server types**
- `list_images` - list images, filterable by type
- `get_image` - get an image by ID
- `list_server_types` - list available server types (plans)
- `get_server_type` - get a server type by ID

**SSH keys**
- `list_ssh_keys` - list SSH keys uploaded to the project
- `get_ssh_key` - get an SSH key by ID
- `create_ssh_key` - upload a new SSH key. Adds a persistent credential
- `delete_ssh_key` - delete an SSH key from the project. **Destructive**

**Locations & datacenters**
- `list_locations` - list locations Hetzner resources can run in
- `get_location` - get a location by ID
- `list_datacenters` - list datacenters Hetzner resources can run in
- `get_datacenter` - get a datacenter by ID

**Volumes, networks, firewalls**
- `list_volumes` - list block storage volumes, filterable by label selector
- `get_volume` - get a volume by ID
- `list_networks` - list private networks, filterable by label selector
- `get_network` - get a network by ID
- `list_firewalls` - list firewalls, filterable by label selector
- `get_firewall` - get a firewall by ID

## Safety notes

- Every `list_*`/`get_*` tool is read-only and safe to call freely.
- `delete_server`, `power_server`, and `delete_ssh_key` are destructive; confirm with the user before calling any of them.
- `create_server` is billable and `create_ssh_key` adds a persistent credential; both mutate your account state.
- Scope `HCLOUD_TOKEN` to a read-only API token unless you actually need the mutating tools - Hetzner Cloud tokens can be created as read-only per project.

## Not covered yet

v0.1 does not cover certificates, floating IPs, ISOs, load balancers, placement groups, primary IPs, storage boxes, or DNS zones.

## License

Apache-2.0
