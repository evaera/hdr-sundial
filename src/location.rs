//! Query the current location from the Windows Geolocation (WinRT) API.

use anyhow::{anyhow, Context, Result};

/// Best-effort current position as `(latitude, longitude)` in degrees.
///
/// Blocks until Windows returns a fix, so call this off the UI thread. Returns
/// a user-facing error when location is turned off or no fix is available.
pub fn current_latlon() -> Result<(f64, f64)> {
    use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator};

    // WinRT calls need a COM apartment on this thread; MTA lets `get()` block
    // without a message pump.
    init_apartment();

    // Surfaces the OS location prompt / honors the privacy toggle.
    if let Ok(status) = Geolocator::RequestAccessAsync().and_then(|op| op.get()) {
        if status == GeolocationAccessStatus::Denied {
            return Err(anyhow!(
                "Location is off. Turn it on in Settings \u{2192} Privacy & security \u{2192} Location."
            ));
        }
    }

    let locator = Geolocator::new().context("creating Geolocator")?;
    let pos = locator
        .GetGeopositionAsync()
        .context("requesting position")?
        .get()
        .map_err(|_| anyhow!("Couldn't get a location fix. Is Location enabled?"))?;
    let p = pos.Coordinate()?.Point()?.Position()?;
    Ok((p.Latitude, p.Longitude))
}

fn init_apartment() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    // Harmless if the thread is already initialized; the result is ignored.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}
