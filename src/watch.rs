//! Noticing changes made outside bareframe: a folder watcher whose events are batched and
//! handed to an entity on the UI thread.
use futures::StreamExt;
use gpui::{Context, Task};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{path::Path, time::Duration};

/// A live watch; dropping it stops the watcher and its task.
pub type Watch = (RecommendedWatcher, Task<()>);

/// Watches `dir`; `settle` after the first event `relevant` accepts, the rest of the burst
/// is dropped and `changed` runs once. `None` (reported) if `dir` can't be watched.
pub fn watch<T: 'static>(
    dir: &Path,
    mode: RecursiveMode,
    settle: Duration,
    relevant: impl Fn(&notify::Event) -> bool + Send + 'static,
    changed: impl Fn(&mut T, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) -> Option<Watch> {
    let (sender, mut receiver) = futures::channel::mpsc::unbounded::<()>();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|event| relevant(&event)) {
            let _ = sender.unbounded_send(());
        }
    })
    .inspect_err(|error| eprintln!("bareframe: watching {}: {error}", dir.display()))
    .ok()?;
    watcher
        .watch(dir, mode)
        .inspect_err(|error| eprintln!("bareframe: watching {}: {error}", dir.display()))
        .ok()?;
    let task = cx.spawn(async move |this, cx| {
        while receiver.next().await.is_some() {
            cx.background_executor().timer(settle).await;
            while receiver.try_recv().is_ok() {}
            if this.update(cx, |this, cx| changed(this, cx)).is_err() {
                break;
            }
        }
    });
    Some((watcher, task))
}

/// Watches one file. Editors often save by writing a new file and renaming it over the
/// old one, so the folder is watched and events are matched by name.
pub fn watch_file<T: 'static>(
    path: &Path,
    changed: impl Fn(&mut T, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) -> Option<Watch> {
    let name = path.file_name()?.to_owned();
    watch(
        path.parent()?,
        RecursiveMode::NonRecursive,
        Duration::from_millis(150),
        move |event| {
            event
                .paths
                .iter()
                .any(|path| path.file_name() == Some(name.as_os_str()))
        },
        changed,
        cx,
    )
}
