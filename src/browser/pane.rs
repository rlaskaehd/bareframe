//! A browser pane: Chromium's page as a child view under a toolbar with back, forward,
//! reload and the address. Chromium starts the first time a browser pane is made; a page
//! that opens a window opens here instead.
//!
//! The page's view sits above GPUI's, so it is hidden when its tab isn't shown, and a menu
//! over it shows a snapshot of the page instead. The keyboard (AppKit's first responder)
//! follows GPUI's focus: the page has it exactly while its focus handle is focused.
use super::{
    client::{BrowserEvent, Keyboard},
    engine, mac,
};
use crate::{
    text_field::{TextField, TextFieldEvent},
    theme::{self, Theme, hsla},
};
use cef::*;
use futures::{StreamExt, future::Either};
use gpui::{
    Bounds, Context, Entity, EventEmitter, FocusHandle, Image, ImageFormat, KeyBinding, Pixels,
    RenderImage, Subscription, Task, Window, actions, canvas, div, img, prelude::*, px,
};
use std::{
    path::Path,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

actions!(browser, [Copy, Cut, Paste, SelectAll, Undo, Redo]);

/// Editing keys go to the page's frame: Chromium takes them from the Edit menu, which
/// bareframe's doesn't send it.
const KEY_CONTEXT: &str = "BrowserPage";

pub fn key_bindings() -> Vec<KeyBinding> {
    let context = Some(KEY_CONTEXT);
    vec![
        KeyBinding::new("cmd-c", Copy, context),
        KeyBinding::new("cmd-x", Cut, context),
        KeyBinding::new("cmd-v", Paste, context),
        KeyBinding::new("cmd-a", SelectAll, context),
        KeyBinding::new("cmd-z", Undo, context),
        KeyBinding::new("cmd-shift-z", Redo, context),
    ]
}

/// How long a menu waits for the page's snapshot before covering a blank page.
const SNAPSHOT: Duration = Duration::from_millis(250);

pub enum BrowserPaneEvent {
    /// The page's title or address changed; headers and the sidebar show them.
    Changed,
    /// The page was clicked: its pane becomes the active one.
    Focused,
    /// The user opened a link in a new window: it becomes a new browser tab.
    OpenTab(String),
}

/// What shows while a menu is over the page.
enum Frozen {
    No,
    /// The page stays up until its snapshot is ready.
    Capturing,
    Shot(Option<Arc<RenderImage>>),
}

pub struct BrowserPane {
    /// The engine's browser; none before the first frame, or once it closed.
    browser: Option<i32>,
    /// Why there is no page: Chromium missing from this build, failing to start, or the page
    /// having closed itself.
    error: Option<String>,
    /// The page's address, or the one to open once the browser exists.
    url: String,
    title: String,
    loading: bool,
    back: bool,
    forward: bool,
    address: Entity<TextField>,
    /// The tab is in front.
    visible: bool,
    frozen: Frozen,
    starting: bool,
    theme: &'static Theme,
    page: FocusHandle,
    keyboard: Keyboard,
    _events: Option<Task<()>>,
    /// Snapshots no longer shown; GPUI keeps their textures until the window drops them.
    stale: Vec<Arc<RenderImage>>,
    _snapshot: Option<Task<()>>,
    _subscriptions: [Subscription; 5],
}

impl EventEmitter<BrowserPaneEvent> for BrowserPane {}

impl BrowserPane {
    pub fn new(
        url: String,
        data: &Path,
        theme: &'static Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let address = cx.new(|cx| {
            let mut field = TextField::search(theme.accent, window, cx);
            field.select_on_focus();
            field.set_text(&url, cx);
            field
        });
        let entered = cx.subscribe_in(&address, window, |pane, _, event, window, cx| {
            if let TextFieldEvent::Confirm(text) = event {
                pane.load(&address_for(text), window, cx);
            }
        });
        let release = cx.on_release(|pane: &mut Self, _| pane.close());
        let snapshots = cx.on_release_in(window, |pane: &mut Self, window, _| {
            if let Frozen::Shot(Some(image)) = std::mem::replace(&mut pane.frozen, Frozen::No) {
                pane.stale.push(image);
            }
            pane.drop_stale(window);
        });
        let page = cx.focus_handle();
        let keyboard = Keyboard::default();
        let focused = cx.on_focus(&page, window, |pane, _, _| {
            pane.keyboard.store(true, Ordering::Relaxed);
            if let Some(host) = pane.browser.and_then(engine::host) {
                host.set_focus(1);
            }
        });
        let blurred = cx.on_blur(&page, window, |pane, window, _| {
            pane.keyboard.store(false, Ordering::Relaxed);
            let Some(id) = pane.browser else { return };
            if let Some(host) = engine::host(id) {
                host.set_focus(0);
            }
            // GPUI's view never takes the keyboard back by itself.
            if engine::view(id).is_some_and(mac::has_keyboard)
                && let Some(view) = mac::gpui_view(window)
            {
                mac::give_keyboard(view);
            }
        });
        Self {
            browser: None,
            // Chromium starts here, from the user's action or the app's launch, where no run
            // loop pass is under way (see engine::start).
            error: engine::start(data, cx).err(),
            title: String::new(),
            url,
            loading: false,
            back: false,
            forward: false,
            address,
            visible: true,
            frozen: Frozen::No,
            starting: false,
            theme,
            page,
            keyboard,
            _events: None,
            stale: Vec::new(),
            _snapshot: None,
            _subscriptions: [entered, release, snapshots, focused, blurred],
        }
    }

    /// Closes the page for good, before the window goes.
    pub fn close(&mut self) {
        if let Some(id) = self.browser.take() {
            engine::close(id);
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// The page's title, else its address.
    pub fn title(&self) -> String {
        if self.title.is_empty() {
            self.url.clone()
        } else {
            self.title.clone()
        }
    }

    pub fn set_theme(&mut self, theme: &'static Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// Shows or hides the page with its tab.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.visible = visible;
        self.show_view();
        cx.notify();
    }

    /// A menu or prompt is drawn over the panes: the page's view would hide it, so a
    /// snapshot of the page stands in for it, taken over DevTools.
    pub fn set_frozen(&mut self, frozen: bool, cx: &mut Context<Self>) {
        if let Frozen::Shot(Some(image)) = std::mem::replace(&mut self.frozen, Frozen::No) {
            self.stale.push(image);
        }
        if frozen {
            self.frozen = Frozen::Capturing;
            let shot = self
                .browser
                .filter(|_| self.visible)
                .map(|id| engine::devtools(id, "Page.captureScreenshot", serde_json::json!({})));
            let timeout = cx.background_executor().timer(SNAPSHOT);
            self._snapshot = Some(cx.spawn(async move |pane, cx| {
                let image = match shot {
                    Some(shot) => {
                        match futures::future::select(std::pin::pin!(shot), timeout).await {
                            Either::Left((Ok(result), _)) => decode(&result),
                            _ => None,
                        }
                    }
                    None => None,
                };
                pane.update(cx, |pane, cx| {
                    if matches!(pane.frozen, Frozen::Capturing) {
                        let image =
                            image.and_then(|image| image.to_image_data(cx.svg_renderer()).ok());
                        pane.frozen = Frozen::Shot(image);
                        pane.show_view();
                        cx.notify();
                    }
                })
                .ok();
            }));
        }
        self.show_view();
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.browser.is_some() && !self.url.is_empty() && self.url != "about:blank" {
            // Keys go to the page (see `on_focus` in `new`).
            self.page.focus(window, cx);
        } else {
            // A blank page is waiting for an address.
            self.address
                .update(cx, |address, cx| address.focus(window, cx));
        }
    }

    fn drop_stale(&mut self, window: &mut Window) {
        for image in self.stale.drain(..) {
            let _ = window.drop_image(image);
        }
    }

    fn show_view(&self) {
        if let Some(view) = self.browser.and_then(engine::view) {
            let hidden = !self.visible || matches!(self.frozen, Frozen::Shot(_));
            mac::set_hidden(view, hidden);
        }
    }

    fn load(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.url = url.to_owned();
        match self
            .browser
            .and_then(engine::browser)
            .and_then(|b| b.main_frame())
        {
            Some(frame) => {
                frame.load_url(Some(&CefString::from(url)));
                self.focus(window, cx);
            }
            None => cx.notify(),
        }
    }

    /// Makes the browser in the window, once the pane has a place: the first frame it is
    /// laid out in.
    fn start(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(parent) = mac::gpui_view(window) else {
            return;
        };
        let (sender, mut events) = futures::channel::mpsc::unbounded();
        let url = if self.url.is_empty() {
            "about:blank"
        } else {
            &self.url
        };
        let Some(id) = engine::open(parent, url, sender, self.keyboard.clone()) else {
            self.error = Some("Chromium could not open a page".into());
            return cx.notify();
        };
        self.browser = Some(id);
        if let Some(view) = engine::view(id) {
            mac::place(view, bounds);
            // A new page doesn't take the keyboard until its pane is focused.
            if !self.page.is_focused(window) && mac::has_keyboard(view) {
                mac::give_keyboard(parent);
            }
        }
        self.show_view();
        self._events = Some(cx.spawn_in(window, async move |pane, cx| {
            while let Some(event) = events.next().await {
                if pane
                    .update_in(cx, |pane, window, cx| pane.on_event(event, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
        // A restored page waited in its address bar for the browser.
        if !self.url.is_empty() && self.address.read(cx).is_focused(window) {
            self.page.focus(window, cx);
        }
    }

    fn on_event(&mut self, event: BrowserEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            BrowserEvent::Address(url) => {
                self.url = url;
                // A field being typed in keeps what the user typed.
                if !self.address.read(cx).is_focused(window) {
                    let url = self.url.clone();
                    self.address
                        .update(cx, |address, cx| address.set_text(&url, cx));
                }
            }
            BrowserEvent::Title(title) => self.title = title,
            BrowserEvent::Closed => {
                self.browser = None;
                self.error = Some("The page closed itself.".into());
            }
            BrowserEvent::Loading {
                loading,
                back,
                forward,
            } => (self.loading, self.back, self.forward) = (loading, back, forward),
            // A page may only open web pages this way, not files, scripts or Chromium's own,
            // and never in place of the page the user is reading.
            BrowserEvent::Popup(url) => {
                if is_web(&url) {
                    cx.emit(BrowserPaneEvent::OpenTab(url));
                }
                return;
            }
            BrowserEvent::Focused => {
                if !self.page.is_focused(window) {
                    self.page.focus(window, cx);
                    cx.emit(BrowserPaneEvent::Focused);
                }
                return;
            }
        }
        cx.emit(BrowserPaneEvent::Changed);
        cx.notify();
    }

    fn edit(&self, edit: fn(&Frame)) {
        if let Some(frame) = self
            .browser
            .and_then(engine::browser)
            .and_then(|b| b.focused_frame())
        {
            edit(&frame);
        }
    }

    fn render_toolbar(&self) -> impl IntoElement {
        let theme = self.theme;
        let button = |id: &'static str, icon: &'static str, enabled: bool| {
            div()
                .id(id)
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(24.0))
                .rounded_sm()
                .child(
                    gpui::svg()
                        .path(icon)
                        .size(px(14.0))
                        .text_color(hsla(if enabled {
                            theme.text
                        } else {
                            theme.text_muted
                        })),
                )
                .when(enabled, |this| {
                    this.hover(|style| style.bg(hsla(theme.tab_hover)))
                })
        };
        let browser = self.browser;
        let navigate = move |go: fn(&Browser)| {
            move |_: &gpui::ClickEvent, _: &mut Window, _: &mut gpui::App| {
                if let Some(browser) = browser.and_then(engine::browser) {
                    go(&browser);
                }
            }
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .h(px(34.0))
            .px_2()
            .border_b_1()
            .border_color(hsla(theme.border))
            .bg(hsla(theme.chrome))
            .child(
                button("browser-back", "icons/arrow-left.svg", self.back)
                    .on_click(navigate(|browser| browser.go_back())),
            )
            .child(
                button("browser-forward", "icons/arrow-right.svg", self.forward)
                    .on_click(navigate(|browser| browser.go_forward())),
            )
            .child(
                button(
                    "browser-reload",
                    if self.loading {
                        "icons/stop.svg"
                    } else {
                        "icons/rotate-cw.svg"
                    },
                    self.browser.is_some(),
                )
                .on_click(navigate(if self.loading {
                    |browser: &Browser| browser.stop_load()
                } else {
                    |browser: &Browser| browser.reload()
                })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px_2()
                    .py(px(3.0))
                    .rounded_md()
                    .border_1()
                    .border_color(hsla(theme.border))
                    .bg(hsla(theme.terminal_background))
                    .font_family(theme::UI_FONT)
                    .text_size(px(12.5))
                    .child(self.address.clone()),
            )
    }
}

impl Render for BrowserPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.drop_stale(window);
        let theme = self.theme;
        let pane = cx.entity().downgrade();
        let page = canvas(
            move |bounds, window, cx| {
                pane.update(cx, |pane, cx| {
                    if let Some(view) = pane.browser.and_then(engine::view) {
                        mac::place(view, bounds);
                    } else if pane.browser.is_none() && !pane.starting && pane.error.is_none() {
                        // Started after this frame, not while GPUI is laying it out.
                        pane.starting = true;
                        let pane = cx.entity().downgrade();
                        window.defer(cx, move |window, cx| {
                            pane.update(cx, |pane, cx| pane.start(bounds, window, cx))
                                .ok();
                        });
                    }
                })
                .ok();
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let snapshot = match &self.frozen {
            Frozen::Shot(Some(image)) => Some(image.clone()),
            _ => None,
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(hsla(theme.terminal_background))
            .child(self.render_toolbar())
            .child(
                div()
                    .track_focus(&self.page)
                    .key_context(KEY_CONTEXT)
                    .on_action(cx.listener(|pane, _: &Copy, _, _| pane.edit(|f| f.copy())))
                    .on_action(cx.listener(|pane, _: &Cut, _, _| pane.edit(|f| f.cut())))
                    .on_action(cx.listener(|pane, _: &Paste, _, _| pane.edit(|f| f.paste())))
                    .on_action(
                        cx.listener(|pane, _: &SelectAll, _, _| pane.edit(|f| f.select_all())),
                    )
                    .on_action(cx.listener(|pane, _: &Undo, _, _| pane.edit(|f| f.undo())))
                    .on_action(cx.listener(|pane, _: &Redo, _, _| pane.edit(|f| f.redo())))
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(page)
                    .when_some(snapshot, |this, image| {
                        this.child(img(image).absolute().inset_0().size_full())
                    })
                    .when_some(self.error.clone(), |this, error| {
                        this.child(
                            div()
                                .p_4()
                                .font_family(theme::UI_FONT)
                                .text_color(hsla(theme.text_muted))
                                .child(error),
                        )
                    }),
            )
    }
}

/// The PNG in a `Page.captureScreenshot` result.
fn decode(result: &serde_json::Value) -> Option<Image> {
    let data = base64_decode(Some(&CefString::from(result["data"].as_str()?)))?;
    let mut bytes = vec![0; data.size()];
    data.data(Some(&mut bytes), 0);
    Some(Image::from_bytes(ImageFormat::Png, bytes))
}

/// An `http` or `https` address, the only kind a page may open in its pane.
fn is_web(url: &str) -> bool {
    url.split_once(':').is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    })
}

/// What was typed in the address bar as a URL: kept if it names a scheme, made `https://`
/// if it looks like a host, else searched for.
fn address_for(text: &str) -> String {
    let text = text.trim();
    if text.contains("://")
        || ["about:", "data:", "file:"]
            .iter()
            .any(|s| text.starts_with(s))
    {
        return text.to_owned();
    }
    let host = text.split('/').next().unwrap_or("");
    // A local server usually speaks plain HTTP.
    if ["localhost", "127.0.0.1"]
        .iter()
        .any(|local| host.starts_with(local))
    {
        return format!("http://{text}");
    }
    if !text.contains(' ') && host.contains('.') {
        return format!("https://{text}");
    }
    let query: String = text
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            b' ' => "+".into(),
            _ => format!("%{byte:02X}"),
        })
        .collect();
    format!("https://www.google.com/search?q={query}")
}

#[cfg(test)]
mod tests {
    use super::{address_for, is_web};

    #[test]
    fn pages_may_only_open_web_pages() {
        assert!(is_web("https://a.b") && is_web("HTTP://x"));
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,x",
            "chrome://gpu",
        ] {
            assert!(!is_web(url), "{url}");
        }
    }

    #[test]
    fn typed_addresses_become_urls_or_searches() {
        assert_eq!(address_for("https://a.b/c"), "https://a.b/c");
        assert_eq!(address_for("about:blank"), "about:blank");
        assert_eq!(address_for("example.com/x"), "https://example.com/x");
        assert_eq!(address_for("localhost:3000"), "http://localhost:3000");
        assert_eq!(
            address_for("rust gpui 한"),
            "https://www.google.com/search?q=rust+gpui+%ED%95%9C"
        );
    }
}
