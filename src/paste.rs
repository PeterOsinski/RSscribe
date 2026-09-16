use anyhow::Result;
use enigo::{Enigo, Key, Keyboard, Settings};
use std::thread;
use std::time::Duration;

/// Put `text` on the clipboard and simulate a paste keystroke into
/// whatever window currently has focus.
///
/// Notes:
/// - macOS: your terminal/app needs Accessibility permission
///   (System Settings -> Privacy & Security -> Accessibility) for the
///   simulated keystroke to work, plus Microphone permission for cpal.
/// - Linux/Wayland: enigo's synthetic input support is limited on some
///   compositors; X11 works reliably. rdev's global hotkey capture has
///   the same caveat.
pub fn paste_text(text: &str) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new()?;
    let previous = clipboard.get_text().ok(); // best-effort restore afterwards
    clipboard.set_text(text.to_string())?;

    // Give the OS clipboard a moment to actually update before we
    // trigger the paste shortcut.
    thread::sleep(Duration::from_millis(80));

    let mut enigo = Enigo::new(&Settings::default())?;
    #[cfg(target_os = "macos")]
    let modifier = Key::Meta; // Cmd
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    enigo.key(modifier, enigo::Direction::Press)?;
    enigo.key(Key::Unicode('v'), enigo::Direction::Click)?;
    enigo.key(modifier, enigo::Direction::Release)?;

    // Optional: restore whatever was on the clipboard before we hijacked it.
    if let Some(prev) = previous {
        thread::sleep(Duration::from_millis(200));
        let _ = clipboard.set_text(prev);
    }

    Ok(())
}
