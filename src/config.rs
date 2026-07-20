use crate::cli::{parse_hex_u16, parse_hex_u8};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// How to send the monitor-switch command.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SwitchMethod {
    /// Normal DDC/CI "Set VCP Feature" via `ddc-hi` -- works on most
    /// monitors, cross-platform (Windows via `ddc-winapi`, Linux via
    /// `ddc-i2c`).
    Standard,
    /// LG's DDC alt-mode side channel, for monitors that ignore the
    /// standard command (confirmed needed for the 39GX950B-B). On Windows
    /// the GPU vendor determines how the side channel is reached -- see
    /// [`AltModeBackend`]; on Linux it's always raw I2C (`linux_i2c.rs`).
    LgAltMode,
}

/// On Windows, the LG alt-mode side channel needs raw I2C to the monitor,
/// which is only reachable through the GPU vendor's SDK -- there's no
/// vendor-neutral path. This picks which one. Ignored on Linux (raw
/// `/dev/i2c-*` works regardless of GPU vendor).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AltModeBackend {
    /// AMD's ADL SDK (`adl.rs`). Needs `adl_adapter`/`adl_display` indices.
    #[default]
    Amd,
    /// NVIDIA's NvAPI (`nvapi.rs`). Needs no extra config -- it brute-forces
    /// the display mask/port.
    Nvidia,
}

/// The shape of `config.toml` as written by hand -- strings everywhere
/// because hex/VID:PID values are far more readable as text than as raw
/// TOML integers. [`Config::load`] parses and validates these into usable
/// types.
#[derive(Debug, Deserialize)]
struct RawConfig {
    usb_device_id: String,
    switch_method: SwitchMethod,
    /// Only consulted when `switch_method = "lg_alt_mode"` on Windows.
    #[serde(default)]
    alt_mode_backend: AltModeBackend,
    vcp_feature: String,
    input_value: String,
    adl_adapter: Option<i32>,
    adl_display: Option<i32>,
    #[serde(default = "default_log_level")]
    log_level: String,
}

fn default_log_level() -> String {
    "info".to_string()
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config file at {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("could not determine a config directory for this platform")]
    NoConfigDir,
    #[error("invalid usb_device_id '{0}': expected format 'vvvv:pppp' in hex, e.g. \"8968:4e4b\"")]
    InvalidUsbDeviceId(String),
    #[error("invalid vcp_feature '{0}': {1}")]
    InvalidVcpFeature(String, String),
    #[error("invalid input_value '{0}': {1}")]
    InvalidInputValue(String, String),
    #[error(
        "switch_method is \"lg_alt_mode\" but adl_adapter/adl_display are not set -- run \
         `bad_kvm_switch ddc-adl-probe` to find the right values"
    )]
    MissingAdlIndices,
    #[error("invalid log_level '{0}': {1} (expected one of trace, debug, info, warn, error)")]
    InvalidLogLevel(String, String),
    #[error(
        "input_value '{0}' doesn't fit in one byte, but switch_method \"lg_alt_mode\" sends a \
         single-byte value (e.g. hdmi1=0x90, dp=0xD0)"
    )]
    AltModeValueTooLarge(String),
}

#[derive(Debug)]
pub struct Config {
    pub usb_vendor_id: u16,
    pub usb_product_id: u16,
    pub switch_method: SwitchMethod,
    pub alt_mode_backend: AltModeBackend,
    pub vcp_feature: u8,
    pub input_value: u16,
    pub adl_adapter: i32,
    pub adl_display: i32,
    pub log_level: String,
}

impl Config {
    /// The single-byte value alt mode sends. Guaranteed to fit: `validate`
    /// rejects `lg_alt_mode` configs whose `input_value` exceeds one byte.
    pub fn alt_mode_input_value(&self) -> u8 {
        debug_assert!(self.switch_method == SwitchMethod::LgAltMode);
        self.input_value as u8
    }

    /// Load and validate config from `path`, or the platform default
    /// (`~/.config/bad_kvm_switch/config.toml` on Linux,
    /// `%APPDATA%\bad_kvm_switch\config\config.toml` on Windows) if `path`
    /// is `None`.
    pub fn load(path: Option<&Path>) -> Result<Self, ConfigError> {
        let path = match path {
            Some(p) => p.to_path_buf(),
            None => default_path()?,
        };
        let contents = std::fs::read_to_string(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?;
        let raw: RawConfig =
            toml::from_str(&contents).map_err(|source| ConfigError::Parse { path, source })?;
        Self::validate(raw)
    }

    fn validate(raw: RawConfig) -> Result<Self, ConfigError> {
        let (usb_vendor_id, usb_product_id) = parse_vid_pid(&raw.usb_device_id)?;
        let vcp_feature = parse_hex_u8(&raw.vcp_feature)
            .map_err(|e| ConfigError::InvalidVcpFeature(raw.vcp_feature.clone(), e))?;
        let input_value = parse_hex_u16(&raw.input_value)
            .map_err(|e| ConfigError::InvalidInputValue(raw.input_value.clone(), e))?;

        // Alt mode sends a single-byte value; catch an oversized one here
        // rather than silently truncating with `as u8` at switch time.
        if raw.switch_method == SwitchMethod::LgAltMode && input_value > u16::from(u8::MAX) {
            return Err(ConfigError::AltModeValueTooLarge(raw.input_value.clone()));
        }

        // The ADL adapter/display indices are specific to the AMD backend
        // on Windows -- the NVIDIA (NvAPI) backend brute-forces them, and
        // Linux's raw I2C path doesn't use them at all. So they're only
        // *required* for lg_alt_mode + AMD + Windows; everywhere else they
        // default to 0 and are ignored.
        let needs_adl_indices = raw.switch_method == SwitchMethod::LgAltMode
            && raw.alt_mode_backend == AltModeBackend::Amd
            && cfg!(target_os = "windows");
        let (adl_adapter, adl_display) = if needs_adl_indices {
            match (raw.adl_adapter, raw.adl_display) {
                (Some(a), Some(d)) => (a, d),
                _ => return Err(ConfigError::MissingAdlIndices),
            }
        } else {
            (raw.adl_adapter.unwrap_or(0), raw.adl_display.unwrap_or(0))
        };

        // Fail fast on a bad log_level here rather than at subscriber
        // init time in main(), so config errors are reported consistently.
        tracing_subscriber::EnvFilter::try_new(&raw.log_level)
            .map_err(|e| ConfigError::InvalidLogLevel(raw.log_level.clone(), e.to_string()))?;

        Ok(Config {
            usb_vendor_id,
            usb_product_id,
            switch_method: raw.switch_method,
            alt_mode_backend: raw.alt_mode_backend,
            vcp_feature,
            input_value,
            adl_adapter,
            adl_display,
            log_level: raw.log_level,
        })
    }
}

fn parse_vid_pid(s: &str) -> Result<(u16, u16), ConfigError> {
    let (vid, pid) = s
        .split_once(':')
        .ok_or_else(|| ConfigError::InvalidUsbDeviceId(s.to_string()))?;
    let vid = u16::from_str_radix(vid, 16).map_err(|_| ConfigError::InvalidUsbDeviceId(s.to_string()))?;
    let pid = u16::from_str_radix(pid, 16).map_err(|_| ConfigError::InvalidUsbDeviceId(s.to_string()))?;
    Ok((vid, pid))
}

fn default_path() -> Result<PathBuf, ConfigError> {
    let dirs = directories::ProjectDirs::from("", "", "bad_kvm_switch").ok_or(ConfigError::NoConfigDir)?;
    Ok(dirs.config_dir().join("config.toml"))
}
