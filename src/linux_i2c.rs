//! Raw Linux I2C access for LG's "DDC alt mode" side channel.
//!
//! `ddc-i2c`'s normal DDC/CI "Set VCP Feature" always frames its writes
//! with the standard DDC/CI source address (0x51); this monitor's alt-mode
//! side channel only responds when the source address is the non-standard
//! 0x50 instead, which the normal API has no way to request -- see
//! `adl.rs`'s module docs for the full protocol background (confirmed
//! working there; this is the Linux equivalent using `i2c_transfer`'s raw
//! block write instead of AMD's ADL SDK).
//!
//! Reuses `ddc-i2c`'s own device discovery (`I2cDeviceEnumerator`, the
//! same mechanism `ddc-hi::Display::enumerate()` uses internally) rather
//! than re-implementing bus discovery, then reaches through to the
//! underlying `i2c-linux` device for a raw block write.

use anyhow::{Context, Result};
use i2c_linux::{Message, WriteFlags};

/// DDC/CI's I2C slave address, 7-bit (not shifted -- `i2c_transfer`'s
/// `Message::Write::address` takes the plain 7-bit address, unlike ADL on
/// Windows which wants it pre-shifted).
const DDC_I2C_ADDRESS: u16 = 0x37;

/// Non-standard DDC/CI source address LG's alt-mode firmware listens on
/// (standard DDC/CI always uses 0x51).
const ALT_MODE_SOURCE_ADDR: u8 = 0x50;
/// DDC "length" byte: 0x80 | 4 data bytes (opcode + feature + hi + lo).
const ALT_MODE_LENGTH_BYTE: u8 = 0x84;
/// DDC "Set VCP Feature" opcode.
const SET_VCP_FEATURE_OPCODE: u8 = 0x03;

/// Write `value` to `feature` via LG's DDC alt-mode side channel, on the
/// first DDC/CI-capable I2C device found.
///
/// Requires read/write access to the relevant `/dev/i2c-*` device -- if
/// this fails with a permissions error, add yourself to the `i2c` group
/// (`sudo usermod -aG i2c $USER`, then log out/in) or add a udev rule,
/// same prerequisite as `ddcutil`.
pub fn set_vcp_alt_mode(feature: u8, value: u8) -> Result<()> {
    let mut devices = ddc_i2c::I2cDeviceEnumerator::new().context(
        "failed to enumerate I2C devices (are you in the `i2c` group / is a udev rule set up?)",
    )?;
    let mut device = devices
        .next()
        .context("no DDC/CI-capable I2C device found")?;

    // Standard-shaped DDC "Set VCP Feature" frame, just with the alt-mode
    // source address (0x50) substituted for the usual 0x51. The checksum is
    // computed over the destination write address (0x6E, DDC/CI's 0x37
    // shifted left one bit) even though that byte isn't itself part of the
    // payload here -- i2c_transfer's `address` field handles addressing,
    // matching the ddcutil-documented protocol exactly:
    // https://github.com/rockowitz/ddcutil/wiki/Switching-input-source-on-LG-monitors
    let hi = 0u8;
    let checksum = [
        0x6eu8,
        ALT_MODE_SOURCE_ADDR,
        ALT_MODE_LENGTH_BYTE,
        SET_VCP_FEATURE_OPCODE,
        feature,
        hi,
        value,
    ]
    .into_iter()
    .fold(0u8, |acc, b| acc ^ b);
    let frame = [
        ALT_MODE_SOURCE_ADDR,
        ALT_MODE_LENGTH_BYTE,
        SET_VCP_FEATURE_OPCODE,
        feature,
        hi,
        value,
        checksum,
    ];

    device
        .inner_mut()
        .i2c_transfer(&mut [Message::Write {
            address: DDC_I2C_ADDRESS,
            data: &frame,
            flags: WriteFlags::empty(),
        }])
        .context("I2C write failed")
}
