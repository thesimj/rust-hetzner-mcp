# Codebase review: hetzner-mcp 0.3.1

Reviewed: 2026-08-21 against `main` at `01363b1` (Release 0.3.1).
Scope: the whole tree, not a branch diff. Focus is correctness, safety, and API fidelity.

**Verdict.** This is a tight, well-tested MCP proxy. Auth, path injection, and action allowlists are solid. The real holes are billable-create correctness and retry safety.

## What is strong

- The HTTP layer never puts a token in an error. `Projects` debug-prints names only.
- Mutating tools ignore `HCLOUD_PROJECT` and demand an explicit `project`.
- IDs are `u64`. Zone and RRSet segments are allowlisted before any URL is built.
- Action names cannot leave their lists. Empty update bodies die locally.
- Every tool carries `readOnlyHint` and `destructiveHint`. A test pins all 93.
- CI runs fmt, clippy `-D warnings`, tests, and an MSRV `cargo check --all-targets`.
- rustls only. No `danger_accept_invalid_certs`. `.env` is gitignored and not tracked.
- Empty HTTP 204 becomes `{"success": true}`. `create_zone_rrset` rejects an empty `records` array.

## High

### 1. Creates have no idempotency key

Hetzner accepts `Idempotency-Key` on POST. This client never sends one.

A model that retries `create_server` or `create_volume` bills twice. MCP clients retry failed tool calls as a matter of course.

**Fix.** Take an optional key, or hash the tool name plus args, and send it on every POST.

### 2. Create schemas omit XOR fields the API requires

`create_server` needs `location` XOR `datacenter`. The schema marks both optional. The test in `src/server/compute.rs:461` treats a body with neither as success.

Same gap:

| Tool | API needs | Schema requires |
|---|---|---|
| `create_volume` | `location` XOR `server` | `size`, `name` |
| `create_floating_ip` | `home_location` XOR `server` | `type` |
| `create_primary_ip` | `location` XOR assignee pair | `name`, `type` |
| `create_load_balancer` | `location` XOR `network_zone` | `name`, `load_balancer_type` |
| `create_certificate` | cert+key, or managed `domain_names` | `name` |

A client that follows the schema gets a Hetzner `400`.

**Fix.** Reject the missing XOR locally. Put the rule in the tool description. Change those tests to expect `invalid_params`.

### 3. `create_server` cannot set `public_net`

`CreateServerArgs` has no `public_net`, `networks`, `firewalls`, `volumes`, or `placement_group`.

You cannot turn off public IPv4 at create time. Hetzner bills that IPv4 by default. `server_action` can attach a network later. It cannot undo the billed IPv4.

**Fix.** Add those optional fields and forward them.

## Medium

### 4. Metrics `step` types disagree

`get_server_metrics` takes `Option<f64>` (`src/server/servers_ops.rs:69`). `get_load_balancer_metrics` takes `Option<String>` (`src/server/lb_zone_ops.rs:184`).

Hetzner wants seconds as a number on both. One call shape always fails on the other tool.

**Fix.** Type both as `Option<f64>`.

### 5. `enable_backup` is not marked BILLABLE

`server_action` flags `create_image` as BILLABLE. It does not flag `enable_backup` (~20% of server price). `reset_password` returns a one-time root password. The text never says so.

A client that gates confirms on the word `BILLABLE` will enable backups without a confirm.

**Fix.** Say both in the description.

### 6. HTTP client trusts any `HCLOUD_ENDPOINT`

`endpoint()` accepts `http://` and any host (`src/hcloud.rs:123`). Redirects still follow (default 10 hops).

reqwest strips `Authorization` on a host change. It can still replay a POST body (private keys, cloud-init) on 307/308. There is no total request timeout. `resp.text()` has no size cap. Gzip is on.

A model cannot set `HCLOUD_ENDPOINT`. A wrong or injected env value still sends the Bearer token to that URL.

**Fix.** Require `https://` except loopback. Set `redirect::Policy::none()`. Add `.timeout(60s)`. Cap the decoded body.

### 7. `call_tool` is not tested end to end

