//! Native Linux PKCS11 v2.40 provider. Cloud operations run in an external helper.
//! FFI safety contract: callers provide valid, aligned buffers of the advertised
//! sizes and do not mutate the returned static function table. Null/length checks
//! cannot validate arbitrary addresses supplied by C. No panic crosses the ABI.
#![allow(non_snake_case, clippy::missing_safety_doc)]
mod api;
mod core;
mod functions;
mod helper;
pub use api::*;
pub use functions::*;

macro_rules! export {
    ($name:ident($($arg:ident : $ty:ty),*) $body:block) => {
        #[unsafe(no_mangle)]
        #[allow(unused_unsafe)]
        pub unsafe extern "C" fn $name($($arg: $ty),*) -> cryptoki_sys::CK_RV {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { $body }))
                .unwrap_or(cryptoki_sys::CKR_GENERAL_ERROR)
        }
    };
}
pub(crate) use export;
#[cfg(test)]
mod tests;
