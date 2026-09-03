# Specification: multi-project support (`HCLOUD_TOKEN` with several keys)

> **Status (0.4.0):** section 3 (configuration via `HCLOUD_TOKEN` / `HCLOUD_PROJECT`) and every `HCLOUD_ENDPOINT` mention are superseded by [`docs/config-file-spec.md`](config-file-spec.md) -
> configuration now comes from `~/.config/hetzner-mcp/config.toml` (README > Configuration); no environment variables are read for credentials.
> Sections 4-9 (per-call routing, pin semantics, schema injection, result echo, `list_projects`) still describe current behaviour.

Status: **ready to implement** | Target version: 0.3.0 | Written: 2026-08-21
Basis: research report + adversarial verification, including a compiling spike
(`cargo build`/`clippy` clean, 133 existing tests unchanged) that demonstrated
every mechanism below over real stdio.

---

## 1. Goal

One running `hetzner-mcp` process serves **several Hetzner projects**. The
caller names the target project per tool call. A single-project setup keeps
working with no visible change at all.

Non-goals for 0.3.0:

- No cross-project aggregation tool (no "list servers in all projects"). Each
  call targets exactly one project. Revisit after 0.3.0 ships.
- No reading of the `hcloud` CLI's `cli.toml` (see §12.3).
- No persistent state, no config file of our own, no keyring integration.

---

## 2. Verified constraints that shape the design

Each line is a fact that was checked, not an assumption. Implementers should
not re-litigate these without new evidence.

| # | Constraint | Evidence |
|---|---|---|
| C1 | A Hetzner token is bound to exactly one project; no cross-project or org-level token exists. | `cloud.spec.json` `info.description`: "A token is bound to a Project, to interact with the API of another Project you have to create a new token inside the Project." |
| C2 | **No API call can resolve a token to its project name or id.** The full 3.4 MB spec (151 paths) has no `/projects`, `/me`, `/account`, `/tokens` path and zero properties named `project`. Live probe: response headers carry only `ratelimit-*`, `x-correlation-id`, `traceparent`; `api.hetzner.com/v1/{projects,me,account}` → 404. | spec walk + live probe with a real token, 2026-08-21 |
| C3 | Therefore **project names must come from configuration**. Hetzner's own tools do the same: `hcloud context create <name>` makes zero API calls; the Terraform provider uses operator-invented aliases. | `hetznercloud/cli` `internal/cmd/context/create.go`; `terraform-provider-hcloud` `docs/guides/multiple-projects.md` |
| C4 | A token can only tell you: valid vs `401 unauthorized`, and write-capable vs `401 token_readonly`. Nothing identifying. | `cloud.spec.json` Error Codes table |
| C5 | Rate limits are **per project** (3600 req/h), so N tokens in one process means N independent buckets — an upside, not a shared budget. | `cloud.spec.json` `info.description` |
| C6 | rmcp spawns a **task per request** over one shared `Arc<HcloudServer>`, and handlers provably overlap (3 calls in flight, responses returned out of order). Any in-process "current project" is a real race. | `rmcp-3.1.4/src/service.rs:1348, 1555, 1575`; demonstrated in the spike |
| C7 | The token character set is **not documented anywhere**. Length is 64 (`[A-Za-z0-9]` in the official example); the `hcloud` CLI validates length only, never charset. | `cloud.spec.json` example token; `create.go:50-57` |
| C8 | `HCLOUD_TOKEN` is also read by the official `hcloud` CLI, whose `EnsureToken` checks only non-emptiness — so a multi-value export makes the CLI send the whole string as a bearer token and fail with a bare `401`. | `hetznercloud/cli` `internal/state/helpers.go:18-26` |
| C9 | No existing argument struct has a `project` field, and no tool's schema exposes a top-level `project` property. No collision. | all 92 schemas checked programmatically |
| C10 | rmcp exposes what the design needs as public API: `ToolRouter.map`, `ToolRoute.attr`, `Tool.input_schema: Arc<JsonObject>`; and `#[tool_handler]` emits `call_tool` **only if the impl does not define one**. | `rmcp-3.1.4/src/handler/server/router/tool.rs:327,162`; `src/model/tool.rs:27`; `rmcp-macros-3.1.4/src/tool_handler.rs:44` |

