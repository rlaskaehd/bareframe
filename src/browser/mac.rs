//! What Chromium needs from AppKit: GPUI's application class taught the event protocol
//! Chromium expects of `NSApp`, the browser's own NSView placed inside GPUI's view, and the
//! keyboard (the window's first responder) handed between the two.
#![allow(unsafe_code)]
use gpui::{Bounds, Pixels, Window};
use objc2::{
    Encode, Encoding, class, ffi,
    runtime::{AnyClass, AnyObject, Bool, Imp, MessageReceiver, Sel},
    sel,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    cell::RefCell,
    ffi::c_void,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

/// Whether `sendEvent:` is running, which Chromium asks through `isHandlingSendEvent`.
static HANDLING: AtomicBool = AtomicBool::new(false);
/// GPUI's own `sendEvent:`, called from the replacement.
static SEND_EVENT: OnceLock<Imp> = OnceLock::new();
/// `NSEventTypeApplicationDefined`, and the subtype marking bareframe's own.
const APPLICATION_DEFINED: u64 = 15;
const BETWEEN_EVENTS: i16 = 0x6266;

thread_local! {
    /// Work waiting for the event posted by `between_events`.
    static WAITING: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "C-unwind" fn is_handling(_: *mut AnyObject, _: Sel) -> Bool {
    Bool::new(HANDLING.load(Ordering::Relaxed))
}

unsafe extern "C-unwind" fn set_handling(_: *mut AnyObject, _: Sel, value: Bool) {
    HANDLING.store(value.as_bool(), Ordering::Relaxed);
}

unsafe extern "C-unwind" fn send_event(app: *mut AnyObject, sel: Sel, event: *mut AnyObject) {
    // SAFETY: `event` is the NSEvent being sent; `subtype` is asked only of the type that has it.
    let ours = unsafe {
        let kind: u64 = event.send_message(Sel::register(c"type"), ());
        kind == APPLICATION_DEFINED && {
            let subtype: i16 = objc2::msg_send![event, subtype];
            subtype == BETWEEN_EVENTS
        }
    };
    let was = HANDLING.swap(true, Ordering::Relaxed);
    if ours {
        for work in WAITING.take() {
            work();
        }
    } else if let Some(&original) = SEND_EVENT.get() {
        // SAFETY: `original` is the `sendEvent:` implementation this one replaced.
        unsafe {
            let original: unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) =
                std::mem::transmute(original);
            original(app, sel, event);
        }
    }
    HANDLING.store(was, Ordering::Relaxed);
}

/// Chromium's message pump requires `NSApp` to implement `CrAppControlProtocol`. GPUI's
/// `GPUIApplication` is taught it at run time, which leaves GPUI itself untouched: the
/// two methods, a `sendEvent:` that records when it runs, and the protocols, which the
/// framework registered when it was loaded.
pub fn teach_application() -> Result<(), String> {
    static TAUGHT: OnceLock<Result<(), String>> = OnceLock::new();
    TAUGHT.get_or_init(teach).clone()
}

fn teach() -> Result<(), String> {
    let class = AnyClass::get(c"GPUIApplication").ok_or("GPUI's application class is missing")?;
    let class = std::ptr::from_ref(class).cast_mut();
    // SAFETY: adds methods with matching type encodings to a registered class, on the main
    // thread before any browser exists.
    unsafe {
        let imp = |function: *const ()| std::mem::transmute::<*const (), Imp>(function);
        ffi::class_addMethod(
            class,
            sel!(isHandlingSendEvent),
            imp(is_handling as *const ()),
            c"B@:".as_ptr(),
        );
        ffi::class_addMethod(
            class,
            sel!(setHandlingSendEvent:),
            imp(set_handling as *const ()),
            c"v@:B".as_ptr(),
        );
        let method = ffi::class_getInstanceMethod(class, sel!(sendEvent:));
        let original = ffi::method_getImplementation(method).ok_or("no sendEvent:")?;
        SEND_EVENT.set(original).map_err(|_| "taught twice")?;
        let replacement = imp(send_event as *const ());
        // GPUI's class may inherit `sendEvent:`; then the override is added, else replaced.
        if !ffi::class_addMethod(class, sel!(sendEvent:), replacement, c"v@:@".as_ptr()).as_bool() {
            ffi::method_setImplementation(method.cast_mut(), replacement);
        }
        for name in [c"CrAppProtocol", c"CrAppControlProtocol", c"CefAppProtocol"] {
            let protocol = ffi::objc_getProtocol(name.as_ptr());
            if !protocol.is_null() {
                ffi::class_addProtocol(class, protocol);
            }
        }
    }
    Ok(())
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopCopyCurrentMode(run_loop: *mut c_void) -> *const c_void;
    fn CFRelease(object: *const c_void);
}

/// Whether the main run loop is in a pass (drawing, timers, a menu being tracked), rather
/// than between events, where `-[NSApplication run]` hands them out.
pub fn in_run_loop() -> bool {
    // SAFETY: plain CoreFoundation calls; the copied mode is released.
    unsafe {
        let mode = CFRunLoopCopyCurrentMode(CFRunLoopGetCurrent());
        if !mode.is_null() {
            CFRelease(mode);
        }
        !mode.is_null()
    }
}

/// Runs `work` from `sendEvent:` once the events already queued are handled, outside any
/// run loop pass (see `in_run_loop`): an event is posted for it.
pub fn between_events(work: impl FnOnce() + 'static) -> Result<(), String> {
    teach_application()?;
    WAITING.with_borrow_mut(|waiting| waiting.push(Box::new(work)));
    #[repr(C)]
    struct CGPoint(f64, f64);
    // SAFETY: matches CoreGraphics' `CGPoint`.
    unsafe impl Encode for CGPoint {
        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    }
    // SAFETY: makes and posts an application-defined NSEvent on the main thread.
    unsafe {
        let event: *mut AnyObject = objc2::msg_send![
            class!(NSEvent),
            otherEventWithType: APPLICATION_DEFINED,
            location: CGPoint(0.0, 0.0),
            modifierFlags: 0u64,
            timestamp: 0.0f64,
            windowNumber: 0isize,
            context: std::ptr::null_mut::<AnyObject>(),
            subtype: BETWEEN_EVENTS,
            data1: 0isize,
            data2: 0isize
        ];
        let app: *mut AnyObject = objc2::msg_send![class!(NSApplication), sharedApplication];
        let _: () = objc2::msg_send![app, postEvent: event, atStart: Bool::NO];
    }
    Ok(())
}

/// GPUI's view in `window`: browsers are added to it as children, and it takes the
/// keyboard back from them.
pub fn gpui_view(window: &Window) -> Option<*mut c_void> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::AppKit(handle) => Some(handle.ns_view.as_ptr()),
        _ => None,
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

// SAFETY: matches CoreGraphics' `CGRect` layout and encoding.
unsafe impl Encode for CGRect {
    const ENCODING: Encoding = Encoding::Struct(
        "CGRect",
        &[
            Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]),
            Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]),
        ],
    );
}

