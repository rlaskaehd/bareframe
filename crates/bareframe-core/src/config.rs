use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub const HELP: &str = "bareframe [--cwd DIRECTORY] [--shell EXECUTABLE] [--clean-shell]\n\
    --cwd DIRECTORY    Initial working directory (default: home directory)\n\
    --shell EXECUTABLE Absolute shell executable (default: /bin/zsh)\n\
    --clean-shell      Start zsh -f -i without user startup files\n\
    --help             Print this help\n\
    --version          Print the version";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchConfig {
    pub cwd: PathBuf,
    pub shell: PathBuf,
    pub clean_shell: bool,
    /// The prompt starts at the bottom of the screen, as in Warp, rather than at the top
    /// as in other terminals. Only bareframe's zsh startup acts on it.
    pub prompt_bottom: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LaunchRequest {
    Run(LaunchConfig),
    Help,
    Version,
}

impl LaunchConfig {
    /// Starts in `home` unless `--cwd` is given; a relative `--cwd` resolves against `cwd`.
    pub fn parse(
        args: impl IntoIterator<Item = OsString>,
        cwd: &Path,
        home: &Path,
    ) -> Result<LaunchRequest, String> {
        let mut args = args.into_iter();
        let mut directory = home.to_owned();
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
            prompt_bottom: false,
        }))
    }

    /// Only the shell executable enters the command parser. Cwd has its own field.
    pub fn command(&self) -> String {
        let shell = self.shell.to_str().expect("validated UTF-8 shell");
        format!(
            "{} {}",
            quote(shell),
            if self.clean_shell { "-f -i" } else { "-l -i" }
        )
    }

    /// zsh reads `.zshenv` from bareframe's `own` directory first. It sources Ghostty's
    /// (`ghostty`), which restores the user's ZDOTDIR and rc files and installs the title and
    /// OSC 7 hooks, then starts the prompt at the bottom of the screen. Ghostty's automatic
    /// injection is unusable here (no resources dir), so it goes in the command. Clean
    /// shells (`-f`) and other shells run unchanged.
    pub fn command_with_zsh_integration(
        &self,
        integration: Option<(&Path, &Path)>,
        user_zdotdir: Option<&Path>,
    ) -> String {
        let is_zsh = self.shell.file_name().and_then(|n| n.to_str()) == Some("zsh");
        let Some((own, ghostty)) = integration
            .filter(|_| is_zsh && !self.clean_shell)
            .and_then(|(own, ghostty)| Some((own.to_str()?, ghostty.to_str()?)))
        else {
            return self.command();
        };
        let mut command = format!(
            "/usr/bin/env ZDOTDIR={} BAREFRAME_GHOSTTY_ZSH={}",
            quote(own),
            quote(ghostty)
        );
        if let Some(user) = user_zdotdir.and_then(Path::to_str) {
            command.push_str(&format!(" GHOSTTY_ZSH_ZDOTDIR={}", quote(user)));
        }
        if self.prompt_bottom {
            command.push_str(" BAREFRAME_PROMPT=bottom");
        }
        command.push_str(" GHOSTTY_SHELL_FEATURES=title ");
        command.push_str(&self.command());
        command
    }
}

/// Single-quotes `text` for a POSIX shell: Ghostty's `bash -c`, or a line typed at a prompt.
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Shows `path` with the home directory abbreviated to `~`, as zsh's `%~` does.
pub fn display_path(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
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
                LaunchConfig::parse(
                    args.into_iter().map(Into::into),
                    Path::new("/tmp"),
                    Path::new("/tmp")
                )
                .is_err()
            );
        }
    }
    #[test]
    fn help_never_validates_the_shell_or_cwd() {
        assert_eq!(
            LaunchConfig::parse(
                ["--help".into()],
                Path::new("/does-not-exist"),
                Path::new("/does-not-exist")
            ),
            Ok(LaunchRequest::Help)
        );
    }
    #[test]
    fn cwd_is_never_interpolated_in_shell_command() {
        let config = LaunchConfig {
            cwd: PathBuf::from("/tmp/한글 project; touch unsafe"),
            shell: "/bin/zsh".into(),
            clean_shell: false,
            prompt_bottom: false,
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
            prompt_bottom: false,
        };
        assert_eq!(config.command(), "'/a b/it'\\''s/zsh' -f -i");
    }
    #[test]
    fn default_cwd_is_home_and_relative_cwd_uses_process_cwd() {
        let home = std::env::temp_dir().canonicalize().unwrap();
        let Ok(LaunchRequest::Run(config)) = LaunchConfig::parse([], Path::new("/"), &home) else {
            panic!("expected a run request");
        };
        assert_eq!(config.cwd, home);
        let Ok(LaunchRequest::Run(config)) =
            LaunchConfig::parse(["--cwd".into(), "usr".into()], Path::new("/"), &home)
        else {
            panic!("expected a run request");
        };
        assert_eq!(config.cwd, Path::new("/usr"));
    }
    #[test]
    fn zsh_integration_wraps_only_interactive_zsh() {
        let mut config = LaunchConfig {
            cwd: "/tmp".into(),
            shell: "/bin/zsh".into(),
            clean_shell: false,
            prompt_bottom: false,
        };
        let dirs = (Path::new("/App Support/it's/own"), Path::new("/g"));
        assert_eq!(
            config.command_with_zsh_integration(Some(dirs), Some(Path::new("/z"))),
            "/usr/bin/env ZDOTDIR='/App Support/it'\\''s/own' BAREFRAME_GHOSTTY_ZSH='/g' \
             GHOSTTY_ZSH_ZDOTDIR='/z' GHOSTTY_SHELL_FEATURES=title '/bin/zsh' -l -i"
        );
        assert!(
            !config
                .command_with_zsh_integration(Some(dirs), None)
                .contains("GHOSTTY_ZSH_ZDOTDIR")
        );
        config.prompt_bottom = true;
        assert!(
            config
                .command_with_zsh_integration(Some(dirs), None)
                .contains(" BAREFRAME_PROMPT=bottom GHOSTTY_SHELL_FEATURES=title ")
        );
        config.prompt_bottom = false;
        assert_eq!(
            config.command_with_zsh_integration(None, None),
            config.command()
        );
        config.clean_shell = true;
        assert_eq!(
            config.command_with_zsh_integration(Some(dirs), None),
            config.command()
        );
        config.clean_shell = false;
        config.shell = "/bin/bash".into();
        assert_eq!(
            config.command_with_zsh_integration(Some(dirs), None),
            config.command()
        );
    }
    #[test]
    fn display_path_abbreviates_home() {
        let home = Path::new("/Users/me");
        assert_eq!(display_path(home, home), "~");
        assert_eq!(
            display_path(Path::new("/Users/me/Sandbox/bareframe"), home),
            "~/Sandbox/bareframe"
        );
        assert_eq!(display_path(Path::new("/Users/meow"), home), "/Users/meow");
        assert_eq!(display_path(Path::new("/tmp"), home), "/tmp");
    }
}
