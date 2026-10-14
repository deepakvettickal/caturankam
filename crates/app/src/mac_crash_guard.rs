//! Stops closing the window from crashing the app on macOS.
//!
//! AppKit attaches a KVO observer to every window's view for its automatic
//! Touch Bar support (even on Macs without a Touch Bar). On close it can try
//! to remove that observer a second time; the removal throws an
//! `NSRangeException` from a run-loop callback that nothing wraps in
//! `@try`/`@catch`, so the process aborts ("quit unexpectedly"). There's no
//! public API to opt out, so, like godotengine/godot#105804, this patches
//! winit's `WinitView` `removeObserver:forKeyPath:` to run in a `@try`/`@catch`
//! and swallow only that one exception. Anything else is re-thrown.
//!
//! Adapted from spotifast's `src/mac_touchbar_crash_guard.rs`
//! (https://github.com/crmne/spotifast/pull/532), used under the MIT licence:
//!
//! Copyright (c) 2026 Carmine Paolino
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.

use objc2::exception::catch;
use objc2::ffi::{
    class_getInstanceMethod, class_replaceMethod, method_getImplementation, method_getTypeEncoding,
};
use objc2::runtime::{AnyClass, AnyObject, Imp, MethodImplementation, Sel};
use objc2::sel;
use std::sync::OnceLock;

type RemoveObserver =
    unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject);

static ORIGINAL: OnceLock<Imp> = OnceLock::new();

/// The signature of this one AppKit bug in the exception's reason.
const KNOWN_BENIGN: &str = "_NSTouchBarFinderObservation";

/// Patches `WinitView`. Call once a window exists (so the class is
/// registered); later calls do nothing.
pub fn install() {
    if ORIGINAL.get().is_some() {
        return;
    }
    let Some(class) = AnyClass::get(c"WinitView") else {
        return; // a winit that renamed its view class: nothing to patch
    };
    let selector = sel!(removeObserver:forKeyPath:);
    // SAFETY: a registered class and a valid selector; null if not found.
    let method = unsafe { class_getInstanceMethod(class, selector) };
    if method.is_null() {
        return;
    }
    // SAFETY: `method` is non-null and came from `class_getInstanceMethod`.
    let (Some(original), types) = (unsafe { method_getImplementation(method) }, unsafe {
        method_getTypeEncoding(method)
    }) else {
        return;
    };
    if ORIGINAL.set(original).is_err() {
        return;
    }
    let replacement: RemoveObserver = guarded_remove_observer;
    // SAFETY: `replacement` matches `removeObserver:forKeyPath:` exactly,
    // `(id, SEL, id, id) -> void`, and `types` is that method's own encoding.
    unsafe {
        class_replaceMethod(
            (class as *const AnyClass).cast_mut(),
            selector,
            replacement.__imp(),
            types,
        );
    }
}

/// Calls the original inside `@try`/`@catch`, swallowing only the known
/// Touch Bar exception.
unsafe extern "C-unwind" fn guarded_remove_observer(
    this: *mut AnyObject,
    cmd: Sel,
    observer: *mut AnyObject,
    key_path: *mut AnyObject,
) {
    let Some(original) = ORIGINAL.get().copied() else {
        return;
    };
    // SAFETY: `original` is the real `removeObserver:forKeyPath:`, same signature.
    let original: RemoveObserver = unsafe { std::mem::transmute(original) };
    let outcome = catch(std::panic::AssertUnwindSafe(|| unsafe {
        original(this, cmd, observer, key_path);
    }));
    if let Err(Some(exception)) = outcome
        && !is_known_benign(&exception.to_string())
    {
        objc2::exception::throw(exception); // a different bug: crash and report as before
    }
}

fn is_known_benign(reason: &str) -> bool {
    reason.contains(KNOWN_BENIGN)
}

#[cfg(test)]
mod tests {
    use super::is_known_benign;

    #[test]
    fn swallows_only_the_touch_bar_exception() {
        // The exact reason from our crash log.
        assert!(is_known_benign(
            "Cannot remove an observer <_NSTouchBarFinderObservation 0x7082a84a0> for the key \
             path \"nextResponder\" from <WinitView 0x709107300> because it is not registered \
             as an observer."
        ));
        assert!(!is_known_benign(
            "Cannot remove an observer <SomeOtherObserver 0x1> for the key path \"frame\" from \
             <WinitView 0x2> because it is not registered as an observer."
        ));
    }
}
