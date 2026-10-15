//! Compiled desktop application; UI dependencies never enter the PKCS11 module.
pub use desktopid_client::{autostart, client, installation, storage};
pub mod bridge;
pub mod browsers;
pub mod desktop;
pub mod gui;
pub mod tray;
pub mod ui;
