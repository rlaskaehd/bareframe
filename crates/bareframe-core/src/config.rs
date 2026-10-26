use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub const HELP: &str = "bareframe [--cwd DIRECTORY] [--shell EXECUTABLE] [--clean-shell]\n\
    --cwd DIRECTORY    Initial working directory (default: current directory)\n\
    --shell EXECUTABLE Absolute shell executable (default: /bin/zsh)\n\
    --clean-shell      Start zsh -f -i without user startup files\n\
    --help             Print this help\n\
    --version          Print the version";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchConfig {
    pub cwd: PathBuf,
    pub shell: PathBuf,
    pub clean_shell: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LaunchRequest {
    Run(LaunchConfig),
    Help,
    Version,
}

impl LaunchConfig {
    pub fn parse(
        args: impl IntoIterator<Item = OsString>,
        cwd: &Path,
    ) -> Result<LaunchRequest, String> {
        let mut args = args.into_iter();
        let mut directory = cwd.to_owned();
        let mut shell = PathBuf::from("/bin/zsh");
        let mut clean_shell = false;
        while let Some(argument) = args.next() {
            match argument.to_str() {
                Some("--help" | "-h") => return Ok(LaunchRequest::Help),
                Some("--version" | "-V") => return Ok(LaunchRequest::Version),
                Some("--cwd") => {
                    directory = PathBuf::from(args.next().ok_or("--cwd requires a directory")?)
                }
                Some("--shell") => {
                    shell = PathBuf::from(args.next().ok_or("--shell requires an executable")?)
                }
                Some("--clean-shell") => clean_shell = true,
                _ => return Err(format!("unknown argument: {}", argument.to_string_lossy())),
            }
        }
        if !directory.is_absolute() {
            directory = cwd.join(directory);
        }
        let cwd = directory
            .canonicalize()
            .map_err(|error| format!("working directory {}: {error}", directory.display()))?;
        if !cwd.is_dir() {
            return Err(format!("not a directory: {}", cwd.display()));
        }
        if !shell.is_absolute() {
            return Err("--shell must be an absolute executable path".into());
        }
        if !shell.is_file() {
            return Err(format!("shell does not exist: {}", shell.display()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if shell
                .metadata()
                .map_err(|e| e.to_string())?
                .permissions()
                .mode()
                & 0o111
                == 0
            {
                return Err(format!("shell is not executable: {}", shell.display()));
            }
        }
        if cwd.to_str().is_none() || shell.to_str().is_none() {
            return Err("Ghostty requires UTF-8 directory and executable paths".into());
        }
        if clean_shell && shell.file_name().and_then(|n| n.to_str()) != Some("zsh") {
            return Err("--clean-shell requires zsh".into());
        }
        Ok(LaunchRequest::Run(Self {
            cwd,
            shell,
            clean_shell,
        }))
    }

    /// Only the shell executable enters the command parser. Cwd has its own field.
    pub fn command(&self) -> String {
        let shell = self.shell.to_str().expect("validated UTF-8 shell");
        let quoted = format!("'{}'", shell.replace('\'', "'\\''"));
        format!(
            "{quoted} {}",
            if self.clean_shell { "-f -i" } else { "-l -i" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_or_invalid_arguments_fail_without_launching() {
        for args in [
            vec!["--cwd"],
            vec!["--unknown"],
            vec!["--shell", "zsh"],
            vec!["--cwd", "/bin/zsh"],
        ] {
            assert!(
                LaunchConfig::parse(args.into_iter().map(Into::into), Path::new("/tmp")).is_err()
            );
        }
    }
    #[test]
    fn help_never_validates_the_shell_or_cwd() {
        assert_eq!(
            LaunchConfig::parse(["--help".into()], Path::new("/does-not-exist")),
            Ok(LaunchRequest::Help)
        );
    }
    #[test]
    fn cwd_is_never_interpolated_in_shell_command() {
        let config = LaunchConfig {
            cwd: PathBuf::from("/tmp/한글 project; touch unsafe"),
            shell: "/bin/zsh".into(),
            clean_shell: false,
        };
        assert_eq!(config.command(), "'/bin/zsh' -l -i");
        assert!(!config.command().contains("unsafe"));
    }
    #[test]
    fn shell_path_is_quoted_and_clean_mode_disables_user_rc() {
        let config = LaunchConfig {
            cwd: "/tmp".into(),
            shell: "/a b/it's/zsh".into(),
            clean_shell: true,
        };
        assert_eq!(config.command(), "'/a b/it'\\''s/zsh' -f -i");
    }
}
