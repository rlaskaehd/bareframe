#[cfg(not(target_os = "macos"))]
compile_error!("bareframe currently targets macOS");

mod actions;
mod app;
mod assets;
mod diagram;
mod editor;
mod editor_tabs;
mod explorer;
mod files;
mod markdown;
mod menu_bar;
mod pane;
mod rename;
mod repo;
mod review;
mod search;
mod settings_pane;
mod syntax;
mod terminal;
mod theme;
mod watch;
mod workspace;

use bareframe_core::config::{HELP, LaunchConfig, LaunchRequest};

fn main() {
    let mut args = Vec::new();
    if app::is_validation_instance() {
        args.extend([
            "--clean-shell".into(),
            "--cwd".into(),
            concat!(env!("CARGO_MANIFEST_DIR"), "/target/validation").into(),
        ]);
    }
    args.extend(std::env::args_os().skip(1));
    // Asking for a directory means a fresh window there, not last session's tabs. The
    // validation instance always passes one but keeps its own saved state.
    let restore = app::is_validation_instance() || !args.iter().any(|arg| arg == "--cwd");
    let request = std::env::current_dir()
        .map_err(|error| error.to_string())
        .and_then(|cwd| {
            let home = files::home_dir().unwrap_or_else(|| cwd.clone());
            LaunchConfig::parse(args, &cwd, &home)
        });
    match request {
        Ok(LaunchRequest::Help) => println!("{HELP}"),
        Ok(LaunchRequest::Version) => println!("bareframe {}", env!("CARGO_PKG_VERSION")),
        Ok(LaunchRequest::Run(config)) => app::run(config, restore),
        Err(error) => {
            eprintln!("bareframe: {error}\n\n{HELP}");
            std::process::exit(2);
        }
    }
}