fn object(view: *mut c_void) -> Option<*mut AnyObject> {
    let view = view.cast::<AnyObject>();
    (!view.is_null()).then_some(view)
}

/// Puts `view` at `bounds` (GPUI's top-left points) inside its parent, AppKit's
/// bottom-left coordinates.
pub fn place(view: *mut c_void, bounds: Bounds<Pixels>) {
    let Some(view) = object(view) else { return };
    // SAFETY: `view` is the browser's live NSView, used on the main thread.
    unsafe {
        let parent: *mut AnyObject = objc2::msg_send![view, superview];
        if parent.is_null() {
            return;
        }
        let parent: CGRect = objc2::msg_send![parent, bounds];
        let (x, y, width, height) = (
            f64::from(f32::from(bounds.origin.x)),
            f64::from(f32::from(bounds.origin.y)),
            f64::from(f32::from(bounds.size.width)),
            f64::from(f32::from(bounds.size.height)),
        );
        let frame = CGRect {
            x,
            y: parent.height - y - height,
            width,
            height,
        };
        let _: () = objc2::msg_send![view, setFrame: frame];
    }
}

/// Whether the keyboard is with `view` or something inside it (Chromium's page view).
pub fn has_keyboard(view: *mut c_void) -> bool {
    let Some(view) = object(view) else {
        return false;
    };
    // SAFETY: as in `place`; the first responder is checked to be a view before asking.
    unsafe {
        let window: *mut AnyObject = objc2::msg_send![view, window];
        if window.is_null() {
            return false;
        }
        let responder: *mut AnyObject = objc2::msg_send![window, firstResponder];
        if responder.is_null() {
            return false;
        }
        let is_view: Bool = objc2::msg_send![responder, isKindOfClass: class!(NSView)];
        let inside: Bool = if is_view.as_bool() {
            objc2::msg_send![responder, isDescendantOf: view]
        } else {
            Bool::NO
        };
        inside.as_bool()
    }
}

/// Gives the keyboard to `view`. GPUI's view never takes it back on a click (it doesn't
/// accept first responder that way), so it is handed back explicitly whenever the page
/// stops having GPUI's focus.
pub fn give_keyboard(view: *mut c_void) {
    let Some(view) = object(view) else { return };
    // SAFETY: as in `place`.
    unsafe {
        let window: *mut AnyObject = objc2::msg_send![view, window];
        if !window.is_null() {
            let _: Bool = objc2::msg_send![window, makeFirstResponder: view];
        }
    }
}

/// Takes the keyboard from a browser's `view` back to its parent, GPUI's view, if the page
/// has it; AppKit would otherwise leave no first responder at all when the view goes.
fn release_keyboard(view: *mut c_void) {
    if !has_keyboard(view) {
        return;
    }
    let Some(object) = object(view) else { return };
    // SAFETY: as in `place`.
    let parent: *mut AnyObject = unsafe { objc2::msg_send![object, superview] };
    give_keyboard(parent.cast());
}

pub fn remove(view: *mut c_void) {
    release_keyboard(view);
    if let Some(view) = object(view) {
        // SAFETY: as in `place`.
        unsafe {
            let _: () = objc2::msg_send![view, removeFromSuperview];
        }
    }
}

pub fn set_hidden(view: *mut c_void, hidden: bool) {
    if hidden {
        release_keyboard(view);
    }
    if let Some(view) = object(view) {
        // SAFETY: as in `place`.
        unsafe {
            let _: () = objc2::msg_send![view, setHidden: Bool::new(hidden)];
        }
    }
}