`src/server/mod.rs:765` says `Peer::new` is crate-private, so tests drive `plan_call` only.

Token swap, `annotate_project`, and schema injection never run together against a fake `RequestContext`.

**Fix.** Build a stdio or in-process MCP client in the test crate and call `tools/call`.

### 8. Release pack skips tests and un-hashed `npx`

Tag push does not run CI (`ci.yml` is `main` + PRs only). `release-mcpb.yml` packs with `npx -y @anthropic-ai/mcpb@2.1.2`. Version is pinned. Content hash is not.

**Fix.** Run `cargo test --locked` in the release job. Pin npm integrity.

### 9. Several list tools drop API filters

`list_images` forwards only `type`. No `name`, `architecture`, `label_selector`, or `sort`. `list_ssh_keys` is pagination only. `list_volumes` / `list_networks` / `list_firewalls` omit `name` and `sort`. `list_servers` already has those fields.

Without `name` on `list_images`, you cannot resolve `ubuntu-24.04` except by paging.

**Fix.** Add the missing query fields.

## Low

### 10. `url()` is string concat

It rejects `".."`, `?`, and `#` only (`src/hcloud.rs:80`). It misses `%2e%2e` and a missing leading `/`.

Tool inputs cannot reach this today. A future string ID that skips `validate_zone_id` would.

**Fix.** Join with `Url` and require the result to stay under `base_url`.

### 11. Duplicate helpers will drift

Two `PageArgs`. Two `ActionArgs`. Two `check_action` functions with argument order reversed (`src/server/res_ops.rs:26` vs `src/server/lb_zone_ops.rs:60`).

`schemars(range)` is not a runtime check, so `page: 0` still hits Hetzner.

**Fix.** Keep one `PageArgs`, one `ActionArgs`, and one `check_action` in `mod.rs`. Reject `page < 1` and `per_page > 50` before the HTTP call.

### 12. mcpb manifest is stale in git

`mcpb/manifest.json` says `0.3.0` and `platforms: ["darwin"]`. `Cargo.toml` is `0.3.1`. `scripts/build-mcpb.mjs` overwrites both at pack time.

Anyone who ships the file without the script publishes the wrong version.

**Fix.** Generate the committed version from `Cargo.toml` in CI, or drop the hardcoded version.

### 13. Panic comment vs release profile

`Cargo.toml` sets `panic = "abort"` for release. `.cargo/config.toml` says the crate keeps `panic = "unwind"`.

Linux `.mcpb` is `x86_64-unknown-linux-musl` only. No ARM Linux bundle.

**Fix.** Align the comment with `panic = "abort"`. Add `aarch64-unknown-linux-musl` if ARM Linux desktops matter.

### 14. `idempotent_hint` is never set

Protocol is pinned to `2026-07-28`. Every `list_*` / `get_*` tool could set `idempotent_hint = true`. Clients that gate retries on that hint cannot see it.

**Fix.** Set `idempotent_hint = true` on every read-only tool. Pin it in the crate-wide annotation test.

## Not bugs

- Path injection from tools is closed. Tokens do not appear in parse errors (`T9`).
- Multi-project routing matches the documented D2 pin rule (pin covers read-only tools only).
- Cross-host redirects do not leak the Bearer token (reqwest 0.13 strips `Authorization` on host/port/scheme change). `PRIVACY.md` matches that.
- Form A vs Form B parse, duplicate-name and duplicate-token rejection, and `list_projects` fingerprints match `docs/multi-project-spec.md` as amended by D1/D2.
- Connection reuse across `with_token` is fine. The Bearer header is per request.

## Suggested first cut

1. Idempotency-Key on every POST.
2. Local XOR checks on the six create tools, and tests that expect `invalid_params`.
3. `public_net` (and the other create-time attach fields) on `create_server`.
4. Unify metrics `step` as `Option<f64>`.

Those four change what a model actually does to a live project.

**Counts.** 3 high, 6 medium, 5 low. Worst high: retry of `create_server` without `Idempotency-Key`. Worst medium: `step` type split plus unbounded HTTP reads.
