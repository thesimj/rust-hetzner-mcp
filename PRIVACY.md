# Privacy Policy

_Last updated: 2026-08-21_

`hetzner-mcp` is a local Model Context Protocol (MCP) server that runs
entirely on your own machine. It is a thin client for the
[Hetzner Cloud API](https://docs.hetzner.cloud/).

## What data is collected

The author of `hetzner-mcp` **collects nothing**. There is no telemetry, no
analytics, and no remote logging of any kind. The software has no servers of
its own.

## What the software sends, and to whom

`hetzner-mcp` communicates with exactly one third party - **Hetzner**
(`api.hetzner.cloud`, or the endpoint you set via `HCLOUD_ENDPOINT`) - and only
to perform the actions you (or your AI assistant) explicitly request: listing
and reading resources, and - only when you call a mutating tool - creating,
updating, deleting, or running actions on resources in your project. The server
never calls a mutating endpoint on its own.

Your **Hetzner API token** is sent only to that endpoint, as the
`Authorization: Bearer` header, to authenticate those requests. It is never
sent anywhere else, never logged, and never written to disk by this software.

Hetzner's handling of this data is governed by Hetzner's own
[Privacy Policy](https://www.hetzner.com/legal/privacy-policy).

## Where data is stored

- **API token**: read from the `HCLOUD_TOKEN` environment variable your MCP
  client provides. The server does not read `.env` files and does not persist
  the token.
- **Everything else**: nothing. The server writes no files, keeps no caches,
  and holds no state beyond the lifetime of a single request.

## Data retention

The software retains nothing. Stopping the server removes all data it held.

## Contact

Questions or concerns: open an issue at
<https://github.com/thesimj/rust-hetzner-mcp/issues>.
