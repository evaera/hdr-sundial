//! Win32 display-config FFI: enumerate HDR targets and get/set the SDR white
//! level (a.k.a. the "SDR content brightness" slider in Windows Settings).
//!
//! Reading the level is documented (`DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL`).
//! Setting it is not — Windows exposes an undocumented info type, value
//! `0xFFFFFFEE`, that takes a `{ header, u32 level, u8 finalValue }` packet.

use anyhow::{bail, Result};
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, DisplayConfigSetDeviceInfo, GetDisplayConfigBufferSizes,
    QueryDisplayConfig, DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL, DISPLAYCONFIG_DEVICE_INFO_HEADER,
    DISPLAYCONFIG_DEVICE_INFO_TYPE, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SDR_WHITE_LEVEL, QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{ERROR_SUCCESS, LUID};

/// Undocumented info type for setting the SDR white level.
const SET_SDR_WHITE_LEVEL: DISPLAYCONFIG_DEVICE_INFO_TYPE =
    DISPLAYCONFIG_DEVICE_INFO_TYPE(0xFFFF_FFEEu32 as i32);

/// Windows treats SDR white "1.0" as 80 nits; the level field is `nits * 1000 / 80`.
const NITS_PER_UNIT: f64 = 80.0 / 1000.0;

/// The packet for the undocumented set call. `#[repr(C)]` so the layout (and
/// thus `size_of`) matches the C struct Windows expects.
#[repr(C)]
struct SetSdrWhiteLevelPacket {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    sdr_white_level: u32,
    final_value: u8,
}

/// An active display target that currently has HDR / advanced color enabled.
#[derive(Clone, Copy)]
pub struct HdrTarget {
    adapter_id: LUID,
    id: u32,
}

impl HdrTarget {
    fn header(&self, kind: DISPLAYCONFIG_DEVICE_INFO_TYPE, size: usize) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
        DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: kind,
            size: size as u32,
            adapterId: self.adapter_id,
            id: self.id,
        }
    }

    /// Current SDR white level for this target, in nits.
    pub fn get_white_level_nits(&self) -> Result<f64> {
        let mut req = DISPLAYCONFIG_SDR_WHITE_LEVEL {
            header: self.header(
                DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
                std::mem::size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>(),
            ),
            ..Default::default()
        };
        let rc = unsafe { DisplayConfigGetDeviceInfo(&mut req.header) };
        if rc != ERROR_SUCCESS.0 as i32 {
            bail!("DisplayConfigGetDeviceInfo(GET_SDR_WHITE_LEVEL) failed: {rc}");
        }
        Ok(f64::from(req.SDRWhiteLevel) * NITS_PER_UNIT)
    }

    /// Set this target's SDR white level, in nits.
    pub fn set_white_level_nits(&self, nits: f64) -> Result<()> {
        let level = (nits / NITS_PER_UNIT).round().max(0.0) as u32;
        let mut packet = SetSdrWhiteLevelPacket {
            header: self.header(
                SET_SDR_WHITE_LEVEL,
                std::mem::size_of::<SetSdrWhiteLevelPacket>(),
            ),
            sdr_white_level: level,
            final_value: 1,
        };
        // Safety: the packet begins with the header; Windows reads the trailing
        // fields based on header.size.
        let rc = unsafe {
            DisplayConfigSetDeviceInfo(
                &mut packet.header as *mut DISPLAYCONFIG_DEVICE_INFO_HEADER as *const _,
            )
        };
        if rc != ERROR_SUCCESS.0 as i32 {
            bail!("DisplayConfigSetDeviceInfo(SET_SDR_WHITE_LEVEL) failed: {rc}");
        }
        Ok(())
    }
}

/// Whether advanced color (HDR) is currently enabled on a target.
fn advanced_color_enabled(adapter_id: LUID, id: u32) -> bool {
    let mut info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
            size: std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32,
            adapterId: adapter_id,
            id,
        },
        ..Default::default()
    };
    let rc = unsafe { DisplayConfigGetDeviceInfo(&mut info.header) };
    if rc != ERROR_SUCCESS.0 as i32 {
        return false;
    }
    // Bitfield: bit0 = advancedColorSupported, bit1 = advancedColorEnabled.
    (unsafe { info.Anonymous.value } >> 1) & 1 == 1
}

/// Enumerate all active display targets that currently have HDR enabled.
pub fn enumerate_hdr_targets() -> Result<Vec<HdrTarget>> {
    let flags = QDC_ONLY_ACTIVE_PATHS;
    let mut path_count = 0u32;
    let mut mode_count = 0u32;
    let rc = unsafe { GetDisplayConfigBufferSizes(flags, &mut path_count, &mut mode_count) };
    if rc != ERROR_SUCCESS {
        bail!("GetDisplayConfigBufferSizes failed: {}", rc.0);
    }

    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
    let rc = unsafe {
        QueryDisplayConfig(
            flags,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
    };
    if rc != ERROR_SUCCESS {
        bail!("QueryDisplayConfig failed: {}", rc.0);
    }
    paths.truncate(path_count as usize);

    let mut targets = Vec::new();
    for path in &paths {
        let adapter_id = path.targetInfo.adapterId;
        let id = path.targetInfo.id;
        if advanced_color_enabled(adapter_id, id) {
            targets.push(HdrTarget { adapter_id, id });
        }
    }
    Ok(targets)
}
