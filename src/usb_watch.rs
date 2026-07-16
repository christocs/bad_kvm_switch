use anyhow::Result;
use nusb::hotplug::HotplugEvent;
use nusb::{DeviceId, MaybeFuture};
use std::collections::HashSet;

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

/// Block forever, printing a line every time the device with the given
/// `vendor_id:product_id` connects or disconnects, and calling `on_connect`
/// each time it connects (never on disconnect -- see module docs on the
/// dual-instance architecture: this machine only ever needs to react to
/// its own peripheral arriving, not leaving).
///
/// `watch_devices()` only gives a `Stream`, not a blocking iterator, so
/// `futures_lite::stream::block_on` drives it without needing a full async
/// runtime like tokio. Disconnect events carry only an opaque `DeviceId`
/// (not the vendor/product info), so a small set of "currently connected
/// IDs we care about" is tracked across the loop to recognize our target
/// device disconnecting.
pub fn watch(vendor_id: u16, product_id: u16, mut on_connect: impl FnMut()) -> Result<()> {
    let mut known_ids: HashSet<DeviceId> = HashSet::new();

    // Snapshot devices already connected before the watch starts, so a
    // disconnect of an already-plugged-in target device isn't missed. This
    // does NOT call `on_connect` -- if the peripheral was already here when
    // the service started, the monitor is presumably already showing us.
    for device in nusb::list_devices().wait()? {
        if device.vendor_id() == vendor_id && device.product_id() == product_id {
            println!("already connected: {vendor_id:04x}:{product_id:04x}");
            known_ids.insert(device.id());
        }
    }

    let watch = nusb::watch_devices()?;
    for event in futures_lite::stream::block_on(watch) {
        match event {
            HotplugEvent::Connected(info)
                if info.vendor_id() == vendor_id && info.product_id() == product_id =>
            {
                println!("connected: {vendor_id:04x}:{product_id:04x}");
                known_ids.insert(info.id());
                on_connect();
            }
            HotplugEvent::Disconnected(id) if known_ids.remove(&id) => {
                println!("disconnected: {vendor_id:04x}:{product_id:04x}");
            }
            _ => {}
        }
    }
    Ok(())
}
