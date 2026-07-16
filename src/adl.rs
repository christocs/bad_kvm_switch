//! Raw AMD ADL (legacy Display Library) access, used to send the LG "DDC
//! alt mode" side-channel command via `ADL_Display_DDCBlockAccess_Get`.
//!
//! This supersedes an earlier attempt through the newer ADLX SDK's
//! `IADLXI2C` (see the `adlx`/vendored patch still in this repo) -- ADLX's
//! interface is scoped per-GPU and never reached the monitor's DDC pins
//! (every `ADLX_I2C_LINE` reported "unsupported" for the DDC address).
//! `ADL_Display_DDCBlockAccess_Get` is scoped per adapter *and display*,
//! and is confirmed working on this exact GPU+monitor combination by two
//! independent open-source projects: amildahl/amdddc-windows and
//! phillip9933/LGInputSwitch (MIT). Struct layouts below are transcribed
//! from AMD's public, MIT-licensed `adl_sdk.h`/`adl_structures.h`/
//! `adl_defines.h` (GPUOpen-LibrariesAndSDKs/display-library).

use anyhow::{bail, Context, Result};
use libloading::Library;
use std::ffi::{c_int, c_void};

const ADL_MAX_PATH: usize = 256;
const ADL_OK: i32 = 0;
const ADL_DISPLAY_DISPLAYINFO_DISPLAYCONNECTED: i32 = 0x0000_0001;
const ADL_DISPLAY_DISPLAYINFO_DISPLAYMAPPED: i32 = 0x0000_0002;
/// AMD/ATI's official PCI-SIG vendor ID is the hex value 0x1002, but ADL's
/// `AdapterInfo::i_vendor_id` reports it as the plain decimal number 1002
/// (confirmed empirically -- it parses the digits out of the vendor ID
/// rather than reporting the raw hex value).
const AMD_VENDOR_ID: i32 = 1002;

#[repr(C)]
struct AdapterInfo {
    i_size: i32,
    i_adapter_index: i32,
    str_udid: [i8; ADL_MAX_PATH],
    i_bus_number: i32,
    i_device_number: i32,
    i_function_number: i32,
    i_vendor_id: i32,
    str_adapter_name: [i8; ADL_MAX_PATH],
    str_display_name: [i8; ADL_MAX_PATH],
    i_present: i32,
    i_exist: i32,
    str_driver_path: [i8; ADL_MAX_PATH],
    str_driver_path_ext: [i8; ADL_MAX_PATH],
    str_pnp_string: [i8; ADL_MAX_PATH],
    i_os_display_index: i32,
}

