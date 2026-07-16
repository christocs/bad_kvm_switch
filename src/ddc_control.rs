use anyhow::{Context, Result};
use ddc::Ddc;
use ddc_hi::Display;

/// Grab the first DDC/CI-capable monitor the OS reports.
///
/// v1 doesn't support picking a specific monitor by name/serial when more
/// than one is attached -- if that's ever needed, `Display::enumerate()`
/// already gives each `Display`'s `.info` to filter on.
fn first_display() -> Result<Display> {
    Display::enumerate()
        .into_iter()
        .next()
        .context("no DDC/CI-capable monitor detected")
}

/// Read a VCP feature's current value from the first detected monitor.
pub fn get_vcp(feature: u8) -> Result<u16> {
    let mut display = first_display()?;
    let value = display
        .handle
        .get_vcp_feature(feature)
        .with_context(|| format!("failed to read VCP feature 0x{feature:02x}"))?;
    Ok(value.value())
}

/// Write a value to a VCP feature on the first detected monitor.
///
/// This is a plain passthrough to the DDC "Set VCP Feature" command for
/// whatever feature code is given -- including non-standard/vendor codes
/// like LG's 0xF4 "alt mode" input select, which uses this exact wire
/// format, just with a feature code outside the standard MCCS table.
pub fn set_vcp(feature: u8, value: u16) -> Result<()> {
    let mut display = first_display()?;
    display
        .handle
        .set_vcp_feature(feature, value)
        .with_context(|| format!("failed to set VCP feature 0x{feature:02x} to 0x{value:04x}"))
}

/// Like [`set_vcp`], but reads the feature's current value first and skips
/// the write if it's already `value` -- avoids an unnecessary (and
/// sometimes visually disruptive) input switch when the monitor's already
/// showing the right source.
///
/// Only meaningful for standard, properly-bidirectional VCP features:
/// confirmed via `0x60` (Input Select) correctly reporting the active
/// input. Vendor side channels like LG's `0xF4` alt mode don't reliably
/// report back current state at all (confirmed empirically -- `0x60`
/// stayed stale after a switch driven through `0xF4`), so this isn't
/// offered for that path; see `main.rs`'s `switch_to_my_input`.
///
/// If the read itself fails (e.g. a transient DDC error), this falls
/// through to writing anyway rather than silently skipping a switch that
/// might actually be needed.
pub fn set_vcp_if_needed(feature: u8, value: u16) -> Result<()> {
    match get_vcp(feature) {
        Ok(current) if current == value => {
            tracing::info!(
                "VCP feature 0x{feature:02x} already at 0x{value:04x}, skipping switch"
            );
            Ok(())
        }
        _ => set_vcp(feature, value),
    }
}
