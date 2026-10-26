#[cfg(not(target_os = "macos"))]
compile_error!("bareframe MVP 1 currently targets macOS");

mod app;
mod terminal;
mod workspace;

use bareframe_core::config::{HELP, LaunchConfig, LaunchRequest};

fn main() {
    // These describe the terminal bareframe was started from. Inherited, they
    // point Ghostty and the shell at that app's resources and integration
    // features, so a session started inside cmux or Ghostty would differ from
    // one started with `open`. Ghostty leaves GHOSTTY_SHELL_FEATURES alone when
    // every feature is off, so it has to go here.
    for name in [
        "GHOSTTY_RESOURCES_DIR",
        "GHOSTTY_SHELL_FEATURES",
        "GHOSTTY_ZSH_ZDOTDIR",
        "TERMINFO",
    ] {
        // SAFETY: nothing else has started yet, so no thread reads the environment.
        #[allow(unsafe_code)]
        unsafe {
            std::env::remove_var(name)
        };
    }
    let mut args = Vec::new();
    if app::is_validation_instance() {
        if app::validation_uses_clean_shell() {
            args.push("--clean-shell".into());
        }
        args.extend([
            "--cwd".into(),
            concat!(env!("CARGO_MANIFEST_DIR"), "/target/validation").into(),
        ]);
    }
    args.extend(std::env::args_os().skip(1));
    let request = std::env::current_dir()
        .map_err(|error| error.to_string())
        .and_then(|cwd| LaunchConfig::parse(args, &cwd));
    match request {
        Ok(LaunchRequest::Help) => println!("{HELP}"),
        Ok(LaunchRequest::Version) => println!("bareframe {}", env!("CARGO_PKG_VERSION")),
        Ok(LaunchRequest::Run(config)) => app::run(config),
        Err(error) => {
            eprintln!("bareframe: {error}\n\n{HELP}");
            std::process::exit(2);
        }
    }
}
