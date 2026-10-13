//! Recovered RemoteID protocol, independent of the proprietary Qt interface.
//! Registration and wallet accepted in WSL; real signing and PKCS#11 remain unvalidated.
pub mod bridge;
pub mod client;
pub mod identity;
pub mod protocol;
pub mod signing;
pub mod storage;
pub mod workflow;
