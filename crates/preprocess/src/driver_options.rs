//! Common, shell-free external C preprocessor command-line option handling.
//! Both the checker and optional native-object driver use this parser.
//! The surrounding driver owns filenames and its own non-CPP options.
use crate::Config;
use std::ffi::OsString;

#[derive(Debug, Clone)]
pub struct CppOptions {
    executable: Option<OsString>,
    arguments: Vec<OsString>,
}

impl Default for CppOptions {
    fn default() -> Self {
        Self {
            executable: Some(OsString::from("cc")),
            arguments: vec![OsString::from("-E")],
        }
    }
}

impl CppOptions {
    /// Consume one known preprocessor option, optionally consuming an adjacent
    /// value in \`remaining\`. Return false only for non-preprocessor options.
    /// Every flag/value remains an OS argument, never shell source text.
    pub fn consume(
        &mut self, arg: &str, remaining: &mut impl Iterator<Item = String>,
    ) -> Result<bool, String> {
        if let Some(value) = arg.strip_prefix("--cpp=") {
            if value.is_empty() {
                return Err("--cpp requires an executable name or 'none'".into());
            }
            self.executable = if value == "none" { None } else { Some(value.into()) };
        } else if let Some(value) = arg.strip_prefix("--cpp-arg=") {
            if value.is_empty() { return Err("--cpp-arg requires a value".into()); }
            self.arguments.push(value.into());
        } else if arg == "-nostdinc" {
            self.arguments.push(arg.into());
        } else if matches!(arg, "-isystem" | "--sysroot") {
            let value = remaining.next().ok_or_else(|| format!("{arg} requires a directory"))?;
            if value.is_empty() { return Err(format!("{arg} requires a directory")); }
            let joined = if arg == "--sysroot" {
                format!("{arg}={value}")
            } else {
                format!("{arg}{value}")
            };
            self.arguments.push(joined.into());
        } else if arg.starts_with("-isystem") || arg.starts_with("--sysroot=") {
            self.arguments.push(arg.into());
        } else if matches!(arg, "-I" | "-D" | "-U") {
            let value = remaining.next().ok_or_else(|| format!("{arg} requires a value"))?;
            if value.is_empty() { return Err(format!("{arg} requires a nonempty value")); }
            self.arguments.push(format!("{arg}{value}").into());
        } else if arg.starts_with("-I") || arg.starts_with("-D") || arg.starts_with("-U") {
            self.arguments.push(arg.into());
        } else {
            return Ok(false);
        }
        Ok(true)
    }

    pub fn finish(self) -> Result<Option<Config>, String> {
        if self.executable.is_none() && self.arguments.len() != 1 {
            return Err(
                "preprocessor options -I/-D/-U/-isystem/--sysroot/-nostdinc/--cpp-arg cannot be used with --cpp=none".into(),
            );
        }
        Ok(self.executable.map(|executable| Config {
            executable,
            args: self.arguments,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_split_attached_and_no_shell_evaluation() {
        let mut options = CppOptions::default();
        let mut values = ["include dir".to_owned(), "WITH=1".to_owned()].into_iter();
        for arg in ["-I", "-D", "-UOLD", "--cpp=clang", "--cpp-arg=-undef",
                    "-DTEXT=x;$(touch /tmp/never)"] {
            assert!(options.consume(arg, &mut values).unwrap());
        }
        let config = options.finish().unwrap().unwrap();
        assert_eq!(config.executable, OsString::from("clang"));
        assert_eq!(config.args, vec![
            "-E", "-Iinclude dir", "-DWITH=1", "-UOLD", "-undef",
            "-DTEXT=x;$(touch /tmp/never)",
        ].into_iter().map(OsString::from).collect::<Vec<_>>());
    }

    #[test]
    fn accepts_system_header_root_options_without_shell() {
        let mut cpp = CppOptions::default();
        let mut args = ["my system headers".to_string(), "/tmp/test root".to_string()].into_iter();
        for arg in ["-isystem", "--sysroot", "-nostdinc", "-Iproject"] {
            assert!(cpp.consume(arg, &mut args).unwrap());
        }
        assert_eq!(cpp.finish().unwrap().unwrap().args,
            vec!["-E", "-isystemmy system headers", "--sysroot=/tmp/test root",
                 "-nostdinc", "-Iproject"].into_iter().map(OsString::from)
                .collect::<Vec<_>>());
    }

    #[test]
    fn disabled_preprocessor_forbids_ignored_flags() {
        let mut options = CppOptions::default();
        assert!(options.consume("--cpp=none", &mut std::iter::empty()).unwrap());
        assert!(options.finish().unwrap().is_none());
        let mut options = CppOptions::default();
        assert!(options.consume("-DTEST", &mut std::iter::empty()).unwrap());
        assert!(options.consume("--cpp=none", &mut std::iter::empty()).unwrap());
        assert!(options.finish().unwrap_err().contains("none"));
        assert!(CppOptions::default().consume("--cpp=", &mut std::iter::empty()).is_err());
        assert!(CppOptions::default().consume("-U", &mut std::iter::empty()).is_err());
    }
}
