use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::CStr,
    fs,
    io::Read,
    mem::{MaybeUninit, size_of},
    path::{Path, PathBuf},
    process::Command,
};

pub fn os_release() -> Result<BTreeMap<String, String>> {
    let text = match fs::read_to_string("/etc/os-release") {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error.into()),
    };
    Ok(text
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let (name, value) = line.split_once('=')?;
            Some((name.into(), value.trim_matches(['\"', '\'']).into()))
        })
        .collect())
}

fn available(name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| {
            fs::metadata(directory.join(name))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

pub fn collect(module: Option<&Path>) -> Result<Value> {
    let mut uname = MaybeUninit::<libc::utsname>::uninit();
    // SAFETY: uname fills the supplied valid utsname storage on success.
    ensure!(
        unsafe { libc::uname(uname.as_mut_ptr()) } == 0,
        "Não foi possível consultar o kernel"
    );
    // SAFETY: the successful uname call initialized all fields; strings are NUL terminated.
    let uname = unsafe { uname.assume_init() };
    let system = unsafe { CStr::from_ptr(uname.sysname.as_ptr()) }.to_string_lossy();
    let release = unsafe { CStr::from_ptr(uname.release.as_ptr()) }.to_string_lossy();
    let kernel = release.to_ascii_lowercase();
    let is_wsl = system == "Linux"
        && (kernel.contains("microsoft")
            || kernel.contains("wsl")
            || std::env::var_os("WSL_INTEROP").is_some());
    let runtime = if is_wsl {
        "wsl".into()
    } else {
        system.to_ascii_lowercase()
    };
    let commands = [
        "firefox",
        "chromium",
        "chromium-browser",
        "google-chrome",
        "brave-browser",
        "p11-kit",
        "modutil",
        "certutil",
        "pkcs11-tool",
        "openssl",
        "readelf",
        "gcc",
        "make",
        "pkg-config",
        "flatpak",
        "rpmbuild",
        "podman",
    ];
    let commands: BTreeMap<_, _> = commands
        .iter()
        .map(|name| (*name, available(name)))
        .collect();
    let mut result = json!({
        "system": system, "architecture": std::env::consts::ARCH,
        "pointer_bits": size_of::<*const ()>() * 8, "unsigned_long_bits": libc::c_ulong::BITS,
        "session_type": std::env::var("XDG_SESSION_TYPE").ok(), "desktop": std::env::var("XDG_CURRENT_DESKTOP").ok(),
        "tool_version": env!("CARGO_PKG_VERSION"), "kernel_release": release,
        "is_wsl": is_wsl, "runtime": runtime, "os_release": os_release()?, "commands_available": commands,
        "note": "Este inventário não testa cadastro, 2FA ou assinatura. DLL e dylib extraídas não são módulos ELF para o Fedora."
    });
    if available("rpm") {
        let mut packages = BTreeMap::new();
        for name in [
            "firefox",
            "chromium",
            "p11-kit",
            "p11-kit-devel",
            "nss-tools",
            "opensc",
            "openssl",
            "openssl-devel",
            "gtk4",
            "gtk4-devel",
            "gcc",
            "make",
            "rpm-build",
        ] {
            let query = Command::new("rpm").args(["-q", name]).output()?;
            let version = query
                .status
                .success()
                .then(|| String::from_utf8_lossy(&query.stdout).trim().to_owned());
            packages.insert(name, version);
        }
        result["packages"] = serde_json::to_value(packages)?;
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        result["native_firefox_profile_directory_present"] = json!(
            home.join(".mozilla/firefox").is_dir() || config.join("mozilla/firefox").is_dir()
        );
        result["flatpak_firefox_directory_present"] =
            json!(home.join(".var/app/org.mozilla.firefox").is_dir());
    }
    if let Some(path) = module {
        let mut file = fs::File::open(path)?;
        let metadata = file.metadata()?;
        ensure!(metadata.is_file(), "O módulo precisa ser arquivo regular");
        let mut header = Vec::new();
        file.by_ref().take(32).read_to_end(&mut header)?;
        result["module"] = elf_info(&header, metadata.len());
    }
    Ok(result)
}

fn elf_info(header: &[u8], size: u64) -> Value {
    let is_elf = header.starts_with(b"\x7fELF");
    let mut result = json!({"size_bytes": size, "is_elf": is_elf});
    if is_elf && header.len() >= 20 && matches!(header[4], 1 | 2) && matches!(header[5], 1 | 2) {
        let bits = if header[4] == 1 { 32 } else { 64 };
        let machine = if header[5] == 1 {
            u16::from_le_bytes([header[18], header[19]])
        } else {
            u16::from_be_bytes([header[18], header[19]])
        };
        result["bits"] = json!(bits);
        result["machine_id"] = json!(machine);
        result["matches_process_bits"] = json!(bits == size_of::<*const ()>() * 8);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inspects_truncated_and_big_endian_headers_without_loading() {
        assert_eq!(elf_info(b"abc", 3)["is_elf"], false);
        assert!(elf_info(b"\x7fELF", 4).get("bits").is_none());
        let mut bytes = [0; 20];
        bytes[..6].copy_from_slice(b"\x7fELF\x02\x02");
        bytes[19] = 62;
        let result = elf_info(&bytes, 20);
        assert_eq!(result["bits"], 64);
        assert_eq!(result["machine_id"], 62);
    }
}
