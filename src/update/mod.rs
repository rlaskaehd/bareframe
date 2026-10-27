//! Updating from GitHub Releases without an Apple Developer ID: the latest release is looked
//! up at startup and once a day (Settings can turn that off), offered in the top bar, and
//! installed only when clicked. Archives are checked against the maintainer's Ed25519 key.
mod install;
pub mod release;

use gpui::{App, AppContext, Context, Entity, Global, Task};
use release::Release;
use std::time::Duration;

/// How long after startup the first look happens, and how often after that.
const FIRST_CHECK: Duration = Duration::from_secs(15);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

pub enum State {
    Idle,
    Checking,
    Available(Release),
    Installing(String),
    /// Shown after a check asked for from the menu.
    UpToDate,
    Failed(String),
}

pub struct Updates {
    pub state: State,
    /// Settings › Check for updates.
    automatic: bool,
    _work: Option<Task<()>>,
    _schedule: Task<()>,
}

struct GlobalUpdates(Entity<Updates>);

impl Global for GlobalUpdates {}

pub fn init(cx: &mut App) {
    let updates = cx.new(|cx| Updates {
        state: State::Idle,
        automatic: false,
        _work: None,
        _schedule: cx.spawn(async move |updates, cx| {
            cx.background_executor().timer(FIRST_CHECK).await;
            loop {
                let checked = updates.update(cx, |updates: &mut Updates, cx| {
                    if updates.automatic
                        && matches!(updates.state, State::Idle)
                        && install::running_bundle().is_some()
                    {
                        updates.check(false, cx);
                    }
                });
                if checked.is_err() {
                    return;
                }
                cx.background_executor().timer(CHECK_EVERY).await;
            }
        }),
    });
    cx.set_global(GlobalUpdates(updates));
}

pub fn global(cx: &App) -> Entity<Updates> {
    cx.global::<GlobalUpdates>().0.clone()
}

impl Updates {
    pub fn set_automatic(&mut self, automatic: bool) {
        self.automatic = automatic;
    }

    /// Looks for a newer release. Asked from the menu, it also reports "up to date" and
    /// failures; on its own it stays quiet unless there is something to offer.
    pub fn check(&mut self, asked: bool, cx: &mut Context<Self>) {
        if matches!(self.state, State::Checking | State::Installing(_)) {
            return;
        }
        self.state = State::Checking;
        cx.notify();
        self._work = Some(cx.spawn(async move |updates, cx| {
            let found = cx.background_executor().spawn(async { latest() }).await;
            updates
                .update(cx, |updates, cx| {
                    updates.state = match found {
                        Ok(Some(release)) => State::Available(release),
                        Ok(None) if asked => State::UpToDate,
                        Err(error) if asked => State::Failed(error),
                        _ => State::Idle,
                    };
                    cx.notify();
                })
                .ok();
        }));
    }

    /// Downloads and installs the offered release, then quits so it can start; editors with
    /// unsaved changes still ask first.
    pub fn install(&mut self, cx: &mut Context<Self>) {
        let State::Available(release) = &self.state else {
            return;
        };
        let Some(bundle) = install::running_bundle() else {
            self.state = State::Failed("only an installed release build updates itself".into());
            return cx.notify();
        };
        let release = release.clone();
        self.state = State::Installing(release.version.clone());
        cx.notify();
        self._work = Some(cx.spawn(async move |updates, cx| {
            let installed = cx
                .background_executor()
                .spawn({
                    let bundle = bundle.clone();
                    async move {
                        install::install(&release, &bundle)?;
                        install::relaunch_after_exit(&bundle)
                    }
                })
                .await;
            updates
                .update(cx, |updates, cx| match installed {
                    Ok(()) => cx.dispatch_action(&crate::actions::Quit),
                    Err(error) => {
                        updates.state = State::Failed(error);
                        cx.notify();
                    }
                })
                .ok();
        }));
    }

    /// The top bar's note was dismissed.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        if matches!(self.state, State::UpToDate | State::Failed(_)) {
            self.state = State::Idle;
            cx.notify();
        }
    }
}

/// The latest release, when it is newer than this build and signed releases are set up.
fn latest() -> Result<Option<Release>, String> {
    if release::PUBLIC_KEY.len() != 64 {
        return Err("this build has no release key to check updates against".into());
    }
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        release::REPOSITORY
    );
    let reply = match install::fetch(&url) {
        Ok(reply) => reply,
        // No release published yet.
        Err(error) if error.contains("error: 404") => return Ok(None),
        Err(error) => return Err(error),
    };
    let reply: serde_json::Value =
        serde_json::from_slice(&reply).map_err(|error| format!("GitHub's answer: {error}"))?;
    Ok(release::parse_latest(&reply)
        .filter(|release| release::is_newer(&release.version, env!("CARGO_PKG_VERSION"))))
}
