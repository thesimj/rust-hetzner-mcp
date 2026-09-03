//! Command-line surface: `[--config <path>] | --help | --version`. Pure std;
//! argv only - nothing here reads the environment or touches a file.

use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

/// What the process was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cli {
    /// Run the MCP server, reading `config` or the default config file.
    Serve {
        config: Option<PathBuf>,
    },
    Help,
    Version,
}

pub const USAGE: &str = "usage: hetzner-mcp [--config <path>] | --help | --version";

/// Parse the arguments after the program name.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Cli> {
    let mut args = args.into_iter();
    let mut config = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--config") => {
                let path = args
                    .next()
                    .filter(|p| !p.is_empty())
                    .context("--config requires a path")?;
                config = Some(PathBuf::from(path));
            }
            Some("--help" | "-h") => return Ok(Cli::Help),
            Some("--version" | "-V") => return Ok(Cli::Version),
            _ => bail!("unexpected argument \"{}\"\n{USAGE}", arg.to_string_lossy()),
        }
    }
    Ok(Cli::Serve { config })
}

/// `--help` output: the usage line plus the resolved default config path.
pub fn help_text(default_path: &str) -> String {
    format!("{USAGE}\n  default config file: {default_path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    // 25
    #[test]
    fn no_args_means_serve_with_the_default_config() {
        assert_eq!(parse(args(&[])).unwrap(), Cli::Serve { config: None });
    }

    // 26
    #[test]
    fn config_flag_takes_the_next_argument() {
        assert_eq!(
            parse(args(&["--config", "/p"])).unwrap(),
            Cli::Serve {
                config: Some(PathBuf::from("/p"))
            }
        );
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let raw = OsString::from_vec(vec![b'/', 0xff, b'p']);
            assert_eq!(
                parse(vec![OsString::from("--config"), raw.clone()]).unwrap(),
                Cli::Serve {
                    config: Some(PathBuf::from(raw))
                }
            );
        }
    }

    // 27
    #[test]
    fn rejects_a_dangling_or_empty_config_flag_and_unknown_arguments() {
        for bad in [vec!["--config"], vec!["--config", ""]] {
            let err = parse(args(&bad)).unwrap_err().to_string();
            assert!(err.contains("requires a path"), "{bad:?}: {err}");
        }
        for bad in [vec!["--bogus"], vec!["serve"], vec!["--config=/p"]] {
            let err = parse(args(&bad)).unwrap_err().to_string();
            assert!(err.contains("usage:"), "{bad:?}: {err}");
            assert!(err.contains(bad[0]), "{bad:?}: {err}");
        }
    }

    // 28
    #[test]
    fn recognises_help_and_version() {
        assert_eq!(parse(args(&["--help"])).unwrap(), Cli::Help);
        assert_eq!(parse(args(&["-h"])).unwrap(), Cli::Help);
        assert_eq!(parse(args(&["--version"])).unwrap(), Cli::Version);
        assert_eq!(parse(args(&["-V"])).unwrap(), Cli::Version);
        let help = help_text("/some/path");
        assert!(help.contains("/some/path"), "{help}");
        assert!(help.contains("usage:"), "{help}");
    }
}
