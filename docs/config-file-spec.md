# Specification: hetzner-mcp 0.4.0 - config.toml replaces environment variables

Status: **ready to implement** | Target version: 0.4.0 | Written: 2026-09-03
Basis: three competing designs, a judge's synthesis, and one critic round
(21 findings: 4 blocking, 10 should-fix, 7 nit - every one resolved below or
listed in the "Rejected critiques" appendix). Supersedes §3 of
`docs/multi-project-spec.md`.

## Judge's scores

| Criterion (0-10) | Design 1 (minimal) | Design 2 (operator UX) | Design 3 (security) |
|---|---|---|---|
| Fidelity to requirement + D1-D9 | 9 | 8 (adds enum, `--config=`, exit 2, token trim) | 7 (moves endpoint rule, adds permission refusal, printable-ASCII rule, token trim) |
| SOLID seams | 8 | 9 (cli.rs, assemble()) | 8 |
| TDD test plan | 8 | 9 (fake-HOME CLI tests, env-guard test, misplaced-key test) | 9 (widest matrix, but tests features that are cut) |
| YAGNI discipline | 10 | 7 | 5 |
| Security hygiene | 8 (toml Display leak closed, redacted Debug missing on Config) | 8 (echoes an invalid `name`, which leaks a token when name/token are swapped) | 9 |
| Operator UX | 7 | 10 | 8 |
| **Total** | **50** | **51** | **46** |

**Winner: Design 1 as the spine**, with Design 2's diagnostics, `enum`,
env-guard test and process-level CLI tests grafted on, and Design 3's
absolute-path display and redacted `Debug` for `Config`.

Conflict decisions (one sentence each):

