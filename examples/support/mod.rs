#[cfg(not(any(
    all(target_os = "linux", feature = "linuxfb", feature = "evdev"),
    all(target_os = "linux", feature = "drm", feature = "evdev")
)))]
pub mod winit_backend;

#[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
pub mod linux_backend;

#[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
pub mod drm_backend;
