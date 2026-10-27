//! The Settings tab and the settings file: changes made on screen are validated, written
//! back to `~/.config/bareframe/config` and applied; outside edits to the file are reloaded.
use super::{PaneView, WorkspaceView};
use crate::{
    app,
    settings_pane::{SettingsEvent, SettingsPane},
    theme::Theme,
    watch::watch,
};
use bareframe_core::{
    keymap,
    settings::{self, Settings, ThemeName},
    workspace::PaneId,
};
use gpui::{AppContext, Context, Window, WindowAppearance};
use notify::RecursiveMode;
use std::time::Duration;

pub(super) fn is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

impl WorkspaceView {
    /// The theme in effect: the configured one, or the OS appearance for "Sync with OS".
    pub(super) fn theme(&self) -> &'static Theme {
        Theme::get(self.settings.theme().unwrap_or(if self.dark_appearance {
            ThemeName::Dark
        } else {
            ThemeName::Light
        }))
    }

    /// Warp keeps one Settings tab per window: open it, or go back to it.
    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let existing = self
            .panes
            .iter()
            .find(|(_, (pane, _))| matches!(pane, PaneView::Settings(_)))
            .map(|(id, _)| *id);
        if let Some(id) = existing {
            self.model.focus(id);
        } else {
            let id = self.model.new_tab();
            // Settings belongs to the window, not to the group the user happened to be in.
            if self.model.active().group.is_some() {
                let tab = self.model.active_tab;
                self.model.remove_from_group(&[tab]);
            }
            self.insert_settings_pane(id, window, cx);
        }
        self.sync(window, cx);
    }

    pub(super) fn insert_settings_pane(
        &mut self,
        id: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let theme = self.theme();
        let settings = self.settings.clone();
        let pane = cx.new(|cx| SettingsPane::new(theme, settings, window, cx));
        let events = cx.subscribe_in(&pane, window, |view, _, event, _, cx| match event {
            SettingsEvent::Set(key, value) => view.set_setting(key, value, cx),
            SettingsEvent::OpenFile => view.open_config_file(),
            SettingsEvent::Bind(name, change) => view.set_binding(name, change.as_ref(), cx),
        });
        self.panes
            .insert(id, (PaneView::Settings(pane), Some(events)));
    }

    /// Validates, saves and applies one setting. Invalid input is reported and dropped.
    pub(super) fn set_setting(&mut self, key: &str, value: &str, cx: &mut Context<Self>) {
        let Some(def) = settings::find(key) else {
            return;
        };
        match self.settings.set(key, value) {
            Ok(value) => {
                match app::save_setting(key, &value, def.default) {
                    Ok(text) => self.config_text = text,
                    Err(error) => eprintln!("bareframe: saving settings: {error}"),
                }
                if key == "font-size" {
                    self.base_font_size = self.settings.font_size();
                }
            }
            Err(error) => eprintln!("bareframe: {error}"),
        }
        self.apply_settings(cx);
    }

    /// Saves a Keyboard shortcuts edit as `keybind` lines and rebinds (`None` = default,
    /// `Some(None)` = no shortcut).
    fn set_binding(
        &mut self,
        name: &str,
        change: Option<&Option<keymap::Trigger>>,
        cx: &mut Context<Self>,
    ) {
        let change = change.map(Option::as_ref);
        match app::edit_config(|text| keymap::write_binding(text, name, change)) {
            Ok(text) => self.reload_settings(text, cx),
            Err(error) => eprintln!("bareframe: saving keyboard shortcut: {error}"),
        }
    }

    fn open_config_file(&self) {
        let Some(path) = app::config_file() else {
            return;
        };
        if !path.exists()
            && let Err(error) = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&path, "# bareframe settings (key = value)\n"))
        {
            eprintln!("bareframe: {}: {error}", path.display());
            return;
        }
        if let Err(error) = std::process::Command::new("/usr/bin/open")
            .arg("-t")
            .arg(&path)
            .spawn()
        {
            eprintln!("bareframe: opening {}: {error}", path.display());
        }
    }

    /// Reloads the settings when the file changes on disk, unless the change is our own
    /// write. A font size set with Cmd+= is not saved, so a reload returns to the file's.
    pub(super) fn watch_config(&mut self, cx: &mut Context<Self>) {
        let Some(path) = app::config_file() else {
            return;
        };
        let Some(dir) = path.parent().map(std::path::Path::to_path_buf) else {
            return;
        };
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        // The folder also holds the layout state, rewritten every few seconds; only the
        // settings file (or its temporary copy, renamed over it) matters here.
        let name = path.file_name().map(std::ffi::OsStr::to_owned);
        let relevant = move |event: &notify::Event| {
            event.paths.iter().any(|changed| {
                changed.file_name() == name.as_deref() || changed.file_stem() == name.as_deref()
            })
        };
        let reload = move |view: &mut Self, cx: &mut Context<Self>| {
            // A file caught mid-save or unreadable for a moment keeps the settings as
            // they are; a deleted one means the defaults.
            match app::read_optional(&path) {
                Ok(text) => view.reload_settings(text.unwrap_or_default(), cx),
                Err(error) => eprintln!("bareframe: {}: {error}", path.display()),
            }
        };
        self._config_watch = watch(
            &dir,
            RecursiveMode::NonRecursive,
            Duration::from_millis(200),
            relevant,
            reload,
            cx,
        );
    }

    /// Settings › Reload settings file (Ghostty's `reload_config`).
    pub(super) fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        let Some(path) = app::config_file() else {
            return;
        };
        let text = match app::read_optional(&path) {
            Ok(text) => text.unwrap_or_default(),
            Err(error) => return eprintln!("bareframe: {}: {error}", path.display()),
        };
        self.config_text.clear();
        self.reload_settings(text, cx);
    }

    fn reload_settings(&mut self, text: String, cx: &mut Context<Self>) {
        if text == self.config_text {
            return;
        }
        let (settings, warnings) = Settings::parse(&text);
        for warning in warnings {
            eprintln!("bareframe: settings: {warning}");
        }
        self.config_text = text;
        self.base_font_size = settings.font_size();
        self.settings = settings;
        self.apply_settings(cx);
    }
}
