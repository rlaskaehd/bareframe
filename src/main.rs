#[cfg(not(target_os = "macos"))]
compile_error!("bareframe MVP 1 currently targets macOS");

mod app;
mod terminal;
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
