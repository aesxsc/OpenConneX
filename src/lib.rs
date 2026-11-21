pub mod device;
#[cfg(any(windows, target_os = "linux"))]
mod engine;
pub mod protocol;
#[cfg(any(windows, target_os = "linux"))]
mod storage;
#[cfg(any(windows, target_os = "linux"))]
mod ui;

#[cfg(any(windows, target_os = "linux"))]
#[cfg_attr(windows, path = "windows_ble.rs")]
#[cfg_attr(target_os = "linux", path = "linux_ble.rs")]
mod ble;

#[cfg(any(windows, target_os = "linux"))]
pub use engine::hardware_check;
#[cfg(any(windows, target_os = "linux"))]
pub use ui::run;
