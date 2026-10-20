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
        Self {
            executable: OsString::from("cc"),
            args: vec![OsString::from("-E")],
        }
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
            Self::Failed { code, stderr } => {
                write!(f, "preprocessor failed (exit {code:?}): {stderr}")
            }
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
pub fn already_preprocessed(source: &str) -> &str {
    source
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bypass_preserves_input() {
        assert_eq!(
            already_preprocessed("# 1 \"a.c\"\nint x;"),
            "# 1 \"a.c\"\nint x;"
        );
    }
    #[test]
    fn preprocessing_includes_macros_and_conditional_code() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("mini c preprocess {} {nonce}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let header = dir.join("header with spaces.h");
        let input = dir.join("source with spaces.c");
        std::fs::write(&header, "#define VALUE 17\n").unwrap();
        std::fs::write(&input, "#include \"header with spaces.h\"\n#if VALUE == 17\nint result = VALUE;\n#endif\n").unwrap();
        let result = file(&input, &Config::default());
        let _ = std::fs::remove_dir_all(&dir);
        let output = match result {
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return,
            other => other.unwrap(),
        };
        assert!(output.contains("int result = 17;"), "{output}");
        let map = source_map::SourceMap::from_preprocessed(&output, input.to_str().unwrap());
        let origin = map.origin(output.find("int result").unwrap());
        assert_eq!(origin.line, 3);
        assert!(origin.file.ends_with("source with spaces.c"));
        let mut parser = mini_c_syntax::parser::Parser::new(&output);
        use mini_c_syntax::stream::ItemSource;
        assert!(parser.next_item().unwrap().is_some());
        assert!(parser.next_item().unwrap().is_none());
    }

    #[test]
    fn preprocessing_reports_nonzero_exit_and_stderr() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("mini-c-missing-{}-{nonce}.c", std::process::id()));
        std::fs::write(&path, "#error mini_c_expected_failure\n").unwrap();
        let result = file(&path, &Config::default());
        let _ = std::fs::remove_file(&path);
        match result {
            Err(Error::Failed { code, stderr }) => {
                assert_ne!(code, Some(0));
                assert!(stderr.contains("mini_c_expected_failure"), "{stderr}");
            }
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {},
            other => panic!("expected failed preprocessing, got {other:?}"),
        }
    }

    #[test]
    fn missing_executable_is_an_error() {
        let config = Config {
            executable: OsString::from("mini-c-preprocessor-does-not-exist-982734"),
            args: vec![],
        };
        assert!(matches!(file(Path::new("x.c"), &config), Err(Error::Io(_))));
    }
}
