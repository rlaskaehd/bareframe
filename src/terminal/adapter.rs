//! Only this module knows GPUI's embedding and the pinned Ghostty adapter ABI.
//! It emits backend-independent events. Command output is still owned by Ghostty.
use crate::app::{Copy, Paste};
use bareframe_core::{
    config::LaunchConfig,
    ime::Composition,
    session::{SessionKey, TerminalEvent, TerminalEventKind},
};
use gpui::{
    AppContext, Bounds, Context, ElementInputHandler, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, KeyDownEvent, KeyUpEvent, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, ScrollDelta, ScrollWheelEvent, Subscription, Task, UTF16Selection, Window, canvas, div,
    point, prelude::*, px, size,
};
use gpui_libghostty::{
    __private::{
        KeyAction, Modifiers, MouseButton, MouseState, NativeEvent, NativeSurface,
        TerminalThemeState, spawn_surface,
    },
    ClipboardOperation, TerminalOptions,
};
use std::{ffi::CString, ops::Range};

pub struct GhosttyAdapter {
    key: SessionKey,
    // Task and subscriptions are explicitly cancelled before destroying the surface.
    tick_task: Option<Task<()>>,
    subscriptions: Vec<Subscription>,
    surface: Option<NativeSurface>,
    _theme: TerminalThemeState,
    focus: FocusHandle,
    bounds: Bounds<Pixels>,
    composition: Composition,
    started: bool,
    exited: bool,
}

impl EventEmitter<TerminalEvent> for GhosttyAdapter {}

