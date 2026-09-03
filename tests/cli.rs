//! Process-level tests: spawn the real binary and check exit codes, stdout
//! (the MCP wire) and stderr (diagnostics). Never touches `~/.config`: every
//! child either gets `--config` or a fake `HOME`/`USERPROFILE` with
//! `XDG_CONFIG_HOME` removed, set on the child only.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_hetzner-mcp");

fn fake_token(c: char) -> String {
    std::iter::repeat_n(c, 64).collect()
}

/// A mixed-case token: 64 lowercase chars would be a legal project name.
fn mixed_token() -> String {
    "Ab".repeat(32)
}

/// A temp directory removed on drop; files are created inside it.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("hetzner-mcp-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn display(&self) -> String {
        self.0.display().to_string()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_closed_stdin(configure: impl FnOnce(&mut Command)) -> Output {
    let mut cmd = Command::new(BIN);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure(&mut cmd);
    cmd.output().expect("spawn hetzner-mcp")
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Point the child at an empty fake home with no XDG override.
fn fake_home(cmd: &mut Command, home: &Path) {
    cmd.env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("XDG_CONFIG_HOME");
}

// 38
#[test]
fn missing_config_file_exits_non_zero_and_names_the_absolute_path() {
    let tmp = TempDir::new();
    let missing = tmp.path().join("nonexistent.toml");
    let out = run_closed_stdin(|c| {
        c.args(["--config", &missing.display().to_string()]);
    });
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty(), "stdout: {}", stdout_of(&out));
    let stderr = stderr_of(&out);
    assert!(stderr.contains(&missing.display().to_string()), "{stderr}");
    assert!(stderr.contains("[[projects]]"), "{stderr}");
    assert!(stderr.contains("--config"), "{stderr}");

    // A relative --config is absolutized against the child's cwd.
    let out = run_closed_stdin(|c| {
        c.current_dir(tmp.path()).args(["--config", "missing.toml"]);
    });
    assert_eq!(out.status.code(), Some(1));
    let stderr = stderr_of(&out);
    let expected = tmp.path().join("missing.toml").display().to_string();
    assert!(stderr.contains(&expected), "{stderr}");
}

// 39a
#[test]
fn default_path_is_resolved_under_a_fake_home() {
    let home = TempDir::new();
    let out = run_closed_stdin(|c| fake_home(c, home.path()));
    assert_eq!(out.status.code(), Some(1));
    let expected = home
        .path()
        .join(".config")
        .join("hetzner-mcp")
        .join("config.toml");
    let stderr = stderr_of(&out);
    assert!(stderr.contains(&expected.display().to_string()), "{stderr}");
    assert!(stderr.contains("config file not found"), "{stderr}");
    assert_eq!(
        std::fs::read_dir(home.path()).unwrap().count(),
        0,
        "nothing created under {}",
        home.display()
    );
}

// 39b
#[test]
fn xdg_config_home_overrides_the_home_fallback_and_is_absolutized() {
    let home = TempDir::new();
    let xdg = TempDir::new();
    let out = run_closed_stdin(|c| {
        fake_home(c, home.path());
        c.env("XDG_CONFIG_HOME", xdg.path());
    });
    assert_eq!(out.status.code(), Some(1));
    let expected = xdg.path().join("hetzner-mcp").join("config.toml");
    let stderr = stderr_of(&out);
    assert!(stderr.contains(&expected.display().to_string()), "{stderr}");
    assert!(
        !stderr.contains(".config"),
        "HOME fallback not used: {stderr}"
    );

    // A relative XDG_CONFIG_HOME is shown resolved against the child's cwd.
    let cwd = TempDir::new();
    let out = run_closed_stdin(|c| {
        fake_home(c, home.path());
        c.env("XDG_CONFIG_HOME", "rel").current_dir(cwd.path());
    });
    assert_eq!(out.status.code(), Some(1));
    let expected = cwd
        .path()
        .join("rel")
        .join("hetzner-mcp")
        .join("config.toml");
    let stderr = stderr_of(&out);
    assert!(stderr.contains(&expected.display().to_string()), "{stderr}");

    // Nothing was created anywhere under the fake dirs.
    for dir in [&home, &xdg, &cwd] {
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "{}",
            dir.display()
        );
    }
}

