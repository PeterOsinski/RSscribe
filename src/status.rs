use anyhow::Result;
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// Menu-bar status item that shows a colored dot: grey while idle, red while
/// dictation is recording. Created and updated on the main thread, which
/// AppKit requires for `NSStatusItem`.
pub struct StatusIcon {
    tray: TrayIcon,
    idle: Icon,
    recording: Icon,
}

impl StatusIcon {
    pub fn new() -> Result<Self> {
        init_appkit()?;

        let idle = dot_icon(150, 150, 150);
        let recording = dot_icon(235, 60, 45);
        let tray = TrayIconBuilder::new()
            .with_icon(idle.clone())
            .with_tooltip("voice-paste — idle")
            .with_icon_as_template(false)
            .build()?;
        Ok(Self { tray, idle, recording })
    }

    pub fn set_recording(&self, recording: bool) -> Result<()> {
        let (icon, tooltip) = if recording {
            (&self.recording, "voice-paste — recording")
        } else {
            (&self.idle, "voice-paste — idle")
        };
        self.tray.set_icon(Some(icon.clone()))?;
        self.tray.set_tooltip(Some(tooltip))?;
        Ok(())
    }
}

/// A plain CLI process has no `NSApplication`, so AppKit can't connect to the
/// window server and `NSStatusItem` creation aborts. Initialize one and run as
/// an "accessory" so no Dock icon appears.
#[cfg(target_os = "macos")]
fn init_appkit() -> Result<()> {
    let mtm = objc2::MainThreadMarker::new()
        .ok_or_else(|| anyhow::anyhow!("status icon must be created on the main thread"))?;
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Accessory);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn init_appkit() -> Result<()> {
    Ok(())
}

/// Run the AppKit run loop for a short slice so the status item gets placed in
/// the menu bar and redrawn. Called from the daemon's main loop instead of
/// blocking on the hotkey channel indefinitely.
#[cfg(target_os = "macos")]
pub fn pump_event_loop() {
    use objc2_foundation::{NSDate, NSRunLoop};
    let run_loop = NSRunLoop::mainRunLoop();
    let limit = NSDate::dateWithTimeIntervalSinceNow(0.02);
    // SAFETY: `NSDefaultRunLoopMode` is a framework constant that is always valid.
    let mode = unsafe { objc2_foundation::NSDefaultRunLoopMode };
    run_loop.runMode_beforeDate(mode, &limit);
}

#[cfg(not(target_os = "macos"))]
pub fn pump_event_loop() {}

/// Draw a filled, anti-aliased circle into a square RGBA buffer. The icon is
/// scaled to the standard 18 pt menu-bar height by tray-icon, so a 44 px
/// source stays crisp on Retina displays.
fn dot_icon(r: u8, g: u8, b: u8) -> Icon {
    const SIZE: u32 = 44;
    let radius = SIZE as f64 * 0.42;

    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f64 + 0.5 - SIZE as f64 / 2.0;
            let dy = y as f64 + 0.5 - SIZE as f64 / 2.0;
            let dist = (dx * dx + dy * dy).sqrt();

            let edge = dist - radius;
            let coverage = (0.5 - edge).clamp(0.0, 1.0);

            let i = ((y * SIZE + x) * 4) as usize;
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = (255.0 * coverage) as u8;
        }
    }

    Icon::from_rgba(rgba, SIZE, SIZE).expect("fixed-size RGBA icon is valid")
}
