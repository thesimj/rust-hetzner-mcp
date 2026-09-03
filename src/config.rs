//! The config file: `<config dir>/hetzner-mcp/config.toml` (or `--config
//! <path>`). Owns file location, the TOML shape, every validation rule and
//! the redaction of parser messages - a token can never reach stderr from
//! here. No rmcp, no reqwest; `server::run` consumes the validated [`Config`].

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// The minimal example shown when the file is missing or lists no project.
pub const EXAMPLE: &str = "[[projects]]\n\
name = \"main\"\n\
token = \"<64-character API token from Hetzner Cloud Console -> Security -> API tokens>\"\n\
# description = \"optional, shown to the model\"";

/// One configured Hetzner project. `Debug` redacts the token by hand -
/// never derive it.
#[derive(Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub token: String,
    pub description: Option<String>,
}

impl std::fmt::Debug for Project {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Project")
            .field("name", &self.name)
            .field("token", &"<redacted>")
            .field("description", &self.description)
            .finish()
    }
}

/// A validated config: at least one project, unique names, unique tokens,
/// `default` names a configured project, `base_url` already checked.
#[derive(Clone, PartialEq, Eq)]
pub struct Config {
    pub base_url: String,
    pub default: Option<String>,
    pub projects: Vec<Project>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("base_url", &self.base_url)
            .field("default", &self.default)
            .field("projects", &self.projects)
            .finish()
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    default: Option<String>,
    endpoint: Option<String>,
    #[serde(default)]
    projects: Vec<RawProject>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProject {
    name: String,
    token: String,
    description: Option<String>,
}

/// Parse and validate config text. Pure: no file or environment access.
pub fn parse(text: &str) -> Result<Config> {
    let raw: RawConfig = toml::from_str(text).map_err(|e| render_toml_error(&e, text))?;
    validate(raw)
}

/// Turn a `toml::de::Error` into a message that locates the problem by line
/// without reprinting the source. `Display` on the error is never used - it
/// quotes the offending line, which may be a token line.
fn render_toml_error(e: &toml::de::Error, text: &str) -> anyhow::Error {
    let mut message = redact_quoted(e.message());
    if message.starts_with("unknown field `default`")
        || message.starts_with("unknown field `endpoint`")
    {
        message.push_str(" (top-level keys must come before the first [[projects]] table)");
    }
    if message.ends_with("expected a sequence") {
        message.push_str(
            " (projects must be written as [[projects]] tables - double brackets, one table per project)",
        );
    }
    match e.span() {
        Some(span) => {
            // Count bytes, not a `str` slice: a span offset inside a multibyte
            // char would make slicing panic with the source text in the message.
            let start = span.start.min(text.len());
            let line = text.as_bytes()[..start]
                .iter()
                .filter(|&&b| b == b'\n')
                .count()
                + 1;
            anyhow::anyhow!("line {line}: {message}")
        }
        None => anyhow::anyhow!("{message}"),
    }
}

/// Replace every `"..."` or `` `...` `` segment of a parser message whose
/// content fails [`valid_name`] or is 32+ characters long with `<redacted>`.
/// serde echoes string values (`invalid type: string "<value>"`) and unknown
/// keys (`unknown field `<key>``), either of which can be a pasted token; our
/// own field names are short valid names and survive untouched, as does a
/// one-character literal such as `` `"` `` (it can never be a token). A `"`
/// that serde's `Debug` rendering escaped as `\"` does not close a segment,
/// an unterminated segment is redacted whole, and [`redact_long_runs`] then
/// catches any shape this pairing does not model.
fn redact_quoted(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(open) = rest.find(['"', '`']) {
        let delim = rest.as_bytes()[open] as char;
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = find_closing(after, delim) else {
            out.push_str("<redacted>");
            return redact_long_runs(&out);
        };
        let content = &after[..close];
        let len = content.chars().count();
        if len == 1 || (valid_name(content) && len < 32) {
            out.push(delim);
            out.push_str(content);
            out.push(delim);
        } else {
            out.push_str("<redacted>");
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    redact_long_runs(&out)
}

/// Byte index of the `delim` that closes a segment. For `"` a quote preceded
/// by an odd number of backslashes is an escaped inner quote (serde's `Debug`
/// output), not the close. Backticks are never escaped by toml/serde.
fn find_closing(after: &str, delim: char) -> Option<usize> {
    let mut backslashes = 0usize;
    for (i, c) in after.char_indices() {
        if c == delim && (delim != '"' || backslashes.is_multiple_of(2)) {
            return Some(i);
        }
        backslashes = if c == '\\' { backslashes + 1 } else { 0 };
    }
    None
}

/// Defence in depth: replace every run of 32+ ASCII alphanumerics with
/// `<redacted>`. No toml or serde message contains such a run, and a Hetzner
/// token is 64 of them, so this closes any echo path the quote pairing misses.
fn redact_long_runs(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut run = String::new();
    for c in message.chars() {
        if c.is_ascii_alphanumeric() {
            run.push(c);
            continue;
        }
        flush_run(&mut out, &mut run);
        out.push(c);
    }
    flush_run(&mut out, &mut run);
    out
}

fn flush_run(out: &mut String, run: &mut String) {
    if run.len() >= 32 {
        out.push_str("<redacted>");
    } else {
        out.push_str(run);
    }
    run.clear();
}

fn validate(raw: RawConfig) -> Result<Config> {
    if raw.projects.is_empty() {
        bail!("config must define at least one [[projects]] entry, for example:\n\n{EXAMPLE}");
    }
    let mut projects: Vec<Project> = Vec::with_capacity(raw.projects.len());
    for (idx, entry) in raw.projects.into_iter().enumerate() {
        let i = idx + 1;
        if !valid_name(&entry.name) {
            bail!("projects[{i}]: name must match [a-z0-9._-]{{1,64}}");
        }
        let name = entry.name;
        if let Some(j) = projects.iter().position(|p| p.name == name) {
            bail!(
                "projects[{i}]: duplicate name \"{name}\" (already used by projects[{}])",
                j + 1
            );
        }
        if entry.token.len() != TOKEN_LEN {
            bail!(
                "projects[{i}] (\"{name}\"): token must be exactly {TOKEN_LEN} characters (got {})",
                entry.token.len()
            );
        }
        if let Some(j) = projects.iter().position(|p| p.token == entry.token) {
            bail!(
                "projects[{i}] (\"{name}\"): token is identical to projects[{}] (\"{}\") - \
                 every project needs its own token",
                j + 1,
                projects[j].name
            );
        }
        let description = entry
            .description
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(str::to_string);
        if let Some(d) = &description
            && d.chars().count() > MAX_DESCRIPTION_CHARS
        {
            bail!(
                "projects[{i}] (\"{name}\"): description must be at most {MAX_DESCRIPTION_CHARS} \
                 characters (got {})",
                d.chars().count()
            );
        }
        projects.push(Project {
            name,
            token: entry.token,
            description,
        });
    }
    if let Some(d) = &raw.default {
        let names = projects
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        if !valid_name(d) {
            bail!(
                "default must match [a-z0-9._-]{{1,64}} and name a configured project; \
                 configured projects: {names}"
            );
        }
        if !projects.iter().any(|p| &p.name == d) {
            bail!("default = \"{d}\" names no configured project; configured projects: {names}");
        }
    }
    let base_url = crate::hcloud::resolve_endpoint(raw.endpoint.as_deref())?;
    Ok(Config {
        base_url,
        default: raw.default,
        projects,
    })
}

const TOKEN_LEN: usize = 64;
const MAX_DESCRIPTION_CHARS: usize = 200;

/// The project-name grammar `[a-z0-9._-]{1,64}`. This is a name grammar, not
/// a token detector: callers echo a value only after it passes here, so a
/// token pasted into `name` or `default` is never printed.
fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

/// `$XDG_CONFIG_HOME/hetzner-mcp/config.toml` if set and non-empty, else
/// `<home>/.config/hetzner-mcp/config.toml`. Pure: the caller supplies both.
pub fn config_path(xdg_config_home: Option<PathBuf>, home: Option<PathBuf>) -> Result<PathBuf> {
    let non_empty = |p: PathBuf| (!p.as_os_str().is_empty()).then_some(p);
    let config_dir = match (
        xdg_config_home.and_then(non_empty),
        home.and_then(non_empty),
    ) {
        (Some(xdg), _) => xdg,
        (None, Some(home)) => home.join(".config"),
        (None, None) => bail!(
            "cannot locate the config file: neither XDG_CONFIG_HOME nor a home directory is set; \
             pass --config <path>"
        ),
    };
    Ok(config_dir.join("hetzner-mcp").join("config.toml"))
}

/// [`config_path`] fed from the process environment - the only environment
/// reads in this crate (pinned by `server::tests::env_is_read_only_in_config_rs`).
pub fn default_path() -> Result<PathBuf> {
    config_path(
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::home_dir(),
    )
}

/// Read and parse the config file at `override_path`, or the default path.
/// Every message names the absolute path that was actually tried.
pub fn load(override_path: Option<&Path>) -> Result<Config> {
    let path = match override_path {
        Some(p) => p.to_path_buf(),
        None => default_path()?,
    };
    let abs = std::path::absolute(&path)
        .with_context(|| format!("cannot resolve config file path {}", path.display()))?;
    let shown = abs.display();
    // Checked up front: reading a directory reports EISDIR on Unix but
    // NotFound on Windows, and the not-found hint would be wrong here.
    if abs.is_dir() {
        bail!(
            "config path {shown} is a directory, not a file; \
             pass the config.toml inside it with: hetzner-mcp --config <path>"
        );
    }
    let text = match std::fs::read_to_string(&abs) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
            "config file not found: {shown}\n\n\
             create it with at least one project, for example:\n\n{EXAMPLE}\n\n\
             or pass another file with: hetzner-mcp --config <path>"
        ),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => bail!(
            "cannot read config file {shown}: {e} (the file must be UTF-8; on Windows use \
             Set-Content -Encoding utf8 or an editor that saves UTF-8)"
        ),
        Err(e) => bail!("cannot read config file {shown}: {e}"),
    };
    parse(&text).with_context(|| format!("invalid config file {shown}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_URL: &str = "https://api.hetzner.cloud/v1";

    fn tok(c: char) -> String {
        std::iter::repeat_n(c, 64).collect()
    }

    /// A mixed-case token for every swap scenario: 64 lowercase chars would
    /// be a legal name and make a redaction test pass trivially.
    fn mixed_token() -> String {
        "Ab".repeat(32)
    }

    fn single(name: &str, token: &str) -> String {
        format!("[[projects]]\nname = \"{name}\"\ntoken = \"{token}\"\n")
    }

    fn two(default: Option<&str>) -> String {
        let head = default.map_or(String::new(), |d| format!("default = \"{d}\"\n"));
        format!(
            "{head}{}{}",
            single("lab", &tok('a')),
            single("nb-dns", &tok('b'))
        )
    }

    fn err_of(text: &str) -> String {
        format!("{:#}", parse(text).unwrap_err())
    }

    // 1
    #[test]
    fn parses_a_minimal_single_project_config() {
        let cfg = parse(&single("main", &tok('a'))).unwrap();
        assert_eq!(cfg.projects.len(), 1);
        assert_eq!(cfg.projects[0].name, "main");
        assert_eq!(cfg.projects[0].token, tok('a'));
        assert_eq!(cfg.projects[0].description, None);
        assert_eq!(cfg.default, None);
        assert_eq!(cfg.base_url, DEFAULT_URL);
    }

    // 1b - Windows editors and PowerShell `Set-Content -Encoding utf8` write a
    // UTF-8 BOM and CRLF line endings; `toml` strips the BOM and treats CRLF
    // as a newline. Pinned so a `toml` major bump cannot silently turn a
    // Windows-authored file into an opaque `line 1: ...` error.
    #[test]
    fn bom_and_crlf_are_accepted() {
        let text = format!(
            "\u{feff}[[projects]]\r\nname = \"main\"\r\ntoken = \"{}\"\r\ndescription = \"x\"\r\n",
            tok('a')
        );
        let cfg = parse(&text).unwrap();
        assert_eq!(cfg.projects.len(), 1);
        assert_eq!(cfg.projects[0].name, "main");
        assert_eq!(cfg.projects[0].token, tok('a'));
        assert_eq!(cfg.projects[0].description.as_deref(), Some("x"));

        // The file layer too: the BOM bytes are EF BB BF on disk.
        let file = TempFile::new(text.as_bytes());
        let cfg = load(Some(&file.0)).unwrap();
        assert_eq!(cfg.projects[0].name, "main");
        assert_eq!(cfg.projects[0].description.as_deref(), Some("x"));
    }

    // 2
    #[test]
    fn parses_the_readme_example() {
        let text = r#"# ~/.config/hetzner-mcp/config.toml
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
"#;
        let cfg = parse(text).unwrap();
        assert_eq!(cfg.default.as_deref(), Some("nb-main"));
        assert_eq!(cfg.base_url, DEFAULT_URL);
        let names: Vec<&str> = cfg.projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["nb-main", "nb-dns", "lab"]);
        assert_eq!(
            cfg.projects[0].description.as_deref(),
            Some("main infra: web servers, load balancer, volumes")
        );
        assert_eq!(
            cfg.projects[1].description.as_deref(),
            Some("DNS zones only (read-only token)")
        );
        assert_eq!(cfg.projects[2].description, None);
        assert_eq!(cfg.projects[2].token, tok('2'));
    }

    // 3
    #[test]
    fn blank_description_becomes_none() {
        let blank = format!("{}description = \"   \"\n", single("main", &tok('a')));
        assert_eq!(parse(&blank).unwrap().projects[0].description, None);
        let padded = format!("{}description = \"  DNS  \"\n", single("main", &tok('a')));
        assert_eq!(
            parse(&padded).unwrap().projects[0].description.as_deref(),
            Some("DNS")
        );
    }

    // 4
    #[test]
    fn endpoint_is_resolved_and_empty_means_default() {
        let base = single("main", &tok('a'));
        assert_eq!(parse(&base).unwrap().base_url, DEFAULT_URL);
        assert_eq!(
            parse(&format!("endpoint = \"\"\n{base}")).unwrap().base_url,
            DEFAULT_URL
        );
        assert_eq!(
            parse(&format!("endpoint = \"http://127.0.0.1:9/\"\n{base}"))
                .unwrap()
                .base_url,
            "http://127.0.0.1:9"
        );
    }

    // 4b
    #[test]
    fn rejects_a_non_https_non_loopback_endpoint_by_origin_only() {
        let err = err_of(&format!(
            "endpoint = \"http://api.hetzner.cloud/v1\"\n{}",
            single("main", &tok('a'))
        ));
        assert!(err.contains("endpoint must use https://"), "{err}");
        assert!(err.contains("got http://api.hetzner.cloud"), "{err}");
        assert!(!err.contains("/v1"), "{err}");
    }

    // 5
    #[test]
    fn rejects_a_missing_or_empty_projects_table() {
        for text in ["", "# nothing here\n", "projects = []\n"] {
            let err = err_of(text);
            assert!(err.contains("at least one [[projects]]"), "{text:?}: {err}");
            assert!(err.contains("[[projects]]\nname = \"main\""), "{err}");
        }
    }

    // 6
    #[test]
    fn rejects_unknown_keys_naming_the_expected_ones() {
        let err = err_of(&format!(
            "[[items]]\nname = \"x\"\ntoken = \"{}\"\n",
            tok('a')
        ));
        assert!(err.contains("unknown field `items`"), "{err}");
        assert!(err.contains("`projects`"), "{err}");
        assert!(err.contains("line 1"), "{err}");

        let err = err_of(&format!(
            "[[projects]]\nname = \"x\"\nkey = \"{}\"\n",
            tok('a')
        ));
        assert!(err.contains("unknown field `key`"), "{err}");
        assert!(err.contains("`token`"), "{err}");
    }

    // 7
    #[test]
    fn misplaced_top_level_key_gets_the_placement_hint() {
        let err = err_of(&format!("{}default = \"x\"\n", single("x", &tok('a'))));
        assert!(err.contains("unknown field `default`"), "{err}");
        assert!(
            err.contains("must come before the first [[projects]]"),
            "{err}"
        );
    }

    // 8
    #[test]
    fn single_bracket_projects_table_gets_the_double_bracket_hint() {
        let err = err_of(&format!(
            "[projects]\nname = \"x\"\ntoken = \"{}\"\n",
            tok('a')
        ));
        assert!(err.contains("expected a sequence"), "{err}");
        assert!(err.contains("double brackets"), "{err}");

        let err = err_of(&format!("projects = \"{}\"\n", tok('a')));
        assert!(err.contains("expected a sequence"), "{err}");
        assert!(err.contains("double brackets"), "{err}");
    }

    // 9
    #[test]
    fn rejects_a_missing_token_field_by_line() {
        let err = err_of("# comment\n\n[[projects]]\nname = \"x\"\n");
        assert!(err.contains("missing field `token`"), "{err}");
        assert!(err.contains("line 3"), "{err}");
    }

    // 10
    #[test]
    fn rejects_an_invalid_name_without_echoing_it() {
        let long = "a".repeat(65);
        for bad in ["NB-DNS", "", "a b", long.as_str(), &mixed_token()] {
            let err = err_of(&single(bad, &tok('a')));
            assert!(err.contains("projects[1]"), "{bad:?}: {err}");
            assert!(err.contains("[a-z0-9._-]{1,64}"), "{bad:?}: {err}");
            if !bad.is_empty() {
                assert!(!err.contains(bad), "{bad:?} echoed: {err}");
            }
        }
    }

    // 11
    #[test]
    fn rejects_a_wrong_length_token_by_index_and_name() {
        for (len, hint) in [(63, "(got 63)"), (65, "(got 65)")] {
            let token = "c".repeat(len);
            let err = err_of(&format!(
                "{}{}",
                single("prod", &tok('a')),
                single("staging", &token)
            ));
            assert!(err.contains("projects[2] (\"staging\")"), "{err}");
            assert!(err.contains("exactly 64 characters"), "{err}");
            assert!(err.contains(hint), "{err}");
            assert!(!err.contains(&token), "{err}");
        }
    }

    // 12
    #[test]
    fn token_is_not_trimmed() {
        let err = err_of(&single("main", &format!(" {} ", tok('a'))));
        assert!(err.contains("(got 66)"), "{err}");
    }

    // 13
    #[test]
    fn rejects_a_duplicate_name_citing_both_indices() {
        let err = err_of(&format!(
            "{}{}",
            single("prod", &tok('a')),
            single("prod", &tok('b'))
        ));
        assert!(
            err.contains("projects[2]: duplicate name \"prod\" (already used by projects[1])"),
            "{err}"
        );
    }

    // 14
    #[test]
    fn rejects_a_duplicate_token_naming_both_projects_not_the_token() {
        let err = err_of(&format!(
            "{}{}",
            single("prod", &tok('a')),
            single("staging", &tok('a'))
        ));
        assert!(err.contains("projects[2] (\"staging\")"), "{err}");
        assert!(err.contains("identical to projects[1] (\"prod\")"), "{err}");
        assert!(!err.contains(&tok('a')), "{err}");
    }

    // 15
    #[test]
    fn rejects_an_over_long_description() {
        let base = single("main", &tok('a'));
        let err = err_of(&format!("{base}description = \"{}\"\n", "d".repeat(201)));
        assert!(err.contains("(got 201)"), "{err}");
        assert!(err.contains("at most 200 characters"), "{err}");
        let ok = parse(&format!("{base}description = \"{}\"\n", "d".repeat(200))).unwrap();
        assert_eq!(
            ok.projects[0].description.as_deref().map(str::len),
            Some(200)
        );
    }

    // 16
    #[test]
    fn rejects_a_default_naming_no_project_and_lists_the_names() {
        let err = err_of(&two(Some("prod")));
        assert!(err.contains("default = \"prod\""), "{err}");
        assert!(err.contains("lab, nb-dns"), "{err}");

        for bad in ["", &mixed_token()] {
            let err = err_of(&two(Some(bad)));
            assert!(err.contains("default must match"), "{bad:?}: {err}");
            assert!(err.contains("lab, nb-dns"), "{bad:?}: {err}");
            if !bad.is_empty() {
                assert!(!err.contains(bad), "{bad:?} echoed: {err}");
            }
        }
    }

    // 17
    #[test]
    fn accepts_default_with_a_single_project() {
        let cfg = parse(&format!(
            "default = \"main\"\n{}",
            single("main", &tok('a'))
        ))
        .unwrap();
        assert_eq!(cfg.default.as_deref(), Some("main"));
    }

    // 18
    #[test]
    fn every_rejection_omits_the_token() {
        let t = mixed_token();
        let ok = single("prod", &t);
        let cases = [
            format!("{}{}", single("prod", &t), single("prod", &tok('a'))), // duplicate name
            format!("{}{}", single("prod", &t), single("staging", &t)),     // duplicate token
            single("prod", &format!("{t}x")),                               // 65 chars
            single("PROD", &t),                                             // bad name
            format!("{ok}default = \"x\"\n"),                               // misplaced default
            format!("default = \"nope\"\n{ok}"),                            // unknown default
            format!("[[projects]]\nname = \"prod\"\ntoken = \"{t}\n"),      // unterminated string
            format!("[[projects]]\nname = \"prod\"\nkey = \"{t}\"\n"),      // token on unknown key
            single(&t, &tok('a')),                                          // token as a name
            format!("default = \"{t}\"\n{ok}"),                             // token as default
            format!("projects = \"{t}\"\n"),                                // token as projects
            format!("projects = [\"{t}\"]\n"),                              // token in the array
            format!("[[projects]]\n{t} = \"nb-dns\"\ntoken = \"{t}\"\n"),   // token as a key
            format!("[[projects]]\nname = {{ x = \"{t}\" }}\ntoken = \"{t}\"\n"), // token in a map
            format!("endpoint = \"http://{t}@evil.example/v1\"\n{ok}"),     // token as userinfo
            format!("endpoint = \"ftp://{t}\"\n{ok}"),                      // token as host
            format!("endpoint = \"https-{t}\"\n{ok}"),                      // token in a non-URL
            format!("endpoint = \"{t}:443\"\n{ok}"),                        // token as scheme
            format!("endpoint = \"{t}://x\"\n{ok}"), // token as scheme + host
            format!("projects = \"\\\"{t}\"\n"),     // token behind an escaped quote
            format!("[[projects]]\nname = \"prod\"\n\"x`{t}\" = \"x\"\n"), // backtick in a key
        ];
        for text in &cases {
            let err = err_of(text);
            assert!(!err.contains(&t), "leaked token in: {err}\nconfig:\n{text}");
            // URL hosts are lowercased by the parser; a case-folded echo leaks too.
            assert!(
                !err.to_lowercase().contains(&t.to_lowercase()),
                "leaked token in: {err}\nconfig:\n{text}"
            );
        }
    }

    // 19
    #[test]
    fn toml_syntax_errors_report_the_line_but_never_the_source_line() {
        let err = err_of(&format!(
            "[[projects]]\nname = \"prod\"\ntoken = \"{}\n",
            tok('b')
        ));
        assert!(err.contains("line 3"), "{err}");
        assert!(!err.contains("bbbbbbbb"), "{err}");
    }

    // 20
    #[test]
    fn toml_type_errors_never_echo_a_quoted_string_value() {
        let t = mixed_token();
        let err = err_of(&format!("projects = \"{t}\"\n"));
        assert!(
            err.contains("invalid type: string <redacted>, expected a sequence"),
            "{err}"
        );
        assert!(!err.contains(&t), "{err}");

        let err = err_of(&format!("[[projects]]\nname = \"prod\"\n{t} = \"x\"\n"));
        assert!(err.contains("unknown field <redacted>"), "{err}");
        assert!(!err.contains(&t), "{err}");

        let short = "unknown field `key`, expected one of `name`, `token`";
        assert_eq!(redact_quoted(short), short);

        // serde's Debug rendering escapes an inner quote; the escape must not
        // close the segment early and leave the token in the tail.
        let err = err_of(&format!("projects = \"\\\"{t}\"\n"));
        assert!(err.contains("<redacted>"), "{err}");
        assert!(!err.contains(&t), "{err}");

        // One-character literals are toml's own expectations, never a token.
        let literals = "invalid basic string, expected `\"`, `'`, `\\`";
        assert_eq!(redact_quoted(literals), literals);

        // A bare 64-char run survives no pairing at all and is still caught.
        let bare = format!("unknown field `x`{t}`, expected `name`");
        assert!(!redact_quoted(&bare).contains(&t));
    }

    // 21
    #[test]
    fn debug_of_config_and_project_redacts_the_token() {
        let cfg = parse(&format!(
            "{}description = \"DNS zones\"\n",
            single("nb-dns", &tok('a'))
        ))
        .unwrap();
        let dbg = format!("{cfg:?}");
        assert!(dbg.contains("nb-dns"), "{dbg}");
        assert!(dbg.contains("DNS zones"), "{dbg}");
        assert!(dbg.contains("<redacted>"), "{dbg}");
        assert!(!dbg.contains(&tok('a')), "{dbg}");
        let dbg = format!("{:?}", cfg.projects[0]);
        assert!(!dbg.contains(&tok('a')), "{dbg}");
    }

    // 22
    #[test]
    fn example_config_parses_once_the_token_is_filled_in() {
        let placeholder = EXAMPLE
            .split('"')
            .find(|s| s.starts_with('<'))
            .expect("EXAMPLE has a <placeholder> token");
        let text = EXAMPLE.replace(placeholder, &tok('a'));
        let cfg = parse(&text).unwrap();
        assert_eq!(cfg.projects[0].name, "main");
    }

    // 23
    #[test]
    fn config_path_prefers_xdg_then_home_dot_config() {
        let xdg = Path::new("/x").join("hetzner-mcp").join("config.toml");
        let home = Path::new("/h")
            .join(".config")
            .join("hetzner-mcp")
            .join("config.toml");
        let p = |s: &str| Some(PathBuf::from(s));
        assert_eq!(config_path(p("/x"), p("/h")).unwrap(), xdg);
        assert_eq!(config_path(p(""), p("/h")).unwrap(), home);
        assert_eq!(config_path(None, p("/h")).unwrap(), home);
        for (x, h) in [(None, p("")), (None, None)] {
            let err = config_path(x, h).unwrap_err().to_string();
            assert!(err.contains("--config"), "{err}");
        }
    }

    struct TempFile(PathBuf);

    impl TempFile {
        fn new(contents: impl AsRef<[u8]>) -> Self {
            static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "hetzner-mcp-config-test-{}-{n}.toml",
                std::process::id()
            ));
            std::fs::write(&path, contents).unwrap();
            Self(path)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    // 24a
    #[test]
    fn load_prefixes_parse_errors_with_the_absolute_path() {
        let bad = TempFile::new("[[projects]]\nname = \"x\"\n");
        let abs = std::path::absolute(&bad.0).unwrap();
        let err = load(Some(&bad.0)).unwrap_err();
        assert!(
            err.to_string()
                .starts_with(&format!("invalid config file {}", abs.display())),
            "{err:#}"
        );
        assert!(
            format!("{err:#}").contains("missing field `token`"),
            "{err:#}"
        );
    }

    // 24b
    #[test]
    fn load_reads_a_valid_file() {
        let good = TempFile::new(single("main", &tok('a')));
        let cfg = load(Some(&good.0)).unwrap();
        assert_eq!(cfg.projects.len(), 1);
        assert_eq!(cfg.projects[0].name, "main");
    }

    // 24c
    #[test]
    fn load_reports_a_missing_file_with_example_and_config_hint() {
        let missing = std::env::temp_dir().join(format!(
            "hetzner-mcp-config-test-{}-missing.toml",
            std::process::id()
        ));
        let err = format!("{:#}", load(Some(&missing)).unwrap_err());
        assert!(err.contains("config file not found"), "{err}");
        assert!(err.contains(&missing.display().to_string()), "{err}");
        assert!(err.contains("[[projects]]"), "{err}");
        assert!(err.contains("--config"), "{err}");
    }

    // 24d
    #[test]
    fn load_reports_non_utf8_and_directory_paths_with_the_path() {
        // UTF-16 with a BOM, as Windows PowerShell 5.1 `Out-File` writes it.
        let utf16 = TempFile::new(b"\xff\xfe[[projects]]\n");
        let err = format!("{:#}", load(Some(&utf16.0)).unwrap_err());
        assert!(err.contains("cannot read config file"), "{err}");
        assert!(err.contains(&utf16.0.display().to_string()), "{err}");
        assert!(err.contains("must be UTF-8"), "{err}");
        assert!(
            !err.contains("[[projects]]"),
            "no example for a present file: {err}"
        );

        // A directory is reported as such on every platform (Unix says
        // EISDIR, Windows says NotFound - neither hint would be right).
        let dir = std::env::temp_dir();
        let shown = std::path::absolute(&dir).unwrap().display().to_string();
        let err = format!("{:#}", load(Some(&dir)).unwrap_err());
        assert!(err.contains("is a directory, not a file"), "{err}");
        assert!(err.contains(&shown), "{err}");
        assert!(!err.contains("must be UTF-8"), "{err}");
        assert!(!err.contains("config file not found"), "{err}");
    }
}
