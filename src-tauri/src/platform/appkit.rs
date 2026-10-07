//! AppKit pieces the tray and windows need that Tauri does not expose: the status item's
//! frame, image and accessibility label, the current modifier keys, per-window occlusion
//! observers (D-036), the Esc key monitor for the popover, and placing a window by its
//! AppKit frame. All of it runs on the main thread; every `unsafe` block has a `SAFETY:`
//! comment.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::AnyThread;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{
    NSAccessibility, NSBitmapFormat, NSBitmapImageRep, NSDeviceRGBColorSpace, NSEvent, NSEventMask,
    NSEventModifierFlags, NSImage, NSStatusItem, NSVariableStatusItemLength, NSWindow,
    NSWindowDidChangeOcclusionStateNotification, NSWindowOcclusionState,
};
use objc2_foundation::{
    NSInteger, NSNotification, NSNotificationCenter, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString,
};
use tauri::{Runtime, WebviewWindow};

use crate::popover::Rect;

fn rect(r: NSRect) -> Rect {
    Rect {
        x: r.origin.x,
        y: r.origin.y,
        w: r.size.width,
        h: r.size.height,
    }
}

/// The `NSWindow` behind a Tauri window.
pub fn ns_window<R: Runtime>(w: &WebviewWindow<R>) -> Option<Retained<NSWindow>> {
    let ptr = w.ns_window().ok()?.cast::<NSWindow>();
    // SAFETY: Tauri returns the window's live NSWindow pointer (or null); retaining it
    // keeps it valid for as long as the returned handle lives.
    unsafe { Retained::retain(ptr) }
}

/// The status item button's frame and the visible frame of the screen it is on.
pub fn status_item_frames(item: &NSStatusItem) -> Option<(Rect, Rect)> {
    let mtm = MainThreadMarker::new()?;
    let window = item.button(mtm)?.window()?;
    let screen = window.screen()?;
    Some((rect(window.frame()), rect(screen.visibleFrame())))
}

/// Sets the status item button's VoiceOver label.
pub fn set_status_item_label(item: &NSStatusItem, label: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    if let Some(button) = item.button(mtm) {
        button.setAccessibilityLabel(Some(&NSString::from_str(label)));
    }
}

/// Sets the status item button's image to a template image built straight from
/// premultiplied RGBA (`width * height * 4` bytes), shown at `size_pt` points.
///
/// This replaces tray-icon's `set_icon_with_as_template` for frames that keep the image's
/// size: no PNG encode and decode, and the image reaches the button already at its
/// point size and flagged as a template, so the button sees one change instead of an
/// image at pixel size that is then shrunk. It does not resize the status item; a size
/// change goes through tray-icon, which also resizes its click target. Returns false,
/// changing nothing, when the button or the bitmap is unavailable.
pub fn set_status_item_image(
    item: &NSStatusItem,
    rgba: &[u8],
    width: u32,
    height: u32,
    size_pt: (f64, f64),
) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(button) = item.button(mtm) else {
        return false;
    };
    let (Ok(w), Ok(h)) = (NSInteger::try_from(width), NSInteger::try_from(height)) else {
        return false;
    };
    let row = w.saturating_mul(4);
    let Some(len) = usize::try_from(row.saturating_mul(h))
        .ok()
        .filter(|&len| len > 0 && len == rgba.len())
    else {
        return false;
    };
    // SAFETY: null planes make AppKit allocate the bitmap itself, sized from the other
    // arguments: `w` x `h` meshed pixels of four 8-bit samples (RGBA, premultiplied alpha
    // last: format 0), `row` bytes per row. `NSDeviceRGBColorSpace` is an AppKit constant
    // that lives for the process.
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bitmapFormat_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            w,
            h,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            NSBitmapFormat::empty(),
            row,
            32,
        )
    };
    let Some(rep) = rep else { return false };
    let dst = rep.bitmapData();
    if dst.is_null() || rep.bytesPerRow() != row || rep.pixelsHigh() != h {
        return false;
    }
    // SAFETY: `dst` is the rep's own buffer of `bytesPerRow * pixelsHigh` = `len` bytes
    // (checked above), `rgba` holds `len` bytes, and a fresh AppKit allocation cannot
    // overlap a Rust slice. `rep` is alive for the copy.
    unsafe { std::ptr::copy_nonoverlapping(rgba.as_ptr(), dst, len) };
    let size = NSSize::new(size_pt.0, size_pt.1);
    rep.setSize(size);
    let image = NSImage::initWithSize(NSImage::alloc(), size);
    image.addRepresentation(&rep);
    image.setTemplate(true);
    button.setImage(Some(&image));
    true
}

