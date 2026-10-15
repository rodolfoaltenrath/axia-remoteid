//! Recovered RemoteID protocol, independent of the proprietary Qt interface.
//! Registration and wallet accepted in WSL; real signing and PKCS#11 remain unvalidated.
pub mod autostart;
pub mod bridge;
pub mod client;
pub mod gui;
pub mod identity;
pub mod installation;
pub mod protocol;
mod session_cache;
pub mod signing;
pub mod storage;
pub mod tray;
pub mod workflow;