// 40
#[test]
fn a_valid_config_serves_the_mcp_handshake_over_stdio() {
    let tmp = TempDir::new();
    let config = tmp.file(
        "config.toml",
        &format!(
            "endpoint = \"http://127.0.0.1:9\"\n\n\
             [[projects]]\nname = \"prod\"\ntoken = \"{}\"\n\n\
             [[projects]]\nname = \"staging\"\ntoken = \"{}\"\ndescription = \"staging infra\"\n",
            fake_token('a'),
            fake_token('b')
        ),
    );
    let mut child = Command::new(BIN)
        .args(["--config", &config.display().to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn hetzner-mcp");
    {
        let mut stdin = child.stdin.take().unwrap();
        let initialize = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2026-07-28",
                "capabilities": {},
                "clientInfo": {"name": "cli-test", "version": "0"}
            }
        });
        writeln!(stdin, "{initialize}").unwrap();
        // stdin drops here: EOF ends the server loop.
    }
    let out = child.wait_with_output().unwrap();
    let stdout = stdout_of(&out);
    assert!(stdout.contains("\"name\":\"hetzner-mcp\""), "{stdout}");
    assert!(
        stdout.contains(&format!("\"version\":\"{}\"", env!("CARGO_PKG_VERSION"))),
        "{stdout}"
    );
    let stderr = stderr_of(&out);
    assert!(!stderr.contains(&fake_token('a')), "{stderr}");
    assert!(!stderr.contains(&fake_token('b')), "{stderr}");
    assert_eq!(out.status.code(), Some(0), "stderr: {stderr}");
}

/// `--config <file>` -> (exit code, stderr).
fn run_with_config(path: &Path) -> (Option<i32>, String) {
    let out = run_closed_stdin(|c| {
        c.args(["--config", &path.display().to_string()]);
    });
    (out.status.code(), stderr_of(&out))
}

// 41a
#[test]
fn a_short_token_exits_non_zero_citing_the_index_not_the_token() {
    let tmp = TempDir::new();
    let t = mixed_token();
    let short = tmp.file(
        "short.toml",
        &format!("[[projects]]\nname = \"prod\"\ntoken = \"{}\"\n", &t[..63]),
    );
    let (code, stderr) = run_with_config(&short);
    assert_eq!(code, Some(1));
    assert!(stderr.contains("projects[1]"), "{stderr}");
    assert!(!stderr.contains(&t[..63]), "{stderr}");
}

// 41b
#[test]
fn an_unterminated_token_line_exits_non_zero_without_echoing_it() {
    let tmp = TempDir::new();
    let t = mixed_token();
    let unterminated = tmp.file(
        "unterminated.toml",
        &format!("[[projects]]\nname = \"prod\"\ntoken = \"{t}\n"),
    );
    let (code, stderr) = run_with_config(&unterminated);
    assert_eq!(code, Some(1));
    assert!(stderr.contains("line 3"), "{stderr}");
    assert!(!stderr.contains(&t), "{stderr}");
}

// 41c
#[test]
fn a_token_in_place_of_the_projects_array_is_redacted_with_the_bracket_hint() {
    let tmp = TempDir::new();
    let t = mixed_token();
    let swapped = tmp.file("swapped.toml", &format!("projects = \"{t}\"\n"));
    let (code, stderr) = run_with_config(&swapped);
    assert_eq!(code, Some(1));
    assert!(stderr.contains("<redacted>"), "{stderr}");
    assert!(stderr.contains("double brackets"), "{stderr}");
    assert!(!stderr.contains(&t), "{stderr}");
}

// 42
#[test]
fn help_and_version_exit_zero_without_a_config_file() {
    let home = TempDir::new();
    let out = run_closed_stdin(|c| {
        fake_home(c, home.path());
        c.arg("--help");
    });
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("usage:"), "{stdout}");
    assert!(stdout.contains("config.toml"), "{stdout}");

    let out = run_closed_stdin(|c| {
        fake_home(c, home.path());
        c.arg("--version");
    });
    assert_eq!(out.status.code(), Some(0), "{}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out).trim(),
        format!("hetzner-mcp {}", env!("CARGO_PKG_VERSION"))
    );
}
