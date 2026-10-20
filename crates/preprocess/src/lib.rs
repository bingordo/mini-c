//! Optional external preprocessing. The syntax and semantic crates do not depend on this crate.
//! Commands are invoked directly, never through a shell.
pub mod source_map;

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct Config {
    pub executable: OsString,
    pub args: Vec<OsString>,
}

impl Default for Config {
    fn default() -> Self {
        Self { executable: OsString::from("cc"), args: vec![OsString::from("-E")] }
    }
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Failed { code: Option<i32>, stderr: String },
    InvalidUtf8(std::string::FromUtf8Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot execute preprocessor: {e}"),
            Self::Failed { code, stderr } => write!(f, "preprocessor failed (exit {code:?}): {stderr}"),
            Self::InvalidUtf8(e) => write!(f, "preprocessor output is not UTF-8: {e}"),
        }
    }
}
impl std::error::Error for Error {}

/// Preprocess a file. This is an explicit driver operation, not a parser side effect.
pub fn file(path: &Path, config: &Config) -> Result<String, Error> {
    let output = Command::new(&config.executable)
        .args(&config.args)
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(Error::Io)?;
    if !output.status.success() {
        return Err(Error::Failed {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    String::from_utf8(output.stdout).map_err(Error::InvalidUtf8)
}

/// For already-preprocessed input, bypass all subprocesses and preserve the bytes.
pub fn already_preprocessed(source: &str) -> &str { source }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bypass_preserves_input() {
        assert_eq!(already_preprocessed("# 1 \"a.c\"\nint x;"), "# 1 \"a.c\"\nint x;");
    }
    #[test]
    fn missing_executable_is_an_error() {
        let config = Config { executable: OsString::from("mini-c-preprocessor-does-not-exist-982734"), args: vec![] };
        assert!(matches!(file(Path::new("x.c"), &config), Err(Error::Io(_))));
    }
}
