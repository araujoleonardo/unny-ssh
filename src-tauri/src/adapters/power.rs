/// Native notifications supplement the UI's visibility and elapsed-clock guards.
#[cfg(target_os = "macos")]
pub fn watch(app: tauri::AppHandle) {
    use objc2_app_kit::{
        NSWorkspace, NSWorkspaceWillPowerOffNotification, NSWorkspaceWillSleepNotification,
    };
    use objc2_foundation::NSNotification;
    use tauri::Emitter;
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    for name in unsafe {
        [
            NSWorkspaceWillSleepNotification,
            NSWorkspaceWillPowerOffNotification,
        ]
    } {
        let handle = app.clone();
        let callback = block2::RcBlock::new(move |_: std::ptr::NonNull<NSNotification>| {
            let _ = handle.emit_to("main", "app-suspend", ());
        });
        // No object or queue filter. The block captures only a thread-safe AppHandle.
        let observer = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &callback)
        };
        // Notification subscriptions last exactly as long as this process. Keeping
        // these two tokens alive avoids callbacks into a released subscription.
        std::mem::forget(observer);
    }
}
#[cfg(not(target_os = "macos"))]
pub fn watch(_app: tauri::AppHandle) {}