impl Default for AdapterInfo {
    fn default() -> Self {
        // SAFETY: an all-zero AdapterInfo is a valid bit pattern (plain ints + char arrays).
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AdlDisplayId {
    i_display_logical_index: i32,
    i_display_physical_index: i32,
    i_display_logical_adapter_index: i32,
    i_display_physical_adapter_index: i32,
}

#[repr(C)]
struct AdlDisplayInfo {
    display_id: AdlDisplayId,
    i_display_controller_index: i32,
    str_display_name: [i8; ADL_MAX_PATH],
    str_display_manufacturer_name: [i8; ADL_MAX_PATH],
    i_display_type: i32,
    i_display_output_type: i32,
    i_display_connector: i32,
    i_display_info_mask: i32,
    i_display_info_value: i32,
}

type AdlMainMallocCallback = unsafe extern "C" fn(c_int) -> *mut c_void;
type AdlMainControlCreateFn = unsafe extern "C" fn(AdlMainMallocCallback, c_int) -> c_int;
type AdlMainControlDestroyFn = unsafe extern "C" fn() -> c_int;
type AdlAdapterNumberOfAdaptersGetFn = unsafe extern "C" fn(*mut c_int) -> c_int;
type AdlAdapterAdapterInfoGetFn = unsafe extern "C" fn(*mut AdapterInfo, c_int) -> c_int;
type AdlDisplayDisplayInfoGetFn =
    unsafe extern "C" fn(c_int, *mut c_int, *mut *mut AdlDisplayInfo, c_int) -> c_int;
type AdlDisplayDdcBlockAccessGetFn = unsafe extern "C" fn(
    c_int,
    c_int,
    c_int,
    c_int,
    c_int,
    *mut u8,
    *mut c_int,
    *mut u8,
) -> c_int;

unsafe extern "C" fn adl_malloc(size: c_int) -> *mut c_void {
    unsafe { libc::malloc(size as usize) }
}

struct Adl {
    _lib: Library,
    control_destroy: AdlMainControlDestroyFn,
    adapter_count: AdlAdapterNumberOfAdaptersGetFn,
    adapter_info: AdlAdapterAdapterInfoGetFn,
    display_info: AdlDisplayDisplayInfoGetFn,
    ddc_block_access: AdlDisplayDdcBlockAccessGetFn,
}

impl Adl {
    fn load() -> Result<Self> {
        let lib = unsafe { Library::new("atiadlxx.dll") }
            .or_else(|_| unsafe { Library::new("atiadlxy.dll") })
            .context("failed to load atiadlxx.dll / atiadlxy.dll (AMD driver not installed?)")?;

        // `T` must be the actual function-pointer type: `Symbol<T>: Deref<Target = T>`
        // dereferences straight to a callable `T`. (Instantiating with a data
        // pointer type like `*const ()` and casting/dereferencing through
        // that reads the wrong bytes as a function address -- this is what
        // caused the access violation on the first ADL call below.)
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {
                *unsafe { lib.get::<$ty>($name) }
                    .with_context(|| format!("missing symbol {:?}", $name))?
            };
        }

        let control_create: AdlMainControlCreateFn =
            symbol!(b"ADL_Main_Control_Create\0", AdlMainControlCreateFn);
        let control_destroy: AdlMainControlDestroyFn =
            symbol!(b"ADL_Main_Control_Destroy\0", AdlMainControlDestroyFn);
        let adapter_count: AdlAdapterNumberOfAdaptersGetFn = symbol!(
            b"ADL_Adapter_NumberOfAdapters_Get\0",
            AdlAdapterNumberOfAdaptersGetFn
        );
        let adapter_info: AdlAdapterAdapterInfoGetFn = symbol!(
            b"ADL_Adapter_AdapterInfo_Get\0",
            AdlAdapterAdapterInfoGetFn
        );
        let display_info: AdlDisplayDisplayInfoGetFn = symbol!(
            b"ADL_Display_DisplayInfo_Get\0",
            AdlDisplayDisplayInfoGetFn
        );
        let ddc_block_access: AdlDisplayDdcBlockAccessGetFn = symbol!(
            b"ADL_Display_DDCBlockAccess_Get\0",
            AdlDisplayDdcBlockAccessGetFn
        );

        // Second arg 1 == only enumerate currently active adapters.
        let result = unsafe { control_create(adl_malloc, 1) };
        if result != ADL_OK {
            bail!("ADL_Main_Control_Create failed with code {result}");
        }

        let adl = Self {
            _lib: lib,
            control_destroy,
            adapter_count,
            adapter_info,
            display_info,
            ddc_block_access,
        };

        // Every public entry point (`probe_displays`, `set_vcp_alt_mode`) goes
        // through `load()`, so gating here keeps non-AMD systems from ever
        // reaching an ADL call.
        let adapters = adl.adapters()?;
        let has_amd = adapters
            .iter()
            .any(|a| a.i_present != 0 && a.i_vendor_id == AMD_VENDOR_ID);
        if !has_amd {
            bail!(
                "No active AMD GPU detected ({} adapter(s) found, none AMD) -- the LG DDC \
                 alt-mode workaround requires an AMD GPU, since it uses AMD's ADL SDK",
                adapters.len()
            );
        }

        Ok(adl)
    }

    fn adapters(&self) -> Result<Vec<AdapterInfo>> {
        let mut count: c_int = 0;
        let result = unsafe { (self.adapter_count)(&mut count) };
        if result != ADL_OK {
            bail!("ADL_Adapter_NumberOfAdapters_Get failed with code {result}");
        }

        let mut adapters: Vec<AdapterInfo> = (0..count).map(|_| AdapterInfo::default()).collect();
        let buffer_size = (count as usize * std::mem::size_of::<AdapterInfo>()) as c_int;
        let result = unsafe { (self.adapter_info)(adapters.as_mut_ptr(), buffer_size) };
        if result != ADL_OK {
            bail!("ADL_Adapter_AdapterInfo_Get failed with code {result}");
        }
        Ok(adapters)
    }

    /// Connected displays for one adapter, as `(logical_display_index, name)`.
    fn connected_displays(&self, adapter_index: i32) -> Result<Vec<(i32, String)>> {
        let mut count: c_int = 0;
        let mut infos: *mut AdlDisplayInfo = std::ptr::null_mut();
        let result = unsafe { (self.display_info)(adapter_index, &mut count, &mut infos, 0) };
        if result != ADL_OK || infos.is_null() {
            // Common for inactive/secondary adapter indices -- not fatal.
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for i in 0..count {
            let info = unsafe { &*infos.offset(i as isize) };
            if info.i_display_info_value & ADL_DISPLAY_DISPLAYINFO_DISPLAYCONNECTED != 0 {
                let name = unsafe { std::ffi::CStr::from_ptr(info.str_display_name.as_ptr()) }
                    .to_string_lossy()
                    .into_owned();
                let mapped = info.i_display_info_value & ADL_DISPLAY_DISPLAYINFO_DISPLAYMAPPED != 0;
                out.push((
                    info.display_id.i_display_logical_index,
                    format!("{name} (mapped={mapped})"),
                ));
            }
        }
        unsafe { libc::free(infos.cast()) };
        Ok(out)
    }

    fn ddc_block_write(&self, adapter_index: i32, display_index: i32, data: &mut [u8]) -> Result<()> {
        let mut recv_len: c_int = 0;
        let result = unsafe {
            (self.ddc_block_access)(
                adapter_index,
                display_index,
                0,
                0,
                data.len() as c_int,
                data.as_mut_ptr(),
                &mut recv_len,
                std::ptr::null_mut(),
            )
        };
        if result != ADL_OK {
            bail!("ADL_Display_DDCBlockAccess_Get failed with code {result}");
        }
        Ok(())
    }
}

impl Drop for Adl {
    fn drop(&mut self) {
        unsafe { (self.control_destroy)() };
    }
}

/// Print every connected display as `adapter=<N> display=<N> <name>`, to
/// find the right `--adapter`/`--display` values for [`set_vcp_alt_mode`].
pub fn probe_displays() -> Result<()> {
    let adl = Adl::load()?;
    for adapter in adl.adapters()? {
        if adapter.i_present == 0 {
            continue;
        }
        for (display_index, name) in adl.connected_displays(adapter.i_adapter_index)? {
            println!(
                "adapter={} display={} {name}",
                adapter.i_adapter_index, display_index
            );
        }
    }
    Ok(())
}

/// Write `value` to `feature` via LG's DDC alt-mode side channel, on the
/// given adapter/display, using `ADL_Display_DDCBlockAccess_Get`.
pub fn set_vcp_alt_mode(feature: u8, value: u8, adapter_index: i32, display_index: i32) -> Result<()> {
    let adl = Adl::load()?;

    // DDC/CI-shaped frame per https://github.com/rockowitz/ddcutil/wiki/Switching-input-source-on-LG-monitors:
    // [dest(0x37<<1), alt-mode source(0x50), length, opcode=SetVCPFeature, feature, hi, lo, checksum]
    const DEST: u8 = 0x37 << 1;
    const ALT_SOURCE: u8 = 0x50;
    const LENGTH: u8 = 0x84;
    const SET_VCP_FEATURE: u8 = 0x03;
    let hi = 0u8;
    let mut frame = [DEST, ALT_SOURCE, LENGTH, SET_VCP_FEATURE, feature, hi, value, 0u8];
    let checksum = frame[..7].iter().fold(0u8, |acc, b| acc ^ b);
    frame[7] = checksum;

    adl.ddc_block_write(adapter_index, display_index, &mut frame)
}