---

## 3. Configuration format

One variable, unchanged in name: **`HCLOUD_TOKEN`**. It accepts exactly two
forms, and the form is detected from the value itself:

| Form | Value | Meaning |
|---|---|---|
| **A - single project** (today's format) | `HCLOUD_TOKEN={key}` | One project. Its name is **`default`**. No `project` property appears on any tool. |
| **B - multi project** | `HCLOUD_TOKEN={project}={key},{project}={key}[,...]` | N named projects. Each tool gains a `project` property (§6). |

Form B with a single entry (`HCLOUD_TOKEN=prod={key}`) is legal and means one
project explicitly named `prod`. Form A is exactly equivalent to
`HCLOUD_TOKEN=default={key}`.

In Form A the name `default` is an internal label, not something the operator
typed, so a caller may refer to that project as `default`, as the empty string
`""`, or by omitting the argument entirely - all three mean the same project
(§4, §5.3). This keeps a client config that was written for Form B working
after it is reduced to one project.

### 3.1 Grammar

```
HCLOUD_TOKEN := form_a | form_b
form_a       := token                        # single project, name := "default"
form_b       := named ( "," named )*         # one or more named projects
named        := name "=" token
name         := [a-z0-9._-]{1,64}
token        := [^,=]{64}                    # exactly 64 chars, no comma, no '='
```

Form detection: a value containing no `=` is Form A. A value containing `=` is
Form B, and then **every** entry must be named (§3.2 rule 5).

Examples:

```bash
HCLOUD_TOKEN="<64-char key>"                                  # single project (today's behaviour)
HCLOUD_TOKEN="prod=<64-char key>,staging=<64-char key>"       # two named projects
HCLOUD_TOKEN="prod=<key>,staging=<key>,sandbox=<key>"         # three
```

Optional companion variable:

| Variable | Purpose |
|---|---|
| `HCLOUD_PROJECT` | Pins a default project **by name**. Only meaningful with several entries. The model cannot set environment variables, so this is an operator-only control (the same property Cloudflare's `cf-account-id` header relies on). |
| `HCLOUD_ENDPOINT` | Unchanged. Applies to every project. |

### 3.2 Parse rules (all mandatory)

1. Split on `,`. Trim ASCII whitespace around each entry and around name/token.
2. **No `=` anywhere** → Form A: one project, name `default`. This is the
   backward-compatible path (§8).
3. **Any `=` present** → Form B: split each entry on its **first** `=`.
4. **Validate every token at exactly 64 characters.** This is not optional: it
   is the guard that turns C7's unproven charset into a loud failure instead of
   silent credential corruption. The spike proved the failure mode —
   `HCLOUD_TOKEN="AAAAAAAA=BBBBBBBB"` silently sent `Bearer BBBBBBBB`.
5. Reject, at startup, with a non-zero exit:
   - an empty entry (e.g. a trailing comma),
   - an empty name or empty token,
   - a name outside `[a-z0-9._-]{1,64}`,
   - a token whose length is not 64,
   - a **duplicate name**,
   - a **duplicate token** — two names for one project is undetectable via the
     API (C2), so "I deleted it in staging" could mean prod,
   - an entry without a `name=` prefix while any other entry has one (once the
     value contains an `=` it is Form B, and every entry must be named),
   - `HCLOUD_PROJECT` naming a project that is not configured.
6. **Startup errors must never quote a token value.** Refer to the entry by its
   1-based index: `HCLOUD_TOKEN entry 2: token must be exactly 64 characters`.
   MCP clients log stderr.

### 3.3 Documentation requirement (from C8)

The README must state: *the multi-value form belongs in your MCP client's `env`
block. The official `hcloud` CLI reads the same variable and cannot parse it.*
Note also that one variable holding N secrets makes per-project rotation a
whole-value rewrite.

---

## 4. Resolution algorithm

Executed per tool call, before dispatch. `n` = number of configured projects.

```
1. n == 1
     a. call carries no `project`            -> use the only token
     b. `project` matches the configured name (Form A: "default", or "")
                                             -> use the only token
     c. `project` is any other value         -> ERROR invalid_params (§5.3)
2. n > 1
     a. call carries `project` = known name  -> use that token
     b. call carries `project` = unknown     -> ERROR invalid_params (§5.2)
     c. no `project`, HCLOUD_PROJECT set     -> use the pinned project
     d. no `project`, no pin                 -> ERROR invalid_params (§5.1)
```

Hard rule: **never fall back to a default in case 2d.** A silent default is the
single mechanism by which a `delete_server` lands in the wrong project.
Matching precedent: Cloudflare's MCP servers return an error listing the usable
accounts; the AWS MCP proxy rejects with the allowed profile list. The
anti-precedents (kubernetes-mcp-server, cloud-run-mcp) silently default and are
read-mostly tools.

Optional hardening, recommended: when `HCLOUD_PROJECT` is set, allow the
implicit default for **read-only tools only** and still require an explicit
`project` on mutating tools. The annotations already classify every tool
(42 read-only / 50 mutating, test-enforced at `src/server/mod.rs:259-284`), so
this is a one-line predicate on `read_only_hint`.

---

## 5. Error contract

All are JSON-RPC `-32602` (`invalid_params`), returned **before any HTTP
request** is made. Zero HTTP was verified for each case in the spike.

**5.1 Ambiguous (n > 1, no selector, no pin)**
```
project is required because several projects are configured; pass one of: prod, staging
```

**5.2 Unknown name**
```
unknown project "prodd"; configured projects: prod, staging
```
Exact match only. **No** fuzzy matching, prefix matching, or case folding.

**5.3 Unknown selector in single-project mode**
```
unknown project "prod"; this server has one project: default
```
Accepted without error in single-project mode: the argument omitted, the
configured name (`default` in Form A, or the operator's name in a one-entry
Form B), or the empty string `""`. Anything else is rejected rather than
ignored - a model that names a project it believes exists must not be told the
call succeeded against a different one.

Every message lists the configured names so the model recovers in one turn.
No message ever contains a token.

---

## 6. Tool schema changes

### 6.1 Conditional injection

- `n == 1` → **no `project` property on any tool.** `tools/list` must be
  byte-identical to the pre-feature output (§8).
- `n > 1` → inject a `project` property into **all 92 tools**, and mark it
  `required` unless `HCLOUD_PROJECT` is set.

Making it schema-`required` (not merely handler-enforced) is deliberate: many
clients validate the schema before the call reaches us, so this is the one
guard that can stop a mis-targeted call earlier than our own code. The handler
check in §4 stays as defence in depth.

### 6.2 Measured cost (re-measured independently, 0% deviation)

| Variant | tools array bytes | Delta |
|---|---|---|
| baseline (1 project) | 66,748 | — |
| 92 tools, optional, 32-char description | 73,831 | +7,083 (+10.6%) |
| 92 tools, optional, 100-char description | 80,087 | +13,339 (+20.0%) |
| 92 tools, required, 100-char description | 81,241 | +14,493 (+21.7%) |
| 92 tools, required, 32-char description | ~74,985 | ~+8,237 (+12.3%) |

Description length dominates. **Use a short field description** (~32 chars,
e.g. `"Target project name."`) and put the full explanation plus the list of
configured names in the server's `initialize` instructions, which are sent
once. For comparison, running one server per project costs the full 66,748
bytes **per project** (~200 KB at three projects).

### 6.3 Mechanism (proven in the spike)

At startup, after summing the 8 routers (`src/server/mod.rs:42-49`), and only
when `n > 1`: iterate `router.map.values_mut()` and insert the property via
`Arc::make_mut(&mut route.attr.input_schema)`. All three APIs are public (C10).

**No changes to any of the 48 argument structs.** `get_pricing` — the only tool
with an empty `properties` object — goes through the same path and was verified
working.

Record a caveat in a code comment plus one test: the injected property exists
in the JSON schema only, never in the Rust structs. This is safe today (no
struct uses `deny_unknown_fields`, and rmcp does not schema-validate arguments
in the call path), but a future strict-validation change in rmcp would break
all 92 tools at once.

---

## 7. Request interception and project echo

### 7.1 Interception

Hand-write `call_tool` inside the existing `#[tool_handler(router = self.tool_router)]`
impl. Because the macro only generates `call_tool` when the impl lacks one
(C10), `list_tools` and `get_tool` keep being generated for free. Verified: the
hand-written method compiles with no duplicate-method error.

```rust
// sketch, not final code
async fn call_tool(&self, mut request: CallToolRequestParam, context: RequestContext<RoleServer>)
    -> Result<CallToolResult, ErrorData>
{
    let selector = request.arguments.as_mut().and_then(|a| a.remove("project"));
    let read_only = self.tool_router.get(&request.name)
        .and_then(|r| r.attr.annotations.as_ref())
        .and_then(|a| a.read_only_hint)
        .unwrap_or(false);
    let (name, token) = self.projects.resolve(selector, read_only)?;   // §4, §5
    let scoped = Self { client: self.client.with_token(token), ..self.clone() };
    let result = scoped.tool_router.call(ToolCallContext::new(&scoped, request, context)).await?;
    Ok(annotate_project(result, &name))                                // §7.2
}
```

`HcloudClient::with_token(&self, token) -> Self` clones and swaps only the
token, so the single `reqwest::Client` (internally `Arc`) and its connection
pool are shared across projects.

Keep the field named `client` with the same type, so **all 92 `self.client…`
call sites and all 92 handler bodies stay untouched**. Put `tool_router` behind
`Arc` so the per-call clone is cheap.

### 7.2 Echo the resolved project in every result

**Do not attempt this in `respond()`.** That function is a free function with
93 call sites and no access to the resolved project; the earlier design note
claiming "one line in `respond()`" is wrong.

Correct place: post-process the `CallToolResult` inside `call_tool` (~5 lines),
covering all 92 tools at once. Requirements:

- Add the project **name** only. Never the token.
- Prefer a structured location over string mangling — e.g. wrap the JSON payload
  as `{"project": "<name>", "result": <original>}`, or append a short text
  content block. Whichever is chosen, apply it consistently and pin it with a
  test, because it changes every tool's output shape.
- Rationale: a mis-targeted call becomes visible in the transcript immediately,
  and post-hoc audit becomes possible. This is the cheapest real safety win in
  the design.

---

## 8. Backward compatibility contract

With a single-token `HCLOUD_TOKEN` (no comma, no `=`):

1. No `project` property in any tool schema.
2. `tools/list` byte-identical to the pre-feature output — **66,748 bytes**.
   Verified in the spike (`BYTE-IDENTICAL: True`).
3. All existing tests pass with no assertion changed (verified: 133/133).
4. Every one of the 19 `HCLOUD_TOKEN` snippets in `CONNECT.md` keeps working
   verbatim.

This contract is testable and must be asserted (§10, T1).

---

## 9. New tool: `list_projects`

One new tool, bringing the pinned count to **93** (`src/server/mod.rs:244` and
the title pin at `:283` must move).

- Arguments: none.
- Annotations: `read_only_hint = true`, `destructive_hint = false`.
- Never accepts or returns a token. Returns, per project: the configured
  `name`, and `is_default` (whether `HCLOUD_PROJECT` pins it).
- Must work in single-project mode too: Form A returns the single entry named
  `default`; a one-entry Form B returns that entry's operator-chosen name.

### 9.1 Optional: mislabel detection (recommended)

C2 means a **mislabeled** token — the staging key stored under the name `prod` —
is undetectable by any API check. It becomes detectable by a human or the model
if `list_projects` returns a cheap fingerprint per project: e.g. server count
plus the first two server names (`GET /servers?per_page=2`), or the location of
the first server.

- Do this **lazily and concurrently**, only when the tool is called, never at
  startup (startup latency and rate-limit cost).
- Tolerate failure per project: report `unreachable: <api error code>` for a
  project whose probe fails (this also surfaces an invalid or revoked token,
  and `token_readonly` status if a write is ever probed — do not probe writes).
- Keep it opt-out via a flag if the extra calls are unwanted.

---

## 10. Test matrix (all must pass)

| # | Test | Assertion |
|---|---|---|
| T1 | single-project schemas | `tools/list` has no `project` property on any of 92 tools; serialized size equals the pre-feature baseline |
| T2 | multi-project schemas | all 92 tools carry `project`; `required` when no pin, optional when `HCLOUD_PROJECT` is set |
| T3 | routing | `project: "staging"` sends the staging token (wiremock `header("authorization", "Bearer …")`, the pattern already used at `src/hcloud.rs:196`) |
| T4 | ambiguity | n>1, no selector, no pin → `INVALID_PARAMS`, message lists both names, **zero HTTP requests** |
| T5 | unknown name | → `INVALID_PARAMS` naming the configured projects, zero HTTP |
| T6 | unknown selector in single-project mode | `project:"prod"` with Form A → `INVALID_PARAMS`, zero HTTP |
| T6b | accepted selectors in single-project mode | Form A with `project:"default"`, with `project:""`, and with the argument omitted all succeed against the one token |
| T6c | Form A ≡ `default=` | `HCLOUD_TOKEN={key}` and `HCLOUD_TOKEN=default={key}` produce identical behaviour and identical `tools/list` bytes |
| T7 | pinned default | `HCLOUD_PROJECT=prod`, no selector → prod token on the wire |
| T8 | read-only vs mutating default (if §4 hardening is adopted) | pinned default applies to `list_servers`; `delete_server` without a selector → `INVALID_PARAMS` |
| T9 | parse: every rejection in §3.2 | non-zero exit, and the error text **contains no token substring** |
| T10 | parse: 64-char guard | `HCLOUD_TOKEN="AAAAAAAA=BBBBBBBB"` is rejected, not silently truncated |
| T11 | duplicate token | two names, one token → rejected at startup |
| T12 | echo | every tool result carries the resolved project name; the token appears nowhere in the result |
| T13 | `get_pricing` | the no-properties tool routes correctly in multi-project mode |
| T14 | `list_projects` | returns names and `is_default`; contains no token in any field |
| T15 | schema/struct divergence guard | a call with an unexpected extra argument still deserializes (documents the assumption in §6.3) |
| T16 | existing suite | all 133 current tests pass with no assertion edited |

Add a multi-project sibling of `test_support::server_for`
(`src/server/test_support.rs:10-12`).

---

## 11. Implementation plan and scope

Measured from the spike (`+148 / -14`, 2 files) plus the items the spike did
not implement.

**`src/hcloud.rs`** (~+15)
- `with_token(&self, token: impl Into<String>) -> Self` (+9, proven).
- Replace `from_env()` (`:25-29`) with a loader returning the project map plus
  the optional pin; move `HCLOUD_ENDPOINT` handling unchanged.

**`src/server/mod.rs`** (~+180)
- `Projects` type: `BTreeMap<String, String>` name→token, plus `default: Option<String>`,
  plus `resolve(selector, read_only) -> Result<(String, &str), ErrorData>` (§4, §5).
- `HcloudServer { client, projects: Arc<Projects>, tool_router: Arc<ToolRouter<Self>> }`
  — field `client` keeps its name and type (§7.1).
- `parse_token_env()` with every rule and error in §3.2.
- Hand-written `call_tool` (§7.1, ~29 lines proven) + `annotate_project` (§7.2, ~5).
- `inject_project_property()` (§6.3, ~18 proven).
- `list_projects` tool (§9) and its optional fingerprint.
- Update the pinned counts 92 → 93 at `:244` and `:283`; extend the
  instructions string (`:59-64`) with the multi-project rule.

**Untouched:** all 8 tool modules, all 48 argument structs, all 92 handler
bodies, `main.rs`, `lib.rs`.

**Docs:** README env table + a Multi-project section (including the C8 warning
and the rotation note); CONNECT.md gains one multi-project example; PRIVACY.md
gains one line — several tokens may be held in the process, each sent only to
its own project's requests.

Suggested order: (1) parse + `Projects` + tests T9-T11; (2) `with_token` +
interception + T3-T7; (3) schema injection + T1-T2; (4) echo + T12;
(5) `list_projects` + T14; (6) fingerprint; (7) docs.

---

## 12. Rejected alternatives

**12.1 Bare positional tokens** (`HCLOUD_TOKEN="k1,k2,k3"`, select by index).
Rejected: the model gets no semantic signal ("staging" is not "project 2"), and
inserting a token in the middle silently re-points every earlier reference with
no error possible. Names are cheap and make the transcript auditable.

**12.2 Context-switch tools** (`use_project` holding current state; 92 tools
unchanged). Rejected on three grounds: it is a genuine race here (C6,
demonstrated); the target becomes invisible at the call site, so
`delete_server(id=42)` reads identically after a compaction, in a resumed
session, or to a subagent that never saw the switch, and the client's approval
prompt shows nothing about the project; and Cloudflare shipped exactly this
design across 13 MCP servers and then deleted it in favour of per-call
resolution (commit `f625075`).

**12.3 Reading the `hcloud` CLI's `cli.toml`.** Rejected as a default: it would
load **every** context's token, including projects the operator never meant to
expose (the AWS MCP proxy explicitly refuses this, with an allowlist); the file
stores tokens in plaintext (mode 0600); its path is redirectable via
`HCLOUD_CONFIG`; and it adds a TOML dependency the crate does not have. Viable
later as opt-in **with an explicit allowlist**.

**12.4 One server entry per project in the client config.** Keep documenting
this for one or two projects — it needs no code. Inadequate for "several at
once": N near-identical tool names distinguished only by a server prefix,
N subprocesses, permission rules duplicated per project, no cross-project
question in one turn, and a client restart to add a project.

---

## 13. Decisions left to the implementer

1. **Echo shape** (§7.2): wrapper object vs extra text block. Wrapping changes
   every tool's payload shape — decide once, pin with T12.
2. **Read-only default hardening** (§4): recommended, costs one predicate.
3. **Fingerprint in `list_projects`** (§9.1): recommended, but it makes
   `list_projects` perform network calls — decide whether that needs a flag.
4. **`HCLOUD_TOKENS` as an accepted alias** for the multi-value form: not
   required, and it doubles the parse surface. Default answer: no.
5. Whether `docs/` should be excluded from the published crate
   (`exclude = ["docs/"]` in `Cargo.toml`) — this spec currently would ship
   inside the package.

---

## 14. Acceptance criteria

The feature is done when:

1. All of T1-T16 pass, plus `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`.
2. A single-token `HCLOUD_TOKEN` produces a `tools/list` byte-identical to
   0.2.x (§8).
3. A two-project `HCLOUD_TOKEN` demonstrates over real stdio: the injected
   property present on all tools, the correct token on the wire per named
   project, and an error with zero HTTP for both the ambiguous and the
   unknown-name cases.
4. No token value can appear in a startup error, a tool result, a tool schema,
   or `list_projects` output — asserted, not assumed.
5. README, CONNECT.md, and PRIVACY.md updated; the `hcloud` CLI collision (C8)
   documented.

---

## 15. Implementation notes (0.3.0, 2026-08-21)

Shipped per decisions R3/F2: the project-property description is 73
characters ("Which configured Hetzner project to act on; call list_projects
if unsure."), not the ~32-char guidance in §6.2. Re-measured 92-tool
`tools/list` is **67,962 bytes**, with the pinned-default required-set
semantics of D2 (mutating tools always require `project`) — this is the
single-project set (no `project` property; with it, 79,971 bytes / 79,317
with a default project configured). This supersedes §6.2's 32-char guidance
and its 66,748/~74,985 baseline figures; §6.2 is left unedited as history.
