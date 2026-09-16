use anyhow::Result;
use arboard::Clipboard;
use enigo::{Enigo, Key, Keyboard, Settings};
use std::thread;
use std::time::Duration;

/// Holds the clipboard for the duration of a dictation session. The original
/// clipboard contents are captured on creation and restored when dropped, so
/// many phrases can be pasted without hijacking/restoring the clipboard each
/// time.
pub struct Paster {
    previous: Option<String>,
    clipboard: Clipboard,
    pasted: bool,
}

impl Paster {
    pub fn new() -> Result<Self> {
        let mut clipboard = Clipboard::new()?;
        let previous = clipboard.get_text().ok();
        Ok(Paster {
            previous,
            clipboard,
            pasted: false,
        })
    }

    /// Put `text` on the clipboard and simulate a paste keystroke into
    /// whatever window currently has focus. Consecutive pastes in the same
    /// session are separated by a space so fragments don't run together.
    ///
    /// Notes:
    /// - macOS: your terminal/app needs Accessibility permission
    ///   (System Settings -> Privacy & Security -> Accessibility) for the
    ///   simulated keystroke to work, plus Microphone permission for cpal.
    pub fn paste(&mut self, text: &str) -> Result<()> {
        let text = if self.pasted {
            format!(" {text}")
        } else {
            text.to_string()
        };

        self.clipboard.set_text(text)?;

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

        self.pasted = true;
        Ok(())
    }
}

impl Drop for Paster {
    fn drop(&mut self) {
        // Best-effort restore of whatever was on the clipboard before we
        // hijacked it.
        if let Some(prev) = &self.previous {
            thread::sleep(Duration::from_millis(200));
            let _ = self.clipboard.set_text(prev.clone());
        }
    }
}

/// One-shot convenience: paste a single piece of text and restore the previous
/// clipboard contents afterwards.
pub fn paste_text(text: &str) -> Result<()> {
    let mut paster = Paster::new()?;
    paster.paste(text)
}
