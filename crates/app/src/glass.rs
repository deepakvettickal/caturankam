//! Glass (macOS): the window server blurs whatever is behind the window, under
//! our own drawing. An `NSVisualEffectView` goes into the window's frame view,
//! below the content view, so it can never cover what egui draws; the title
//! bar turns transparent and the content runs up under it. Costs nothing per
//! frame: the blur is the compositor's, not ours.

use objc2::{ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::NSObjectProtocol;

/// Turns glass on or off. Returns the height of the title bar the content now
/// runs under (0 when off), so the UI can keep clear of it.
pub fn set(on: bool) -> f32 {
    let Some(mtm) = MainThreadMarker::new() else {
        return 0.0;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(window) = app.windows().firstObject() else {
        return 0.0;
    };
    let Some(content) = window.contentView() else {
        return 0.0;
    };
    // SAFETY: the content view of a live window has its frame view as superview.
    let Some(frame) = (unsafe { content.superview() }) else {
        return 0.0;
    };
    for view in frame.subviews() {
        if view.isKindOfClass(NSVisualEffectView::class()) {
            view.removeFromSuperview();
        }
    }
    let mut mask = window.styleMask();
    mask.set(NSWindowStyleMask::FullSizeContentView, on);
    window.setStyleMask(mask);
    window.setTitlebarAppearsTransparent(on);
    if !on {
        return 0.0;
    }
    let fx = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), frame.bounds());
    fx.setMaterial(NSVisualEffectMaterial::UnderWindowBackground);
    fx.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    fx.setState(NSVisualEffectState::Active);
    fx.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    frame.addSubview_positioned_relativeTo(&fx, NSWindowOrderingMode::Below, Some(&content));
    (window.frame().size.height - window.contentLayoutRect().size.height) as f32
}