impl GhosttyAdapter {
    pub fn spawn<T: 'static>(
        config: &LaunchConfig,
        key: SessionKey,
        window: &mut Window,
        cx: &mut Context<T>,
    ) -> Result<Entity<Self>, String> {
        let mut options = TerminalOptions::new(config.command(), config.cwd.clone());
        options.quiet_login = true;
        options.clipboard_approval = Some(std::sync::Arc::new(|request| {
            request.operation == ClipboardOperation::Paste
        }));
        // SAFETY: This entity creates, services and drops its child on GPUI's UI
        // thread. WorkspaceView releases it before its parent NSView is closed.
        #[allow(unsafe_code)]
        let (surface, theme) =
            unsafe { spawn_surface(options, window, f64::from(window.scale_factor()))? };
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Ok(cx.new(|cx| {
            let subscriptions = vec![
                cx.on_focus(&focus, window, |view: &mut Self, window, _| {
                    view.sync_focus(window)
                }),
                cx.on_blur(&focus, window, |view: &mut Self, window, _| {
                    view.clear_preedit();
                    view.sync_focus(window);
                }),
                cx.observe_window_activation(window, |view: &mut Self, window, _| {
                    view.sync_focus(window)
                }),
            ];
            Self {
                key,
                tick_task: None,
                subscriptions,
                surface: Some(surface),
                _theme: theme,
                focus,
                bounds: Bounds::default(),
                composition: Composition::default(),
                started: false,
                exited: false,
            }
        }))
    }

    fn emit(&self, kind: TerminalEventKind, cx: &mut Context<Self>) {
        cx.emit(TerminalEvent {
            key: self.key,
            kind,
        });
    }

    fn service(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(surface) = self.surface.as_mut() else {
            return false;
        };
        surface.tick();
        let mut observations = Vec::new();
        while let Some(event) = surface.take_event() {
            observations.push(match event {
                NativeEvent::Title(title) => TerminalEventKind::TitleChanged(title),
                NativeEvent::WorkingDirectory(cwd) => {
                    TerminalEventKind::WorkingDirectoryChanged(cwd.into())
                }
                NativeEvent::CommandFinished {
                    exit_code,
                    duration_ns,
                } => TerminalEventKind::CommandFinished {
                    exit_code,
                    duration_millis: duration_ns / 1_000_000,
                },
                NativeEvent::Gap { dropped } => TerminalEventKind::ObservationGap { dropped },
            });
        }
        let alive = surface.is_alive();
        if !self.started {
            self.started = true;
            self.emit(TerminalEventKind::Started, cx);
        }
        for event in observations {
            self.emit(event, cx);
        }
        if !alive && !self.exited {
            self.exited = true;
            self.emit(TerminalEventKind::Exited { code: None }, cx);
        }
        alive
    }

    fn start_ticking(&mut self, cx: &mut Context<Self>) {
        if self.tick_task.is_some() || self.exited || !self.service(cx) {
            return;
        }
        let Some(surface) = &self.surface else {
            return;
        };
        let wakeup = surface.wakeup();
        let view = cx.entity().downgrade();
        self.tick_task = Some(cx.spawn(async move |_, cx| {
            loop {
                wakeup.wait().await;
                if !matches!(view.update(cx, |view, cx| view.service(cx)), Ok(true)) {
                    break;
                }
            }
        }));
    }

    fn sync_focus(&mut self, window: &Window) {
        if let Some(surface) = &mut self.surface {
            surface.set_focus(self.focus.is_focused(window) && window.is_window_active());
        }
    }

    fn clear_preedit(&mut self) {
        self.composition.clear();
        if let Some(surface) = &mut self.surface {
            surface.preedit(c"");
        }
    }

    pub fn shutdown(&mut self) {
        self.tick_task.take();
        self.subscriptions.clear();
        self.clear_preedit();
        self.surface.take();
    }

    fn key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if self.exited {
            return;
        }
        let key = &event.keystroke;
        if key.key == "backspace" && self.composition.backspace_discards_commit() {
            cx.stop_propagation();
            return;
        }
        self.send_committed();
        // GPUI gives active IMEs the event before this callback. For ordinary
        // keys, consume the event after Ghostty encodes it, avoiding duplicate
        // default text insertion. Option/dead keys still go through AppKit.
        if key
            .key_char
            .as_ref()
            .is_some_and(|text| text.chars().all(|ch| !ch.is_control()))
            && !key.modifiers.control
            && !key.modifiers.platform
            && key.modifiers.alt
        {
            return;
        }
        if let Some(surface) = &mut self.surface {
            surface.send_key(
                if event.is_held {
                    KeyAction::Repeat
                } else {
                    KeyAction::Press
                },
                &key.key,
                key.key_char.as_deref(),
                modifiers(key.modifiers),
            );
            cx.stop_propagation();
        }
    }

    fn key_up(&mut self, event: &KeyUpEvent) {
        if self.composition.active() {
            return;
        }
        let key = &event.keystroke;
        if let Some(surface) = &mut self.surface {
            surface.send_key(KeyAction::Release, &key.key, None, modifiers(key.modifiers));
        }
    }

    fn mouse_position(&mut self, position: Point<Pixels>, input: gpui::Modifiers) {
        if let Some(surface) = &mut self.surface {
            surface.mouse_position(
                f64::from(f32::from(position.x - self.bounds.origin.x)),
                f64::from(f32::from(position.y - self.bounds.origin.y)),
                modifiers(input),
            );
        }
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        self.sync_focus(window);
        self.mouse_position(event.position, event.modifiers);
        if let Some(surface) = &mut self.surface {
            surface.mouse_button(
                MouseState::Press,
                mouse_button(event.button),
                modifiers(event.modifiers),
            );
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent) {
        self.mouse_position(event.position, event.modifiers);
        if let Some(surface) = &mut self.surface {
            surface.mouse_button(
                MouseState::Release,
                mouse_button(event.button),
                modifiers(event.modifiers),
            );
        }
    }

    fn scroll(&mut self, event: &ScrollWheelEvent) {
        self.mouse_position(event.position, event.modifiers);
        let (x, y, precise) = match event.delta {
            ScrollDelta::Pixels(delta) => (
                f64::from(f32::from(delta.x)),
                f64::from(f32::from(delta.y)),
                true,
            ),
            ScrollDelta::Lines(delta) => (f64::from(delta.x), f64::from(delta.y), false),
        };
        if let Some(surface) = &mut self.surface {
            surface.mouse_scroll(x, y, precise);
        }
    }

    fn send_committed(&mut self) {
        let Some(committed) = self.composition.take_committed() else {
            return;
        };
        if let (Some(surface), Ok(text)) = (&mut self.surface, CString::new(committed)) {
            // ghostty_surface_text is paste, including bracketed-paste markers.
            // Match Ghostty's AppKit committedTextAction for IME/dictation.
            surface.key(
                KeyAction::Press,
                Modifiers::empty(),
                Modifiers::empty(),
                0,
                Some(&text),
                0,
            );
        }
    }

    fn binding(&mut self, action: &std::ffi::CStr) {
        self.send_committed();
        if let Some(surface) = &mut self.surface {
            surface.binding_action(action);
        }
    }
}

