fn main() {
    // GTK/GnuTLS hosts can already export C_GetFunctionList through p11-kit.
    // Keep function pointers in our PKCS11 table bound to our own definitions.
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "linux") {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-Bsymbolic-functions");
    }
}