- **Token trimming (2, 3 trim; 1 does not):** no trim - D4 says exactly 64 characters, today's `parse_token_env` does not trim, and the "(got 65)" hint makes a stray space obvious.
- **Echoing a value that failed the name rule (2 echoes):** never - a user who swaps `name` and `token`, or writes `default = "<token>"`, would otherwise see the token on stderr; this rule now also governs `default` (rule 7) and the TOML-layer redaction (§2.4).
- **`enum` on the injected `project` property (2 only):** include it - the cheapest way to make the model pick correctly (D5's purpose), costs 3 lines, single-project schemas stay untouched.
- **`--help`/`--version` (2, 3 include; 1 cuts):** include, because D2 allows them "if trivial with std" and `--help` printing the resolved default path is the one debugging aid an MCPB user has; `--config=<path>`, exit code 2 and repeated-flag detection stay cut.
- **Where the endpoint rule lives (3 moves it to config.rs):** the https/loopback predicate stays in `hcloud.rs`, but `config::parse` calls it so every config error surfaces under one "invalid config file <path>" context and `run()` never validates anything.
- **File permission refusal (3):** cut - it breaks read-only secret mounts and non-POSIX filesystems; documented as `chmod 600` advice.
- **Directory/FIFO/1 MiB/UTF-8 pre-checks (3):** cut except what `fs::read_to_string` already reports (plus one wording hint for `InvalidData`, §2.4); `toml` without the `unbounded` feature is depth-bounded.
- **XDG relative-value rule (3):** cut - D1 says "set and non-empty", implement exactly that; the resolved path is absolutized for display instead (§2.1).

---

## 1. Goal and non-goals

**Goal.** `hetzner-mcp` reads every project (name, token, optional description), the optional default project and the optional API endpoint from one TOML file, `<config dir>/hetzner-mcp/config.toml`, at startup. `HCLOUD_TOKEN`, `HCLOUD_PROJECT` and `HCLOUD_ENDPOINT` are no longer read anywhere. Runtime behaviour (per-call `project` routing, pin semantics, result wrapping, `list_projects`, zero HTTP before the first `tools/call`) is unchanged except that descriptions are now surfaced to the model.

**Non-goals.** No env-var or `.env` fallback; no migration auto-detection; no hot reload; no `init`/`check` subcommands; no config writing; no per-project endpoint; no reading of the hcloud CLI's `cli.toml`; no keychain/secret-manager indirection; no file-permission enforcement; no schema versioning key; no new tools.

## 2. Config file

### 2.1 Location lookup

Resolved once, in this order:

1. `--config <path>` on the command line (D2): used verbatim, no tilde/env expansion. An empty value is an argv error (§2.4).
2. Otherwise `config_dir/hetzner-mcp/config.toml` where `config_dir` is:
   - `$XDG_CONFIG_HOME` if the variable is set and non-empty;
   - else `<home>/.config` where `<home>` = `std::env::home_dir()` filtered to non-empty (Rust < 1.90 returns `Some("")` for an empty `HOME` on unix; MSRV is 1.88). On Windows `home_dir()` reads `USERPROFILE` only (never `HOME`), then the profile API;
   - else error: `cannot locate the config file: neither XDG_CONFIG_HOME nor a home directory is set; pass --config <path>`.
3. Whichever branch produced the path, `config::load` passes it through `std::path::absolute` (stable since 1.79) **once**, and every message (missing file, I/O error, `invalid config file ...`, `--help`) shows that absolute path. A relative `--config` value or a relative `XDG_CONFIG_HOME` is therefore always displayed resolved against the cwd the MCP client actually chose.

These two env reads (`XDG_CONFIG_HOME`, `home_dir()`) are the only environment reads left in the crate (pinned by a test, §6).

Windows consequence (documented, not special-cased): `%USERPROFILE%\.config\hetzner-mcp\config.toml`.

### 2.2 Grammar

```
top-level (all optional, must appear ABOVE the first [[projects]] table):
  default  = "<name>"     # string; must equal the name of one [[projects]] entry
  endpoint = "<url>"      # string; https://..., or http:// to 127.0.0.1 / [::1] / localhost;
                          # trailing "/" trimmed; "" means the default https://api.hetzner.cloud/v1

[[projects]]              # one or more; file order defines the 1-based index used in errors
  name        = "<string>"   # required; matches [a-z0-9._-]{1,64}; unique
  token       = "<string>"   # required; exactly 64 characters, not trimmed; unique
  description = "<string>"   # optional; trimmed; <= 200 chars (chars().count()); "" after trim => absent
```

Unknown keys at either level are errors (`serde(deny_unknown_fields)`). Nothing else is accepted.

### 2.3 Complete example

This exact text goes in README ("Configuration") and, as an inline raw-string fixture, in `config::tests` test 2. There is no `EXAMPLE_FULL` constant in the binary (nothing at runtime would print it); the only compiled example is the 4-line `config::EXAMPLE` in §2.4.

```toml
# ~/.config/hetzner-mcp/config.toml
# ($XDG_CONFIG_HOME/hetzner-mcp/config.toml if XDG_CONFIG_HOME is set;
#  %USERPROFILE%\.config\hetzner-mcp\config.toml on Windows;
#  or any path via: hetzner-mcp --config /path/to/config.toml)
#
# Keep this file private - it holds API tokens:  chmod 600 ~/.config/hetzner-mcp/config.toml
# Top-level keys (`default`, `endpoint`) must come BEFORE the first [[projects]] table.
# Each project is a [[projects]] table - double brackets, one table per project.

# Optional. With several projects, read-only tools may omit `project` and use this one.
# Mutating tools (create_*/update_*/delete_*/*_action/power_server) always require `project`.
default = "nb-main"

# Optional. API base URL for every project. https://, or http:// to 127.0.0.1/[::1]/localhost only.
# endpoint = "https://api.hetzner.cloud/v1"

[[projects]]
name = "nb-main"                 # [a-z0-9._-]{1,64}, unique
token = "0000000000000000000000000000000000000000000000000000000000000000"   # exactly 64 characters, unique
description = "main infra: web servers, load balancer, volumes"   # optional, <= 200 chars; shown to the model - keep it short

[[projects]]
name = "nb-dns"
token = "1111111111111111111111111111111111111111111111111111111111111111"
description = "DNS zones only (read-only token)"

[[projects]]
name = "lab"
token = "2222222222222222222222222222222222222222222222222222222222222222"
# description is optional - the name alone is listed when it is absent
```

### 2.4 Validation rules and exact error templates

Every message is built from our own literals plus: an absolute path, a 1-based index, a name that has already passed the name rule, a field name, a length, or a **redacted** `toml::de::Error::message()`. A token value, a rejected name value, a rejected `default` value, or any file excerpt can never appear. `toml::de::Error`'s `Display` is **never** used (verified in toml-1.1.5 `src/de/error.rs:116-140`: it reprints the offending source line).

**Argv layer** (`cli::parse`; exit 1):
- `--config requires a path` - emitted both when `--config` is the last argument and when the next argument is an empty `OsString` (shell scripts expanding an unset `"$CFG"` hit this; without the check `std::path::absolute("")` would report an opaque I/O error).
- `unexpected argument "{arg}"` followed on the next line by `usage: hetzner-mcp [--config <path>] | --help | --version`

**Location layer** (`config::config_path`): see §2.1 message.

**File layer** (`config::load`; exit 1). `{abs_path}` is the §2.1 step 3 absolute path.
- Not found:
  ```
  config file not found: {abs_path}

  create it with at least one project, for example:

  {EXAMPLE}

  or pass another file with: hetzner-mcp --config <path>
  ```
  where `config::EXAMPLE` is exactly:
  ```
  [[projects]]
  name = "main"
  token = "<64-character API token from Hetzner Cloud Console -> Security -> API tokens>"
  # description = "optional, shown to the model"
  ```
- `io::ErrorKind::InvalidData` (typically UTF-16 written by Windows PowerShell 5.1 `Out-File`): `cannot read config file {abs_path}: {io error} (the file must be UTF-8; on Windows use Set-Content -Encoding utf8 or an editor that saves UTF-8)`. A UTF-8 BOM and CRLF line endings are accepted by `toml` (verified: `toml_parser` strips the BOM at `src/lexer/mod.rs:35`; pinned by test 1b at both the `parse` and `load` layers).
- Any other I/O error: `cannot read config file {abs_path}: {io error}` (std `io::Error` Display never includes content).
- Parse/validation failure: anyhow context `invalid config file {abs_path}` wrapping the message below (stderr shows `Error: invalid config file ...\n\nCaused by:\n    <message>`).

**TOML layer** (`config::parse` -> `render_toml_error`, from `toml::de::Error::message()` + `span()`), four steps in this order:

1. **Redact.** Scan `message()` for every segment delimited by `"..."` or `` `...` ``. If the content fails `valid_name` **or** is 32 or more characters long, replace the whole segment (delimiters included) with `<redacted>`. Rationale (verified against toml-1.1.5 with the §3 `RawConfig`/`RawProject` shapes): serde's `Unexpected::Str` prints the value - `projects = "<token>"` yields ``invalid type: string "<token>", expected a sequence`` and `projects = ["<token>"]` yields ``invalid type: string "<token>", expected struct RawProject``; `deny_unknown_fields` echoes the key - a swapped line `<token> = "nb-dns"` yields ``unknown field `<token>`, expected one of `name`, `token`, `description` ``. Every field name we accept (`name`, `token`, `description`, `default`, `endpoint`, `projects`) passes `valid_name` and is shorter than 32, so legitimate messages are untouched; the length clause also covers an all-lowercase 64-character value that would pass the name grammar.
2. **Placement hint.** If the (redacted) message starts with ``unknown field `default` `` or ``unknown field `endpoint` ``, append ` (top-level keys must come before the first [[projects]] table)`.
3. **Double-bracket hint.** If the message ends with `expected a sequence` (produced by `[projects]` with single brackets -> `invalid type: map, expected a sequence`, and by `projects = "..."` -> `invalid type: string <redacted>, expected a sequence`), append ` (projects must be written as [[projects]] tables - double brackets, one table per project)`.
4. **Line prefix.** `line {N}: {message}` where `N = text[..span.start].matches('\n').count() + 1`; if `span()` is `None`, omit the prefix.

Expected outputs the operator will actually hit: ``line 7: unknown field `key`, expected one of `name`, `token`, `description` ``; ``line 1: unknown field `items`, expected one of `default`, `endpoint`, `projects` ``; ``line 4: missing field `token` ``; `line 3: invalid basic string` (syntax error inside a token line - number only, never the line); `line 1: invalid type: map, expected a sequence (projects must be written as [[projects]] tables - double brackets, one table per project)`; `line 1: invalid type: string <redacted>, expected a sequence (projects must be written as [[projects]] tables - ...)`; `line 2: unknown field <redacted>, expected one of ...`.

**Semantic layer** (`config::parse` -> `validate`, run after a successful deserialize, in this order; first failure wins). `{i}`/`{j}` are 1-based indices; `{name}` is only ever a name that passed rule 2.
1. `projects` absent or empty: `config must define at least one [[projects]] entry, for example:\n\n{EXAMPLE}`
2. Per entry `i`, name not matching `[a-z0-9._-]{1,64}` (same predicate as today's `parse_token_env`: non-empty, `<= 64` bytes, every char `is_ascii_lowercase | is_ascii_digit | '.' | '_' | '-'`): `projects[{i}]: name must match [a-z0-9._-]{1,64}` (the value is deliberately not echoed). `valid_name` carries a one-line comment: it is a name grammar, not a token detector; rules 3 and 7 and the TOML-layer redaction echo a value only after it passes here.
3. Duplicate name: `projects[{i}]: duplicate name "{name}" (already used by projects[{j}])`
4. `token.len() != 64` (byte length, no trim): `projects[{i}] ("{name}"): token must be exactly 64 characters (got {len})`
5. Duplicate token (HashSet of seen tokens): `projects[{i}] ("{name}"): token is identical to projects[{j}] ("{other_name}") - every project needs its own token`
6. Description: trim; empty => `None`; `chars().count() > 200`: `projects[{i}] ("{name}"): description must be at most 200 characters (got {n})`
7. `default` set:
   - if `!valid_name(d)` (covers `""` and a pasted token): `default must match [a-z0-9._-]{1,64} and name a configured project; configured projects: {comma-separated names in file order}` - the value is **not** echoed;
   - else if `d` is not a configured name: `default = "{d}" names no configured project; configured projects: {names in file order}`.
   `default` with a single project is allowed.
8. Endpoint: `crate::hcloud::resolve_endpoint(raw.endpoint.as_deref())` - unchanged rule (trim trailing `/`, empty => `https://api.hetzner.cloud/v1`, else must start with `https://` or be `http://` to `127.0.0.1`/`[::1]`/`localhost` via `url::Url`); message `endpoint must use https:// (or http:// to 127.0.0.1/::1/localhost), got {origin}` where `origin` is `scheme://host` only (`hcloud::endpoint_origin`): userinfo, path and query are dropped, a host or scheme of 32+ characters is `<redacted>` (same threshold as `redact_quoted`), an unparsable value is `an unparsable URL`, a host-less one `scheme: with no host`. The raw value is never echoed - `http://<token>@evil.example/v1` or `ftp://<token>` would otherwise print the token on stderr (round-1 fixer finding 2), and `<token>:443` or `<token>://x` parses with the token as the *scheme*, which `url::Url` case-folds (round-2 fixer finding 2); the user can see the value in the file.

**Runtime** (unchanged): `Projects::resolve` still returns `-32602 invalid_params` with names only.

## 3. Module layout

All paths under `/Users/patron/projects/rust-hetzner-mcp`.

### ADD `src/config.rs` (`pub mod config;` in `src/lib.rs`)
Owns: file location, TOML shape, every rule in §2.4, redaction. No rmcp, no reqwest.

```rust
pub const EXAMPLE: &str = ...;                       // §2.4 minimal example (4 lines) - the ONLY compiled example

pub struct Project { pub name: String, pub token: String, pub description: Option<String> }
// derive Clone, PartialEq, Eq. NO derived Debug. Manual Debug: name, description, token: "<redacted>".

pub struct Config { pub base_url: String, pub default: Option<String>, pub projects: Vec<Project> }
// NO derived Debug. Manual Debug prints base_url, default, projects (redacted via Project's Debug).
// Invariants after parse(): >=1 project, unique names, unique tokens, default names a project,
// base_url already validated.

#[derive(serde::Deserialize)] #[serde(deny_unknown_fields)]
struct RawConfig { default: Option<String>, endpoint: Option<String>, #[serde(default)] projects: Vec<RawProject> }
#[derive(serde::Deserialize)] #[serde(deny_unknown_fields)]
struct RawProject { name: String, token: String, description: Option<String> }   // private, no Debug

pub fn parse(text: &str) -> anyhow::Result<Config>;   // PURE: toml::from_str::<RawConfig> -> render_toml_error -> validate
fn render_toml_error(e: &toml::de::Error, text: &str) -> anyhow::Error;   // §2.4 TOML layer steps 1-4
fn redact_quoted(message: &str) -> String;                                // step 1; pure, unit-tested on its own
fn validate(raw: RawConfig) -> anyhow::Result<Config>;                     // §2.4 rules 1-8, calls hcloud::resolve_endpoint
fn valid_name(s: &str) -> bool;

pub fn config_path(xdg_config_home: Option<PathBuf>, home: Option<PathBuf>) -> anyhow::Result<PathBuf>; // PURE §2.1 steps 1-2
//   both inputs filtered with `.filter(|p| !p.as_os_str().is_empty())`
pub fn default_path() -> anyhow::Result<PathBuf>;     // config_path(env::var_os("XDG_CONFIG_HOME").map(PathBuf::from), env::home_dir()) - the ONLY env reads
pub fn load(override_path: Option<&Path>) -> anyhow::Result<Config>;   // (override or default_path()) -> std::path::absolute -> read_to_string -> parse, with the §2.4 file-layer messages
```

`#[cfg(test)] mod tests` holds every `config::*` test from §6.

### ADD `src/cli.rs` (`pub mod cli;`)
Owns argv only; pure std, ~35 lines.
```rust
pub enum Cli { Serve { config: Option<PathBuf> }, Help, Version }
pub const USAGE: &str = "usage: hetzner-mcp [--config <path>] | --help | --version";
pub fn parse(args: impl IntoIterator<Item = OsString>) -> anyhow::Result<Cli>;
// accepts: [] ; ["--config", P] (P non-empty) ; ["--help"|"-h"] ; ["--version"|"-V"]
// rejects: ["--config"] and ["--config", ""] -> "--config requires a path"; anything else -> "unexpected argument \"{arg}\"\n{USAGE}"
pub fn help_text(default_path: &str) -> String;  // USAGE + "\n  default config file: {default_path}"
```

### CHANGE `src/main.rs` (composition root, ~20 lines)
```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use hetzner_mcp::{cli, config};
    match cli::parse(std::env::args_os().skip(1))? {
        cli::Cli::Help => {
            let shown = config::default_path()
                .and_then(|p| Ok(std::path::absolute(p)?))
                .map_or_else(|e| e.to_string(), |p| p.display().to_string());
            println!("{}", cli::help_text(&shown));
            Ok(())
        }
        cli::Cli::Version => { println!("hetzner-mcp {}", env!("CARGO_PKG_VERSION")); Ok(()) }
        cli::Cli::Serve { config } => {
            let config = config::load(config.as_deref())?;
            hetzner_mcp::server::run(config).await
        }
    }
}
```
anyhow's `main -> Result` prints `Error: ...` + `Caused by:` chain to stderr and exits 1 (D4). stdout is reserved for the MCP wire, so only `--help`/`--version` write to it.

### CHANGE `src/lib.rs`: add `pub mod cli; pub mod config;`.

### CHANGE `src/hcloud.rs`
- Delete `from_env`.
- Rename `fn endpoint(Option<String>)` to `pub(crate) fn resolve_endpoint(override_: Option<&str>) -> Result<String>`; same body; message per §2.4 rule 8. `is_loopback_http` unchanged. Doc comments lose `HCLOUD_ENDPOINT` wording. Existing three endpoint tests adapt to `Option<&str>` and assert `contains("https://")`, `!contains("HCLOUD")`, that the shown origin is `got http://<host>` without `@`/path; a fourth (`endpoint_rejection_never_echoes_userinfo_or_a_long_host`) feeds a mixed-case token as userinfo, as host, in a non-URL and in a host-less URL and asserts it is absent (case-insensitively) from the message.
- `new`, `with_token`, request logic unchanged.

### CHANGE `src/server/mod.rs`
- `pub(crate) struct Projects { entries: BTreeMap<String, crate::config::Project>, pin: Option<String> }` (was `tokens`). Fields stay crate-private. Debug impl stays names-only.
- Add `pub(crate) fn new(projects: Vec<config::Project>, pin: Option<String>) -> Self` - non-validating (Config is already valid); the only constructor used by `run`, `test_support`, `e2e_client`.
- `resolve()` unchanged in logic and return type `(String, String)`; token lookups become `.get(name).map(|p| p.token.clone())`.
- Name enumeration - one source of truth: `fn names(&self) -> Vec<&str>` (BTreeMap order; feeds the `enum`); `names_list()` becomes `self.names().join(", ")` (kept for `resolve` errors); `fn names_with_descriptions(&self) -> String` renders `name` or `name (description)` joined by `, ` -> `lab, nb-dns (DNS zones only), nb-main (main infra)`.
- **Delete** `parse_token_env`, its doc, `TOKEN_LEN` if unused, and tests `t6c`, `t9`, `t10`, `t11`.
- `inject_project_property(router, projects: &Projects)` - new signature; property becomes
  `{"type":"string","enum":[<names>],"description":"Which configured Hetzner project to act on - one of: {names_with_descriptions}. Call list_projects if unsure."}`; `required` logic unchanged (`pin.is_none() || !read_only`).
- `list_projects`: each entry gains `"description": <string|null>` (always present). Tool description becomes: `List every configured Hetzner project by name and description, with a cheap fingerprint (server count and up to two server names) to catch a mislabeled token. Never returns a token.`
- `get_info`: pin note becomes `" With a default project configured (`default` in config.toml), read-only tools may omit `project`; mutating tools always require it."`; the configured-projects sentence uses `names_with_descriptions()`.
- `pub async fn run(config: config::Config) -> anyhow::Result<()>`:
  ```rust
  let client = HcloudClient::new(config.base_url, String::new())?;   // empty template token, unchanged rationale
  let projects = Projects::new(config.projects, config.default);
  let service = HcloudServer::new(client, projects).serve(stdio()).await?;
  service.waiting().await?; Ok(())
  ```
  Zero env reads remain in `server/`.

### CHANGE `src/server/test_support.rs`
- `server_for(uri)`: `Projects::new(vec![Project{ name: "default".into(), token: "test-token".into(), description: None }], None)` (the test project keeps its literal name `default` - still a legal name, no rename churn).
- `server_for_projects(uri, names, pin)`: builds `Project` values with `project_token(n)`, `description: None`, via `Projects::new`.
- Add `pub(crate) fn dead_described(entries: &[(&str, Option<&str>)], pin: Option<&str>) -> HcloudServer`.

### CHANGE `src/server/e2e_client.rs`
Both tests construct via `Projects::new(vec![Project{..}], None)` only - `config::parse` is **not** pulled into the arrange step (a parse regression must fail a parse test, not `startup_and_handshake_send_no_http_requests`). The real file -> parse -> serve path is proven end to end by `tests/cli.rs` test 40.

### ADD `tests/cli.rs` (integration; spawns `env!("CARGO_BIN_EXE_hetzner-mcp")`)
Temp files under `std::env::temp_dir()` named `hetzner-mcp-test-{pid}-{counter}`, removed in a `Drop` guard. Never touches `~/.config`; every spawned process either passes `--config` or sets **both** `HOME` and `USERPROFILE` (and removes `XDG_CONFIG_HOME`) via `Command::env`/`env_remove` - never the test process's own env. Expected paths are built with `Path::join(...).display().to_string()` so separators match the platform (the release workflow runs `cargo test --locked` on windows-latest). Version strings come from `env!("CARGO_PKG_VERSION")`, never a literal.

### CHANGE `Cargo.toml`
- `version = "0.4.0"`.
- `toml = { version = "1", default-features = false, features = ["std", "parse", "serde"] }` (feature names verified in toml-1.1.5's Cargo.toml; `display` pulls `toml_writer`, which we never use; MSRV 1.85 <= 1.88). If the trimmed set fails to compile, fall back to `toml = "1"`.
- Update the `url` comment (no `HCLOUD_ENDPOINT` wording). Run `cargo build` once so `Cargo.lock` gains `toml` (CI uses `--locked`).

**Ownership summary (SOLID):** `cli.rs` = argv; `config.rs` = location + shape + rules + redaction; `hcloud.rs` = URL policy + HTTP; `server/mod.rs` = MCP routing/schema/strings, consumes a validated `Config`; `main.rs` = wiring and exit codes. Each has one reason to change.

## 4. Loading flow

1. `main` -> `cli::parse(args_os().skip(1))`. `Help`/`Version` print to stdout and exit 0 before any file is touched (`Help` resolves and absolutizes the default path only to print it). Usage error -> stderr, exit 1.
2. `config::load(override)`: path = `override` or `config::default_path()` (§2.1), then `std::path::absolute` on either. `fs::read_to_string`; `NotFound` -> missing-file message with `EXAMPLE`; `InvalidData` -> UTF-8 hint; other I/O -> `cannot read config file ...`.
3. `config::parse(&text)`: `toml::from_str::<RawConfig>` (errors rendered via `render_toml_error`: redact -> hints -> line prefix; never `Display`), then rules 1-8 in order, rule 8 calling `hcloud::resolve_endpoint`. Returns `Config { base_url, default, projects }`. Any error is wrapped by `load` in `invalid config file {abs_path}`.
4. `server::run(config)`: `HcloudClient::new(config.base_url, "")` (reqwest/TLS build), `Projects::new(config.projects, config.default)`, `HcloudServer::new` (injects the `project` property with enum + descriptions when `len() > 1`), `serve(stdio())`, `waiting()`. Zero HTTP requests until the first `tools/call` (existing e2e test keeps proving it).
5. Per call: unchanged (`plan_call` -> `extract_selector` -> `Projects::resolve` -> `scoped(token)` -> tool -> `maybe_annotate`).

## 5. `list_projects` output and the injected `project` schema (D5)

`list_projects` result (multi-project, pin `nb-main`):
```json
{"projects":[
  {"name":"lab","description":null,"is_default":false,"fingerprint":"0 server(s)"},
  {"name":"nb-dns","description":"DNS zones only (read-only token)","is_default":false,"fingerprint":"..."},
  {"name":"nb-main","description":"main infra: web servers, load balancer, volumes","is_default":true,"fingerprint":"..."}
]}
```
`description` is always present (`null` when unset) so the shape is stable. Tokens never appear.

Injected property on every tool except `list_projects` when `len() > 1`:
```json
"project": {
  "type": "string",
  "enum": ["lab", "nb-dns", "nb-main"],
  "description": "Which configured Hetzner project to act on - one of: lab, nb-dns (DNS zones only (read-only token)), nb-main (main infra: web servers, load balancer, volumes). Call list_projects if unsure."
}
```
`required` unchanged: present unless a pin exists and the tool is read-only. Single-project schemas: unchanged (T1).

`get_info` instructions (multi-project with pin): `... Several projects are configured; pass `project` to select one. With a default project configured (`default` in config.toml), read-only tools may omit `project`; mutating tools always require it. Configured projects: lab, nb-dns (DNS zones only (read-only token)), nb-main (main infra: ...).`

## 6. Test list (TDD)

Write each test first; one behaviour each. Tokens in tests are obviously fake. Happy-path fixtures use `"a".repeat(64)` / `"b".repeat(64)` / `project_token(name)`. **Every swap-scenario fixture (a token in the `name`, `default`, or a key position) uses a mixed-case token such as `"Ab".repeat(32)`**, because 64 lowercase characters is a legal name and would make the test pass trivially; real Hetzner tokens are mixed-case.

**`config::tests` (unit, `parse(&str)` / pure helpers)**
1. `parses_a_minimal_single_project_config` - one entry -> 1 project, `default None`, `base_url == "https://api.hetzner.cloud/v1"`, `description None`.
1b. `bom_and_crlf_are_accepted` - `"\u{feff}[[projects]]\r\nname = \"main\"\r\ntoken = \"<64 x a>\"\r\ndescription = \"x\"\r\n"` -> `parse` Ok with name `main` and description `Some("x")`; the same bytes written to a temp file -> `load` Ok (Windows editors / PowerShell `Set-Content -Encoding utf8` output).
2. `parses_the_readme_example` - the §2.3 text as an inline raw-string literal -> 3 projects in file order, `default Some("nb-main")`, descriptions trimmed, `lab` description `None`.
3. `blank_description_becomes_none` - `"   "` -> `None`; `"  DNS  "` -> `Some("DNS")`.
4. `endpoint_is_resolved_and_empty_means_default` - `endpoint = ""` and absent -> default URL; `"http://127.0.0.1:9/"` -> `"http://127.0.0.1:9"`.
4b. `rejects_a_non_https_non_loopback_endpoint_by_origin_only` - `"http://api.hetzner.cloud/v1"` -> Err containing `endpoint must use https://` and `got http://api.hetzner.cloud`, not `/v1`.
5. `rejects_a_missing_or_empty_projects_table` - `""`, comments-only, `projects = []` -> Err containing `at least one [[projects]]` and `[[projects]]` from EXAMPLE.
6. `rejects_unknown_keys_naming_the_expected_ones` - `[[items]]` -> contains ``unknown field `items` `` and `` `projects` `` and `line 1`; `key = ...` inside an entry -> ``unknown field `key` `` and `` `token` ``.
7. `misplaced_top_level_key_gets_the_placement_hint` - `default = "x"` after a `[[projects]]` table -> contains ``unknown field `default` `` and `must come before the first [[projects]]`.
8. `single_bracket_projects_table_gets_the_double_bracket_hint` - `[projects]\nname = "x"\ntoken = "<64 a>"` -> contains `expected a sequence` and `double brackets`; `projects = "<64 a>"` -> same hint.
9. `rejects_a_missing_token_field_by_line` - entry with name only -> ``missing field `token` `` with the correct line.
10. `rejects_an_invalid_name_without_echoing_it` - `"NB-DNS"`, `""`, `"a b"`, 65 x `a`, and `"Ab".repeat(32)` (mixed-case token as the name) -> each Err containing `projects[1]` and `[a-z0-9._-]{1,64}` and NOT containing the offending value.
11. `rejects_a_wrong_length_token_by_index_and_name` - 63- and 65-char tokens in entry 2 -> `projects[2] ("staging")`, `exactly 64 characters`, `(got 63)`/`(got 65)`, token absent.
12. `token_is_not_trimmed` - `" <64 chars> "` -> Err `(got 66)`.
13. `rejects_a_duplicate_name_citing_both_indices` - contains `projects[2]: duplicate name "prod" (already used by projects[1])`.
14. `rejects_a_duplicate_token_naming_both_projects_not_the_token` - contains `identical to projects[1] ("prod")`, token absent.
15. `rejects_an_over_long_description` - 201 chars -> `(got 201)`; exactly 200 -> Ok.
16. `rejects_a_default_naming_no_project_and_lists_the_names` - `default = "prod"` with `lab`, `nb-dns` -> contains `default = "prod"` and `lab, nb-dns` (valid name, echo allowed); `default = ""` and `default = "<"Ab".repeat(32)>"` -> contain `default must match` and `lab, nb-dns` and NOT the value.
17. `accepts_default_with_a_single_project`.
18. `every_rejection_omits_the_token` (port of old t9) - table of every bad config above plus: unterminated string on the token line; token on an unknown-key line (`key = "<token>"`); token as a name; `default = "<token>"`; `projects = "<token>"`; `projects = ["<token>"]`; `<token> = "nb-dns"` (token as a key); `name = { x = "<token>" }` (renders as `map`, regression pin); each with the mixed-case token; assert `!format!("{e:#}").contains(token)` for each. Also `endpoint = "http://<token>@evil.example/v1"`, `endpoint = "ftp://<token>"`, `endpoint = "https-<token>"`, `endpoint = "<token>:443"` and `endpoint = "<token>://x"` (token as userinfo, as host, in a non-URL, as scheme, as scheme with a host); the check is case-insensitive because URL hosts and schemes are lowercased by the parser.
19. `toml_syntax_errors_report_the_line_but_never_the_source_line` - unterminated `token = "bbbb...` on line 3 -> contains `line 3` and not `bbbbbbbb`.
20. `toml_type_errors_never_echo_a_quoted_string_value` - `projects = "<token>"` -> message contains `invalid type: string <redacted>, expected a sequence` and not the token; `<token> = "x"` inside an entry -> contains `unknown field <redacted>` and not the token; `redact_quoted("unknown field `key`, expected one of `name`, `token`")` is returned unchanged (short valid names survive).
21. `debug_of_config_and_project_redacts_the_token` - `format!("{cfg:?}")` contains the name, description and `<redacted>`, not the token.
22. `example_config_parses_once_the_token_is_filled_in` - `EXAMPLE` with the placeholder replaced by a 64-char fake parses (guards the help text against drift).
23. `config_path_prefers_xdg_then_home_dot_config` - `(Some("/x"), Some("/h"))` -> `/x/hetzner-mcp/config.toml`; `(Some(""), Some("/h"))` and `(None, Some("/h"))` -> `/h/.config/hetzner-mcp/config.toml`; `(None, Some(""))` and `(None, None)` -> Err containing `--config`. (Inputs are `Option<PathBuf>`; expected values built with `Path::join` so the test is platform-neutral.)
24. `load` - four one-behaviour tests, temp files removed via a Drop guard (the process-level facts - exit code, empty stdout, absolutization - live in test 38):
    - 24a `load_prefixes_parse_errors_with_the_absolute_path` - temp file with a bad config -> Err chain starts `invalid config file <abs path>` and contains `missing field \`token\``.
    - 24b `load_reads_a_valid_file` - valid file -> Ok(1 project named `main`).
    - 24c `load_reports_a_missing_file_with_example_and_config_hint` - nonexistent path -> Err containing `config file not found`, the path, `[[projects]]` and `--config`.
    - 24d `load_reports_non_utf8_and_other_io_errors_with_the_path` - file starting `FF FE` (UTF-16 BOM) -> Err containing `cannot read config file`, the path and `must be UTF-8`, no example; a directory path -> Err containing `cannot read config file` and the path, neither `must be UTF-8` nor `config file not found`.

**`cli::tests`**
25. `no_args_means_serve_with_the_default_config` - `[]` -> `Serve { config: None }`.
26. `config_flag_takes_the_next_argument` - `["--config", "/p"]` -> `Serve { config: Some("/p") }`; non-UTF-8 `OsString` round-trips.
27. `rejects_a_dangling_or_empty_config_flag_and_unknown_arguments` - `["--config"]` and `["--config", ""]` -> `requires a path`; `["--bogus"]`, `["serve"]`, `["--config=/p"]` -> Err containing `usage:`.
28. `recognises_help_and_version` - `--help`/`-h` -> Help; `--version`/`-V` -> Version; `help_text("/some/path")` contains `/some/path` and `usage:`.

**`hcloud::tests`** (existing three endpoint tests, adapted): `resolve_endpoint(Option<&str>)`; assertions `contains("https://")` and `!contains("HCLOUD")`. All other hcloud tests unchanged.

**`server::multi_project_tests`**
29. `t2_multi_project_schemas_gain_the_project_property` (existing, extended) - additionally asserts `properties.project.enum == ["prod","staging"]`.
30. `t2b_project_property_lists_names_and_descriptions` - `dead_described(&[("nb-dns", Some("DNS zones")), ("nb-main", None)], None)` -> every non-`list_projects` tool's `project.description` contains `nb-dns (DNS zones), nb-main` and `list_projects`; `enum == ["nb-dns","nb-main"]`.
31. `t14_list_projects_reports_names_descriptions_and_default_without_a_token` (existing, extended) - `prod` has `description` string, `staging` has `null`; `is_default` as before; no token substring.
32. `instructions_name_the_config_default_and_never_an_env_var` - pinned 2-project server: `get_info().instructions` contains `config.toml` and every name, not `HCLOUD`; single project: no project sentence.
33. `t1`, `t3`-`t8`, `t12`, `t13`, `t15`, `fix1`, `fix4`, `fix6`, `fix7`, `maybe_annotate_...`, `extract_selector_...` - unchanged behaviour, constructor change only. **Delete** `t6c`, `t9`, `t10`, `t11` (superseded by 11, 14, 18).

**`server::tests`**
34. `env_is_read_only_in_config_rs` - walks `src/**/*.rs`; asserts `std::env::var`, `env::var_os` and `home_dir(` appear only in `src/config.rs` (`args_os` in `src/main.rs` is allowed). No `HCLOUD_*` literal sweep: doc comments may legitimately mention the removed variables in migration notes, and process test 39 proves the binary honours only `XDG_CONFIG_HOME`/home.
35. `mcpb_manifest_tools_match_the_router_names_and_descriptions` (existing, unchanged) - fails until `--sync-manifest` is rerun after the `list_projects` description change (intended). Owns tool count + descriptions only.
36. `mcpb_manifest_launches_with_config_arg_and_no_env` (new, same `tests` module, reads the manifest via `CARGO_MANIFEST_DIR`) - `manifest["server"]["mcp_config"].get("env").is_none()`; `args == ["--config", "${user_config.config_file}"]`; `user_config.config_file`: `matches!(type, "file" | "string")`, `required == true`, `default` ends with `hetzner-mcp/config.toml`; `!manifest_text.contains("HCLOUD_")`. Taking the §8 `string` fallback needs no test edit.
37. `e2e_client::startup_and_handshake_send_no_http_requests` and `initialize_list_and_call_flow_through_the_real_call_tool` (existing) - rebuilt on `Projects::new` only.

**`tests/cli.rs` (integration)**
38. `missing_config_file_exits_non_zero_and_names_the_absolute_path` - (a) `--config <nonexistent temp path>`, stdin closed -> exit 1, stdout empty, stderr contains the path, `[[projects]]`, `--config`; (b) `Command::new(bin).current_dir(&tmp_dir).args(["--config", "missing.toml"])` -> stderr contains `tmp_dir.join("missing.toml").display()` (proves the relative path is absolutized against the child's cwd).
39a. `default_path_is_resolved_under_a_fake_home` - no args; `.env("HOME", tmp1).env("USERPROFILE", tmp1).env_remove("XDG_CONFIG_HOME")` -> exit 1, stderr names `Path::new(&tmp1).join(".config").join("hetzner-mcp").join("config.toml").display()` and `config file not found`; nothing created under tmp1.
39b. `xdg_config_home_overrides_the_home_fallback_and_is_absolutized` - same fake home plus `.env("XDG_CONFIG_HOME", tmp2)` -> names `Path::new(&tmp2).join("hetzner-mcp").join("config.toml").display()` and not `.config`; with `.env("XDG_CONFIG_HOME", "rel").current_dir(&tmp3)` -> names `tmp3.join("rel").join("hetzner-mcp").join("config.toml").display()` (relative XDG is absolutized). Exit 1 each time; nothing created under any of the three dirs.
40. `a_valid_config_serves_the_mcp_handshake_over_stdio` - temp config (two projects, fake tokens, `endpoint = "http://127.0.0.1:9"`), pipe `initialize`, assert stdout contains `"name":"hetzner-mcp"` and `format!("\"version\":\"{}\"", env!("CARGO_PKG_VERSION"))`, stderr does not contain a token, exit 0 after stdin closes.
41. One test per malformed shape, each `--config <file>` -> exit 1 (shared `run_with_config` helper):
    - 41a `a_short_token_exits_non_zero_citing_the_index_not_the_token` - 63-char token -> stderr contains `projects[1]` and not the token.
    - 41b `an_unterminated_token_line_exits_non_zero_without_echoing_it` - unterminated token string -> stderr contains `line 3`, not the token.
    - 41c `a_token_in_place_of_the_projects_array_is_redacted_with_the_bracket_hint` - `projects = "<mixed-case token>"` -> stderr contains `<redacted>` and `double brackets`, not the token.
42. `help_and_version_exit_zero_without_a_config_file` - `--help` prints `usage:` and `config.toml`; `--version` prints `format!("hetzner-mcp {}", env!("CARGO_PKG_VERSION"))`; both exit 0 with `HOME` and `USERPROFILE` pointing at an empty temp dir.

Final step: update the README test-count sentence from `cargo test` output.

## 7. Docs

**README.md**
- Features bullet: `**Multiple projects** - one config file lists any number of projects with a name, token and optional description; every tool takes a `project` argument and `list_projects` shows names, descriptions and a fingerprint.`
- "Configure for Claude Code": step 1 create the config file (`mkdir -p ~/.config/hetzner-mcp && $EDITOR ~/.config/hetzner-mcp/config.toml && chmod 600 ~/.config/hetzner-mcp/config.toml`, then the §2.3 example); step 2 `claude mcp add hetzner -- hetzner-mcp`; the JSON block becomes `{"mcpServers":{"hetzner":{"command":"hetzner-mcp"}}}`.
- Replace "Credentials" + "Environment variables" with **"## Configuration"** (anchor `#configuration`): location rule (XDG, `~/.config`, Windows `%USERPROFILE%\.config\...`, `--config <path>` override - absolute paths recommended because MCP clients set arbitrary cwd; every error shows the resolved absolute path; `hetzner-mcp --help` prints the resolved default path), the §2.3 example, a key table (`default`, `endpoint`, `projects[].name`, `projects[].token`, `projects[].description` with rules/defaults - the `description` row adds: "Shown in every tool's `project` argument and in `list_projects` - keep it to a few words (e.g. `DNS zones`); the 200-char cap is a limit, not a target."), the two syntax gotchas (top-level keys above the first `[[projects]]`; `[[projects]]` with double brackets, one table per project), the startup-rejection list rewritten from §2.4 (index-cited, never quotes a token), the sentence "The server reads this one file and nothing else - no environment variables, no `.env`; it never writes the file or copies tokens anywhere", the file-must-be-UTF-8 note for Windows, and the token-creation link + read-only advice + `chmod 600`.
- "Configure for Claude Desktop" (MCPB): add "If the extension shows *Server disconnected*, the startup error (missing file, bad token length, ...) is in Claude Desktop > Settings > Developer > Open Logs Folder (`mcp-server-hetzner-mcp.log`)." and the hidden-folder hint for the file picker (Cmd+Shift+. on macOS, "Show hidden items" on Windows, or paste the path).
- "## Multiple projects": behaviour only - the `project` argument (enum + `name (description)` list), result wrapping, `default` semantics (read-only tools only), `list_projects` fields (`name`, `description`, `is_default`, `fingerprint`), single project unchanged. Drop Form A/B and the hcloud-CLI 401 paragraphs; one sentence: "`hetzner-mcp` no longer shares `HCLOUD_TOKEN` with the official `hcloud` CLI."
- New **"## Migrating from 0.3.x"**:
  > 0.4.0 stops reading `HCLOUD_TOKEN`, `HCLOUD_PROJECT` and `HCLOUD_ENDPOINT`. Create `~/.config/hetzner-mcp/config.toml` (`chmod 600`) and remove the `env` block from your MCP client config:
  >
  > | 0.3.x | 0.4.0 |
  > |---|---|
  > | `HCLOUD_TOKEN=<token>` | one `[[projects]]` entry; pick any name (the implicit name `default` is gone - e.g. `name = "main"`) |
  > | `HCLOUD_TOKEN=prod=<t>,staging=<t>` | one `[[projects]]` entry per pair |
  > | `HCLOUD_PROJECT=prod` | top-level `default = "prod"` |
  > | `HCLOUD_ENDPOINT=<url>` | top-level `endpoint = "<url>"` |
  >
  > With a single project, calls may still omit `project`. Saved prompts that pass `project = "default"` must use the new name.
- Tools > Projects line: add "and description". Safety notes: replace the `HCLOUD_TOKEN` bullet with "Use read-only tokens where possible; keep `config.toml` at mode 0600 - it is the only place tokens live." "Verify the connection": `echo '...' | hetzner-mcp` and `hetzner-mcp --config /path/to/config.toml`. Development: update test counts.

**CONNECT.md**: launch-contract table `env` row -> `(none) - credentials live in ~/.config/hetzner-mcp/config.toml`; add an `args` note `["--config", "/abs/path"]` only for another file; the intro paragraph says all credentials come from the config file (link `README.md#configuration`) and the server reads no env vars or `.env`. Delete every `env`/`environment`/`envs`/`-e`/`[mcp_servers.hetzner.env]` HCLOUD block in all client snippets (Claude Code, Codex, Gemini, Antigravity, opencode, Crush, Goose, Amp, OpenHands, Cursor, Windsurf, VS Code, Zed, Cline, Roo, Continue) - each becomes command-only; add a shared "Prerequisite: create `~/.config/hetzner-mcp/config.toml` (README > Configuration)" paragraph under Prerequisites. GitHub Actions / container example: write the file from a secret then `chmod 600` (`mkdir -p ~/.config/hetzner-mcp && printf '%s\n' "$HETZNER_MCP_CONFIG" > ~/.config/hetzner-mcp/config.toml && chmod 600 ~/.config/hetzner-mcp/config.toml`, secret holds the whole TOML) or mount it and pass `--config`. "Multiple projects" section collapses to a pointer to README. Gotchas table: drop the "Codex CLI - env is a sub-table" row's env note. Verification one-liner loses `HCLOUD_TOKEN=... |`.

**PRIVACY.md**: line 18 `or the endpoint you set via `HCLOUD_ENDPOINT`` -> `or the `endpoint` you set in config.toml`; lines 39-40 -> `**API token**: read once at startup from `~/.config/hetzner-mcp/config.toml` (or the `--config` path) that you create. The server only reads that file, never writes it, never reads `.env` files or environment variables for credentials, and does not persist tokens anywhere else.` Bump "Last updated".

**docs/multi-project-spec.md**: prepend under the title a 3-line status note: §3 (`HCLOUD_TOKEN`/`HCLOUD_PROJECT` forms) and every `HCLOUD_ENDPOINT` mention are superseded by `docs/config-file-spec.md` as of 0.4.0; §4-§9 (routing, pin, schema injection, echo, list_projects) still describe current behaviour. Body untouched.

**.gitignore**: under the local-secrets block add `/config.toml` and reword the comment to `# Local secrets (config.toml, .env) - never commit`.

## 8. Packaging

**Cargo.toml**: `version = "0.4.0"`, `toml` dependency (§3), `url` comment updated; `Cargo.lock` refreshed by `cargo build`.

**mcpb/manifest.json** (then `node scripts/build-mcpb.mjs --sync-manifest` to regenerate `tools` + `version`):
```json
"server": {
  "type": "binary",
  "entry_point": "bin/hetzner-mcp",
  "mcp_config": {
    "command": "${__dirname}/bin/hetzner-mcp",
    "args": ["--config", "${user_config.config_file}"]
  }
},
"user_config": {
  "config_file": {
    "type": "file",
    "title": "hetzner-mcp config file (config.toml)",
    "description": "TOML file listing your Hetzner projects and API tokens - create it first (README > Configuration). Default: ~/.config/hetzner-mcp/config.toml. Keep it readable only by you (chmod 600). If the file picker does not show the .config folder, press Cmd+Shift+. (macOS) or enable hidden items (Windows), or paste the path.",
    "required": true,
    "default": "${HOME}/.config/hetzner-mcp/config.toml",
    "sensitive": false
  }
}
```
Remove `hcloud_token`/`hcloud_project` and the `env` object. `long_description`: replace the `HCLOUD_TOKEN` sentence with `projects and their tokens live in ~/.config/hetzner-mcp/config.toml, with per-call project routing and a list_projects discovery tool`. MCPB spec (anthropics/mcpb MANIFEST.md, manifest_version 0.2, checked) supports `type: "file"`, `${HOME}` in `default`, and `${user_config.KEY}` in `args`; run `npx @anthropic-ai/mcpb validate mcpb/manifest.json` once. Fallback if Claude Desktop mishandles a `file` default: `type: "string"`, keep `required: true` and `default` - test 36 accepts either type, so the switch is a manifest-only change.

**scripts/build-mcpb.mjs** `toolsFromLiveBinary`: `const dir = mkdtempSync(join(tmpdir(), "hetzner-mcp-manifest-"))`; write `join(dir, "config.toml")` = `[[projects]]\nname = "sync"\ntoken = "${"a".repeat(64)}"\n` with `{ mode: 0o600 }`; `execFileSync(binPath, ["--config", cfgPath], { cwd: ROOT, input: requests, encoding: "utf8" })` (plain `process.env`, no `HCLOUD_TOKEN`); `rmSync(dir, { recursive: true, force: true })` in `finally`. Keep the "single project so no `project` property leaks" comment.

**.github/workflows**: `ci.yml` unchanged (`tests/cli.rs` runs under `cargo test --locked`; the msrv job's `cargo check --all-targets --locked` covers the new dev code). `release-mcpb.yml` runs `cargo test --locked` on ubuntu, macos **and windows** before building the bundle - which is why `tests/cli.rs` sets `USERPROFILE` alongside `HOME` and builds expected paths with `Path::join` (§3, §6). It sets no `HCLOUD_*` (verified) - unchanged.

## 9. Explicit YAGNI cuts

- No env-var fallback, no `.env`, no `HETZNER_MCP_CONFIG` env override, no "HCLOUD_TOKEN is set but ignored" sniffing (D1).
- No `clap`/`dirs`/`directories`/`home`/`tempfile`/`assert_cmd` crates.
- No `--config=<path>`, no repeated-flag detection, no positional args, no exit code other than 0/1, no subcommands (`init`, `check`, `validate`).
- No file-permission check or refusal, no ownership/parent-dir check, no Windows ACL logic - `chmod 600` documented only.
- No directory/FIFO/size/UTF-8 pre-checks beyond what `fs::read_to_string` reports (the `InvalidData` hint only rewords an error std already produced); `toml` stays depth-bounded (no `unbounded` feature).
- No token trimming or normalisation; no printable-ASCII rule; no API probe at startup (zero-HTTP invariant stays).
- No XDG "relative value is ignored" rule (D1 says set-and-non-empty); no platform-native dirs (macOS Library, %APPDATA%).
- No per-project endpoint, read-only flag, tags or tool allowlists; no `[projects.<name>]` map shape or `default = true` booleans.
- No hot reload, no scaffolding/writing of the config file, no schema versioning key, no JSON Schema export, no TOML serialization (`display` feature off).
- No description sanitising beyond trim + 200-char cap.
- No `EXAMPLE_FULL` constant, no `docs/example-config.toml` - the full example lives in README plus one inline test fixture.
- No CHANGELOG file (D6).
- No renaming of the test project `"default"` in `test_support`.

## 10. Open risks

1. **Hard breaking change**: every 0.3.x install fails at startup until the file exists; mitigated by the missing-file message (path + example + `--config` hint), the README migration table, and the Claude Desktop log-location sentence. Saved prompts using `project = "default"` break (D8).
2. **Token leak via `toml::de::Error`**: `Display` reprints the source line (never used); `message()` echoes quoted string values (`Unexpected::Str`) and unknown-field keys (`deny_unknown_fields`) - closed by the §2.4 step-1 redaction (`"..."`/`` `...` `` segments that fail `valid_name` or are >= 32 chars become `<redacted>`) and pinned by tests 18, 19, 20, 41. A future `{e}` refactor or removal of `redact_quoted` regresses only if those tests go too. The pairing is escape-aware (a `\"` inside serde's `Debug` rendering does not close a segment), an unterminated segment is redacted whole, a one-character literal such as `` `"` `` is kept, and a second pass redacts every run of 32+ ASCII alphanumerics regardless of quoting (round-3 verifier finding: an escaped quote or a backtick inside a key mis-aligned the pairing). Residual: an unquoted scalar (`invalid type: integer `123``) is echoed; a token is never an unquoted TOML scalar.
3. **Schema payload growth**: with N projects the `project` description (names + up to 200-char descriptions) and enum repeat on 92 tools in `tools/list`; 5 projects x ~220 chars x 92 tools is roughly 100 KB per handshake. README tells operators to keep descriptions to a few words. Fallback if it bites: names-only in the schema, descriptions in `get_info`/`list_projects`.
4. **`enum` and strict clients**: a client that validates enums rejects an unknown project before `resolve()`'s friendlier message; accepted for the accuracy gain.
5. **TOML syntax traps**: `default`/`endpoint` below a `[[projects]]` table becomes an unknown field of that project; `[projects]` with single brackets is a map, not an array. Both get a targeted hint (§2.4 steps 2-3) and a comment in the example.
6. **MCPB `file` user_config**: `${HOME}` default and `required: true` are per spec but untested against the current Claude Desktop build; native file pickers hide `.config` (hint added to the description); fallback documented in §8 and tolerated by test 36. `compatibility.platforms` is `["darwin"]` while the release workflow builds three platforms - pre-existing, out of scope.
7. **Windows default path** `%USERPROFILE%\.config\hetzner-mcp\config.toml` is unconventional; `--config` is the escape hatch.
8. **Durable secret on disk**: tokens move from a process-scoped env var to a file every process running as the user can read; no permission enforcement (cut). Revisit as an opt-in check if operators ask.
9. **`home_dir()` semantics**: Rust < 1.90 returns `Some("")` for empty `HOME` on unix (filtered); Windows reads `USERPROFILE` only - `HOME` is ignored (Rust 1.85 changelog) - then the profile API. `tests/cli.rs` therefore sets both variables per spawned process and never touches the real `~/.config` (D7); the Windows release job runs the suite.
10. **Manifest and README drift** during implementation: run `node scripts/build-mcpb.mjs --sync-manifest` (needs node + a debug build) and update the test-count sentence last, after the suite is green. The README example and test 2's fixture are two copies of the same TOML; test 2 pins the grammar, the README copy is prose.
11. **`Projects` field rename** touches `test_support`, `e2e_client` and most `multi_project_tests`; mechanical, contained by keeping `resolve()`'s `(String, String)` return and routing all construction through `Projects::new`.
12. **Redaction over-reach**: a legitimately long (>= 32 chars) unknown key in a user's file is shown as `<redacted>` rather than by name; the line number still locates it. Accepted - the alternative is a token on stderr.

---

## Appendix: critic round - resolutions

Accepted (spec changed): toml `message()` echo of quoted strings and unknown-field keys (two duplicate blocking findings -> §2.4 redaction step, tests 18/20/41, §10.2); Windows `HOME` vs `USERPROFILE` in integration tests (two duplicate blocking findings -> §3 tests/cli.rs, §6 tests 39/42, §8, §10.9); rule 7 echoing `default` (gated on `valid_name`, tests 16/18); mixed-case swap fixtures (§6 preamble, tests 10/16/18); `env!("CARGO_PKG_VERSION")` in tests 40/42; relative `--config` absolutization test (test 38b); split manifest launch-contract test out of the tool-drift test (tests 35/36); drop `EXAMPLE_FULL` (§2.3, §9); e2e tests no longer call `config::parse` (§3); drop the separate missing-file unit test and the `HCLOUD_*` literal sweep (tests 24/34); API shape (`help_text(&str)`, `names_list()` via `names()`, `config_path(Option<PathBuf>, Option<PathBuf>)`); `[projects]` single-bracket hint (§2.4 step 3, test 8); absolutize the default path too (§2.1 step 3, test 39 relative-XDG case); MCPB hidden-folder hint and Claude Desktop log-location sentence (§7, §8); `--config ""` argv error (§2.4, test 27); manifest test tolerates the `string` fallback (test 36); README description-length guidance (§7, §10.3); `InvalidData` UTF-8 hint (§2.4 file layer).

### Rejected critiques

- Finding 8's alternative of a `docs/example-config.toml` read via `include_str!`: rejected - it adds a third copy of the example and a file the binary never uses; README plus one inline test fixture is enough.
- Finding 2's alternative of gating the fake-`HOME` sub-case behind `#[cfg(unix)]`: rejected - setting `USERPROFILE` too keeps the Windows release job covering the default-path logic for free.
- Finding 1's narrow fix (strip only the `invalid type: string "..."` shape, fallback `invalid type: string`): rejected in favour of the general redaction rule, because the second blocking finding showed `unknown field` echoes the key through the same door and a shape-by-shape list would miss the next one.
- Finding 10's option to keep the `HCLOUD_*` literal sweep "restricted to non-comment lines": rejected - the sweep is dropped entirely; the structural env-read check plus process test 39 already prove the behaviour, and a comment filter is test machinery for no behaviour.
- Finding 12's suggestion to redact *any* 32+-char quoted segment as an alternative to the grammar check: not rejected but merged - the rule is "fails `valid_name` **or** >= 32 chars", since either alone leaves a gap (uppercase short strings vs. all-lowercase 64-char values).
