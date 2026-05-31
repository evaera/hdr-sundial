//! Register/unregister HDR Sundial in the per-user startup entry.
//!
//! Writes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`

use anyhow::{Context, Result};
use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};
use winreg::RegKey;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
// Where Task Manager records each startup entry's enabled/disabled state. It
// outlives the Run value, so a row can linger in the Startup tab if not cleared.
const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const VALUE_NAME: &str = "HDR Sundial";

/// Register the currently-running exe to start at logon.
pub fn add() -> Result<()> {
    let exe = std::env::current_exe().context("locating current exe")?;
    let command = format!("\"{}\"", exe.display()); // quoted in case of spaces

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run, _) = hkcu.create_subkey(RUN_KEY).context("opening HKCU Run key")?;
    run.set_value(VALUE_NAME, &command)
        .context("writing startup value")?;

    println!("Added startup entry '{VALUE_NAME}' -> {command}");
    println!("Shows in Task Manager > Startup apps; runs at next logon.");
    Ok(())
}

/// Remove the startup entry, if present.
pub fn remove() -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = hkcu
        .open_subkey_with_flags(RUN_KEY, KEY_WRITE)
        .context("opening HKCU Run key")?;

    match run.delete_value(VALUE_NAME) {
        Ok(()) => println!("Removed startup entry '{VALUE_NAME}'."),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("No startup entry '{VALUE_NAME}' found; nothing to remove.");
        }
        Err(e) => return Err(anyhow::Error::new(e).context("deleting startup value")),
    }

    // Also clear Task Manager's saved enable/disable state, if any — otherwise
    // the row keeps showing in the Startup tab. Best effort: absent is fine.
    if let Ok(approved) = hkcu.open_subkey_with_flags(APPROVED_KEY, KEY_WRITE) {
        let _ = approved.delete_value(VALUE_NAME);
    }
    Ok(())
}
