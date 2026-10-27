//! The helper process Chromium starts for renderers, the GPU, the network and so on. The
//! bundle holds five copies of it under the names CEF looks for.
use cef::{args::Args, *};

/// The app starts one helper with this at launch: macOS checks a newly installed framework
/// the first time it is loaded (seconds, for Chromium's), and once checked by any process it
/// loads at once, so the app's first browser doesn't freeze the window waiting.
const WARM_UP: &str = "--bareframe-warm-up";

fn main() {
    if std::env::args().nth(1).as_deref() == Some(WARM_UP) {
        let exe = std::env::current_exe().expect("the helper's own path");
        std::process::exit(i32::from(
            !library_loader::LibraryLoader::new(&exe, true).load(),
        ));
    }
    let args = Args::new();
    // Renderers and the other helpers run sandboxed: a page that breaks Chromium still
    // can't reach the user's files.
    let mut sandbox = cef::sandbox::Sandbox::new();
    sandbox.initialize(args.as_main_args());
    let loader = library_loader::LibraryLoader::new(
        &std::env::current_exe().expect("the helper's own path"),
        true,
    );
    if !loader.load() {
        eprintln!("bareframe-helper: Chromium Embedded Framework not found");
        std::process::exit(1);
    }
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let code = execute_process(
        Some(args.as_main_args()),
        Some(&mut HelperApp::new()),
        std::ptr::null_mut(),
    );
    drop(sandbox);
    std::process::exit(code);
}

wrap_app! {
    struct HelperApp;

    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            // As in the app (see engine::BrowserApp): the network service reads the cookie
            // key, which must come from the fixed key and never the Keychain.
            if let Some(command_line) = command_line {
                command_line.append_switch(Some(&CefString::from("use-mock-keychain")));
            }
        }
    }
}
