//! LG "DDC alt mode" side channel on Windows via NVIDIA's NvAPI raw I2C
//! (`NvAPI_I2CWrite`), for the NVIDIA equivalent of `adl.rs`'s AMD path.
//!
//! Same reason as `adl.rs`: the monitor's alt-mode channel needs DDC/CI
//! source address 0x50, which the Windows Monitor Configuration API can't
//! request. AMD reaches it via ADL; NVIDIA reaches it via NvAPI's per-GPU
//! raw I2C. The wire frame is byte-identical to the AMD one (which this
//! project confirmed live on an LG 39GX950B-B) -- the NvAPI call sequence
//! is modelled on `meer-cha/lg-input-switch` (Python/ctypes), which drives
//! the closely-related LG 45GX950A-B through `NvAPI_I2CWrite`. This NVIDIA
//! path itself is unverified -- no NVIDIA hardware on hand to test it.
//!
//! Uses the `nvapi` crate's *safe* `PhysicalGpu::i2c_write` wrapper rather
//! than hand-rolling the `NV_I2C_INFO_V3` struct (the wrapper gets the
//! version field and the two-uint8-then-padding layout right, which the
//! reference had to model by hand). NOTE: the wrapper takes the **7-bit**
//! I2C address and shifts it left itself, so we pass `0x37`, not the
//! pre-shifted `0x6E` -- but the DDC checksum is still seeded with the
//! 8-bit `0x6E`, same as on the wire.

use anyhow::{Context, Result, anyhow, bail};
use nvapi::PhysicalGpu;
use nvapi::sys::i2c::I2cSpeed;

/// DDC/CI destination, 7-bit -- `i2c_write` shifts this left by one to the
/// 8-bit `0x6E` on the wire.
const DDC_DEST_7BIT: u8 = 0x37;
/// The same destination as the 8-bit value it becomes on the wire, used
/// only to seed the DDC checksum (which is computed over the 8-bit form).
const DDC_DEST_8BIT: u8 = 0x6E;
const ALT_MODE_SOURCE_ADDR: u8 = 0x50;
const ALT_MODE_LENGTH_BYTE: u8 = 0x84;
const SET_VCP_FEATURE_OPCODE: u8 = 0x03;

/// Build the 7-byte DDC "Set VCP Feature" payload for the alt-mode channel.
/// Unlike `adl.rs`'s frame, this does NOT include the leading destination
/// byte -- NvAPI carries that separately in `i2cDevAddress`. The checksum
/// is still seeded with the 8-bit destination (`0x6E`), so the value
/// matches the AMD path byte-for-byte.
fn alt_mode_payload(feature: u8, value: u8) -> [u8; 7] {
    let hi = 0u8;
    let body = [
        ALT_MODE_SOURCE_ADDR,
        ALT_MODE_LENGTH_BYTE,
        SET_VCP_FEATURE_OPCODE,
        feature,
        hi,
        value,
    ];
    let checksum = body.iter().fold(DDC_DEST_8BIT, |acc, &b| acc ^ b);
    [
        body[0], body[1], body[2], body[3], body[4], body[5], checksum,
    ]
}

/// Initialize NvAPI and grab the first NVIDIA GPU. Succeeding here IS the
/// "is this an NVIDIA system" check -- NvAPI only initializes/enumerates on
/// NVIDIA hardware, so no separate vendor-id comparison is needed (contrast
/// `adl.rs`, which must check the AMD vendor id explicitly).
fn first_gpu() -> Result<PhysicalGpu> {
    nvapi::initialize()
        .map_err(|e| anyhow!("NvAPI initialize failed (NVIDIA drivers installed?): {e:?}"))?;
    let gpus = PhysicalGpu::enumerate()
        .map_err(|e| anyhow!("failed to enumerate NVIDIA GPUs: {e:?}"))?;
    gpus.into_iter()
        .next()
        .context("no NVIDIA GPU detected")
}

/// Write `value` to `feature` via LG's DDC alt-mode side channel, on the
/// first NVIDIA GPU.
///
/// Brute-forces every plausible display mask x port, exactly as the
/// reference tool does: NvAPI needs the display's legacy output mask/port,
/// and there's no reliable way to know which one the monitor is on without
/// the (unwrapped) connected-outputs query. A write to a mask/port the
/// monitor isn't on simply fails silently and has no visible effect -- only
/// the correct target actually switches -- so spraying all of them is safe.
/// We don't stop on the first success: a wrapped write can report OK for an
/// output the monitor isn't actually behind, so short-circuiting could stop
/// before reaching the real one.
pub fn set_vcp_alt_mode(feature: u8, value: u8) -> Result<()> {
    let gpu = first_gpu()?;
    let payload = alt_mode_payload(feature, value);

    let mut any_ok = false;
    for mask in (0..8u32).map(|i| 1u32 << i) {
        // port None => rely on the display mask; Some(1..=7) => target a
        // specific port id. Mirrors the reference's combination set.
        for port in std::iter::once(None).chain((1..=7u8).map(Some)) {
            if gpu
                .i2c_write(mask, port, true, DDC_DEST_7BIT, &[], &payload, I2cSpeed::Default)
                .is_ok()
            {
                any_ok = true;
            }
        }
    }

    if any_ok {
        Ok(())
    } else {
        bail!("NvAPI I2C write did not succeed on any display mask/port")
    }
}

/// List the NVIDIA GPU(s) NvAPI sees and how many displays each reports --
/// a smoke test that the NvAPI path can reach your hardware, analogous to
/// `adl::probe_displays`. (NVIDIA doesn't need adapter/display indices in
/// config -- `set_vcp_alt_mode` brute-forces them -- so this is purely
/// diagnostic.)
pub fn probe() -> Result<()> {
    nvapi::initialize()
        .map_err(|e| anyhow!("NvAPI initialize failed (NVIDIA drivers installed?): {e:?}"))?;
    let gpus = PhysicalGpu::enumerate()
        .map_err(|e| anyhow!("failed to enumerate NVIDIA GPUs: {e:?}"))?;
    if gpus.is_empty() {
        println!("no NVIDIA GPU detected");
        return Ok(());
    }
    for gpu in &gpus {
        let name = gpu.full_name().unwrap_or_else(|_| "<unknown>".to_string());
        match gpu.display_ids_all() {
            Ok(ids) => println!("NVIDIA GPU: {name} ({} display(s) attached)", ids.len()),
            Err(e) => println!("NVIDIA GPU: {name} (couldn't read displays: {e:?})"),
        }
    }
    Ok(())
}