impl Drop for GhosttyAdapter {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl EntityInputHandler for GhosttyAdapter {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text = self.composition.slice(range.clone())?;
        *adjusted = Some(range);
        Some(text)
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.composition.selection(),
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.composition
            .active()
            .then(|| 0..self.composition.len_utf16())
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        trace_input(&format!("unmark {:?}", self.composition));
        self.clear_preedit();
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        trace_input(&format!(
            "commit {range:?} {text:?} before={:?}",
            self.composition
        ));
        if self.composition.commit(range, text).is_err() {
            return;
        }
        self.clear_preedit();
        // GPUI runs key_down for a key the IME passes on within this same event,
        // so delivery waits until the event is over.
        cx.spawn(async move |view, cx| {
            let _ = view.update(cx, |view, _| view.send_committed());
        })
        .detach();
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        trace_input(&format!(
            "mark {range:?} {text:?} selected={selected:?} before={:?}",
            self.composition
        ));
        if self.composition.replace(range, text, selected).is_err() {
            return;
        }
        if let (Some(surface), Ok(text)) =
            (&mut self.surface, CString::new(self.composition.text()))
        {
            surface.preedit(&text);
        }
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let [x, y, width, height] = self.surface.as_ref()?.ime_rect();
        Some(Bounds::new(
            bounds.origin + point(px(x as f32), px((y - height) as f32)),
            size(px((width as f32).max(1.0)), px((height as f32).max(1.0))),
        ))
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(0)
    }
    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.composition.len_utf16())
    }
    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        !self.exited && self.surface.is_some()
    }
}

impl Render for GhosttyAdapter {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_focus(window);
        self.start_ticking(cx);
        let weak = cx.entity().downgrade();
        let input = cx.entity();
        let focus = self.focus.clone();
        let mut element = div()
            .relative()
            .key_context("BareframeTerminal")
            .track_focus(&self.focus)
            .size_full()
            .min_h_0()
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let scale = f64::from(window.scale_factor());
                        let _ = weak.update(cx, |view, _| {
                            view.bounds = bounds;
                            if let Some(surface) = &mut view.surface {
                                surface.set_frame(
                                    f64::from(f32::from(bounds.origin.x)),
                                    f64::from(f32::from(bounds.origin.y)),
                                    f64::from(f32::from(bounds.size.width)),
                                    f64::from(f32::from(bounds.size.height)),
                                    scale,
                                );
                            }
                        });
                    },
                    move |bounds, _, window, cx| {
                        window.handle_input(
                            &focus,
                            ElementInputHandler::new(bounds, input.clone()),
                            cx,
                        );
                    },
                )
                .absolute()
                .size_full(),
            )
            .on_action(cx.listener(|view, _: &Copy, _, cx| {
                view.binding(c"copy_to_clipboard");
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|view, _: &Paste, _, cx| {
                view.binding(c"paste_from_clipboard");
                cx.stop_propagation();
            }))
            .on_key_down(cx.listener(|view, event, _, cx| view.key_down(event, cx)))
            .on_key_up(cx.listener(|view, event, _, _| view.key_up(event)))
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, _| {
                view.mouse_position(event.position, event.modifiers)
            }))
            .on_scroll_wheel(cx.listener(|view, event, _, _| view.scroll(event)));
        for button in [
            gpui::MouseButton::Left,
            gpui::MouseButton::Middle,
            gpui::MouseButton::Right,
        ] {
            element = element
                .on_mouse_down(
                    button,
                    cx.listener(|view, event, window, cx| view.mouse_down(event, window, cx)),
                )
                .on_mouse_up(
                    button,
                    cx.listener(|view, event, _, _| view.mouse_up(event)),
                );
        }
        element
    }
}

fn modifiers(value: gpui::Modifiers) -> Modifiers {
    let mut result = Modifiers::empty();
    for (set, flag) in [
        (value.shift, Modifiers::SHIFT),
        (value.control, Modifiers::CONTROL),
        (value.alt, Modifiers::ALT),
        (value.platform, Modifiers::SUPER),
    ] {
        if set {
            result.insert(flag);
        }
    }
    result
}

fn mouse_button(value: gpui::MouseButton) -> MouseButton {
    match value {
        gpui::MouseButton::Left => MouseButton::Left,
        gpui::MouseButton::Right => MouseButton::Right,
        gpui::MouseButton::Middle => MouseButton::Middle,
        _ => MouseButton::Unknown,
    }
}

fn trace_input(message: &str) {
    if crate::app::is_validation_instance() {
        use std::io::Write;
        if let Ok(mut log) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/target/validation/input-trace.txt"
            ))
        {
            let _ = writeln!(log, "{message}");
        }
    }
}