/// Sets the status item's `autosaveName`, under which macOS saves and restores its
/// ⌘-drag position (D-037). Main thread only; does nothing elsewhere.
pub fn set_status_item_autosave_name(item: &NSStatusItem, name: &str) {
    if MainThreadMarker::new().is_none() {
        return;
    }
    item.setAutosaveName(Some(&NSString::from_str(name)));
}

/// Pins the status item's length to the width it has now (`pin`), or lets it follow its
/// content again. A pinned item does not re-measure its button on every image swap; the
/// width it keeps is the one AppKit laid out for the current image, so nothing moves.
/// Returns the pinned width in points (`None` when unpinned or unavailable).
pub fn pin_status_item_length(item: &NSStatusItem, pin: bool) -> Option<f64> {
    if !pin {
        item.setLength(NSVariableStatusItemLength);
        return None;
    }
    let mtm = MainThreadMarker::new()?;
    let width = item.button(mtm)?.frame().size.width;
    (width > 0.0).then(|| {
        item.setLength(width);
        width
    })
}

/// Whether Control is held right now (Control-click opens the menu, like a right click).
pub fn control_key_down() -> bool {
    NSEvent::modifierFlags_class().contains(NSEventModifierFlags::Control)
}

/// Moves and resizes `window` to `frame` (AppKit coordinates).
pub fn set_frame(window: &NSWindow, frame: Rect) {
    let r = NSRect::new(
        NSPoint::new(frame.x, frame.y),
        NSSize::new(frame.w, frame.h),
    );
    window.setFrame_display(r, false);
}

type Token = Retained<ProtocolObject<dyn NSObjectProtocol>>;

thread_local! {
    /// Occlusion observer tokens per window label, removed when the window is destroyed.
    /// Only touched on the main thread, where windows are created and destroyed.
    static OBSERVERS: RefCell<HashMap<String, Token>> = RefCell::new(HashMap::new());
}

/// Calls `on_change(visible)` whenever `window`'s occlusion state changes; `visible` is
/// true when the window is on screen and at least partly unoccluded. Replaces any
/// observer already registered for `label`.
pub fn observe_occlusion(label: &str, window: &NSWindow, on_change: impl Fn(bool) + 'static) {
    let block = RcBlock::new(move |note: NonNull<NSNotification>| {
        // SAFETY: AppKit passes a valid notification for the duration of the call.
        let note = unsafe { note.as_ref() };
        let Some(obj) = note.object() else { return };
        // SAFETY: this notification's object is always the NSWindow that changed (the
        // observer is registered with that window as the object filter).
        let win: &NSWindow = unsafe { &*(Retained::as_ptr(&obj).cast::<NSWindow>()) };
        let visible = win.isVisible()
            && win
                .occlusionState()
                .contains(NSWindowOcclusionState::Visible);
        on_change(visible);
    });
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: the name is AppKit's constant; filtering on `window` limits the block to that
    // window; with no queue the block runs synchronously on the posting (main) thread.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWindowDidChangeOcclusionStateNotification),
            Some(window as &AnyObject),
            None,
            &block,
        )
    };
    let old = OBSERVERS.with(|o| o.borrow_mut().insert(label.to_owned(), token));
    if let Some(old) = old {
        // SAFETY: `old` is a token this center returned.
        unsafe { center.removeObserver(old.as_ref()) };
    }
}

/// Removes `label`'s occlusion observer (the window is gone).
pub fn remove_occlusion_observer(label: &str) {
    if let Some(token) = OBSERVERS.with(|o| o.borrow_mut().remove(label)) {
        // SAFETY: `token` is a token the default center returned.
        unsafe { NSNotificationCenter::defaultCenter().removeObserver(token.as_ref()) };
    }
}

/// Esc (key code 53) pressed in `window` calls `on_esc` and is swallowed; every other key
/// passes through. The monitor lives for the rest of the process.
pub fn on_escape_in(window: &NSWindow, on_esc: impl Fn() + 'static) {
    let target = std::ptr::from_ref(window) as usize;
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit passes a valid event for the duration of the call.
        let e = unsafe { event.as_ref() };
        let in_target = MainThreadMarker::new()
            .and_then(|mtm| e.window(mtm))
            .is_some_and(|w| Retained::as_ptr(&w) as usize == target);
        if in_target && e.keyCode() == 53 {
            on_esc();
            return std::ptr::null_mut();
        }
        event.as_ptr()
    });
    // SAFETY: a local key-down monitor runs the block on the main thread for events
    // dispatched to this app; returning the event (or null to swallow it) is the
    // documented contract.
    let token = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
    };
    // App lifetime: never removed.
    std::mem::forget(token);
}
