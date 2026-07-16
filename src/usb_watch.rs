use anyhow::Result;
use nusb::MaybeFuture;

/// Print every currently-attached USB device as one tab-separated line:
/// `vendor:product<TAB>manufacturer<TAB>product<TAB>serial`.
///
/// Used to find the VID:PID of the peripheral to watch for. Note:
/// `manufacturer_string()` is always `None` on Windows (nusb doesn't cache
/// it there), so that column will be blank on Windows builds — that's
/// expected, not a bug.
pub fn print_device_list() -> Result<()> {
    let devices = nusb::list_devices().wait()?;
    for device in devices {
        println!(
            "{:04x}:{:04x}\t{}\t{}\t{}",
            device.vendor_id(),
            device.product_id(),
            device.manufacturer_string().unwrap_or(""),
            device.product_string().unwrap_or(""),
            device.serial_number().unwrap_or(""),
        );
    }
    Ok(())
}
