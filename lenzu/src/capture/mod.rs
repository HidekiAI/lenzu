pub mod capture_traits;
#[cfg(feature = "gtk")]
pub mod capture_wayland;
#[cfg(all(target_os = "windows", feature = "gtk"))]
pub mod capture_winapi;
pub mod capture_x11;
