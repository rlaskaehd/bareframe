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

    /// Environment that loads the zsh integration in `integration_dir` through
    /// ZDOTDIR. Empty means the session runs without integration: clean shells,
    /// shells other than zsh, and values a C environment cannot carry.
    pub fn shell_integration_environment(
        &self,
        integration_dir: Option<&Path>,
        original_zdotdir: Option<&std::ffi::OsStr>,
    ) -> Vec<(String, String)> {
        if self.clean_shell || self.shell.file_name().and_then(|n| n.to_str()) != Some("zsh") {
            return Vec::new();
        }
        let Some(dir) = integration_dir.and_then(Path::to_str) else {
            return Vec::new();
        };
        let mut environment = vec![("ZDOTDIR".to_owned(), dir.to_owned())];
        // The integration's .zshenv restores GHOSTTY_ZSH_ZDOTDIR when it is set,
        // even to an empty value, and otherwise unsets ZDOTDIR.
        match original_zdotdir.filter(|value| !value.is_empty()) {
            None => {}
            Some(value) => match value.to_str() {
                Some(value) => environment.push(("GHOSTTY_ZSH_ZDOTDIR".into(), value.into())),
                None => return Vec::new(),
            },
        }
        environment
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
    fn zdotdir_is_kept_only_when_the_user_set_one() {
        let config = LaunchConfig {
            cwd: "/tmp".into(),
            shell: "/bin/zsh".into(),
            clean_shell: false,
        };
        let dir = Path::new("/App/Contents/Resources/shell-integration/zsh");
        let zdotdir = ("ZDOTDIR".to_owned(), dir.to_str().unwrap().to_owned());
        assert_eq!(
            config.shell_integration_environment(Some(dir), None),
            std::slice::from_ref(&zdotdir)
        );
        // An empty value would make .zshenv export ZDOTDIR="" instead of unsetting it.
        assert_eq!(
            config.shell_integration_environment(Some(dir), Some("".as_ref())),
            std::slice::from_ref(&zdotdir)
        );
        assert_eq!(
            config.shell_integration_environment(Some(dir), Some("/home/u/.zsh".as_ref())),
            [
                zdotdir,
                ("GHOSTTY_ZSH_ZDOTDIR".to_owned(), "/home/u/.zsh".to_owned())
            ]
        );
    }
    #[test]
    fn sessions_without_integration_get_no_environment() {
        let dir = Path::new("/integration/zsh");
        let clean = LaunchConfig {
            cwd: "/tmp".into(),
            shell: "/bin/zsh".into(),
            clean_shell: true,
        };
        assert!(
            clean
                .shell_integration_environment(Some(dir), None)
                .is_empty()
        );
        let bash = LaunchConfig {
            shell: "/bin/bash".into(),
            clean_shell: false,
            ..clean.clone()
        };
        assert!(
            bash.shell_integration_environment(Some(dir), None)
                .is_empty()
        );
        let zsh = LaunchConfig {
            shell: "/bin/zsh".into(),
            ..bash
        };
        assert!(zsh.shell_integration_environment(None, None).is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let invalid = std::ffi::OsStr::from_bytes(b"/home/\xff");
            assert!(
                zsh.shell_integration_environment(Some(dir), Some(invalid))
                    .is_empty(),
                "a ZDOTDIR that cannot be restored disables integration"
            );
        }
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
