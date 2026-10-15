//! Independent NSS test oracle. Production registration remains in axia-desktop.
use anyhow::{Result, ensure};
use libloading::Library;
use std::{
    ffi::{CString, c_char, c_int, c_void},
    fs,
    path::{Path, PathBuf},
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
};

pub const FILES: [&str; 3] = ["pkcs11.txt", "cert9.db", "key4.db"];
pub const SYSTEM_MODULE: &str = "/usr/lib64/axia-remotedid/libdesktopid_pkcs11.so";
#[repr(C)]
struct Links {
    next: *mut Links,
    previous: *mut Links,
}
#[repr(C)]
struct CertificateNode {
    links: Links,
    certificate: *mut c_void,
}

pub struct Database {
    library: Library,
    initialized: bool,
}
impl Database {
    pub fn open(profile: &Path) -> Result<Self> {
        // SAFETY: system NSS has the documented C ABI; owned library outlives calls.
        let library = unsafe { Library::new("libnss3.so") }?;
        let mut database = Self {
            library,
            initialized: false,
        };
        let path = CString::new(format!("sql:{}", profile.display()))?;
        let init: unsafe extern "C" fn(*const c_char) -> c_int =
            database.function(b"NSS_InitReadWrite\0")?;
        let rv = unsafe { init(path.as_ptr()) };
        ensure!(rv == 0, "NSS initialization failed");
        database.initialized = true;
        Ok(database)
    }
    pub fn close(mut self) -> Result<()> {
        let shutdown: unsafe extern "C" fn() -> c_int = self.function(b"NSS_Shutdown\0")?;
        self.initialized = false;
        ensure!(unsafe { shutdown() } == 0, "NSS shutdown failed");
        Ok(())
    }
    fn function<T: Copy>(&self, name: &[u8]) -> Result<T> {
        // SAFETY: each call site supplies the exact NSS prototype.
        Ok(unsafe { *self.library.get::<T>(name)? })
    }
    pub fn check_key_without_login(&self) -> Result<()> {
        static PROMPTS: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "C" fn password(_: *mut c_void, _: c_int, _: *mut c_void) -> *mut c_char {
            PROMPTS.fetch_add(1, Ordering::SeqCst);
            ptr::null_mut()
        }
        let set: unsafe extern "C" fn(
            Option<unsafe extern "C" fn(*mut c_void, c_int, *mut c_void) -> *mut c_char>,
        ) = self.function(b"PK11_SetPasswordFunc\0")?;
        let find: unsafe extern "C" fn(*const c_char) -> *mut c_void =
            self.function(b"PK11_FindSlotByName\0")?;
        let free: unsafe extern "C" fn(*mut c_void) = self.function(b"PK11_FreeSlot\0")?;
        let list: unsafe extern "C" fn(*mut c_void) -> *mut Links =
            self.function(b"PK11_ListCertsInSlot\0")?;
        let destroy: unsafe extern "C" fn(*mut Links) = self.function(b"CERT_DestroyCertList\0")?;
        let need: unsafe extern "C" fn(*mut c_void) -> c_int =
            self.function(b"PK11_NeedLogin\0")?;
        let authenticate: unsafe extern "C" fn(*mut c_void, c_int, *mut c_void) -> c_int =
            self.function(b"PK11_Authenticate\0")?;
        let key: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            self.function(b"PK11_FindKeyByAnyCert\0")?;
        let destroy_key: unsafe extern "C" fn(*mut c_void) =
            self.function(b"SECKEY_DestroyPrivateKey\0")?;
        PROMPTS.store(0, Ordering::SeqCst);
        // SAFETY: callback has static lifetime; NSS returns valid referenced
        // slot/list/key objects which are destroyed before library shutdown.
        unsafe {
            set(Some(password));
            let slot = find(c"Axia RemotedID".as_ptr());
            let mut certificates = ptr::null_mut();
            let result = (|| {
                ensure!(!slot.is_null(), "Token NSS não encontrado");
                ensure!(
                    need(slot) == 0 && authenticate(slot, 1, ptr::null_mut()) == 0,
                    "NSS exige login preliminar"
                );
                certificates = list(slot);
                ensure!(!certificates.is_null(), "Lista NSS ausente");
                let node = (*certificates).next;
                ensure!(
                    !node.is_null() && node != certificates,
                    "Certificado NSS ausente"
                );
                let certificate = (*node.cast::<CertificateNode>()).certificate;
                let key = key(certificate, ptr::null_mut());
                ensure!(!key.is_null(), "NSS perdeu referência remota da chave");
                destroy_key(key);
                ensure!(
                    PROMPTS.load(Ordering::SeqCst) == 0,
                    "NSS solicitou senha desnecessária"
                );
                Ok(())
            })();
            set(None);
            if !certificates.is_null() {
                destroy(certificates);
            }
            if !slot.is_null() {
                free(slot);
            }
            result
        }
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        if self.initialized
            && let Ok(shutdown) =
                self.function::<unsafe extern "C" fn() -> c_int>(b"NSS_Shutdown\0")
        {
            unsafe {
                shutdown();
            }
        }
    }
}

pub fn registered_library(profile: &Path) -> Result<Option<PathBuf>> {
    let text = fs::read_to_string(profile.join("pkcs11.txt"))?;
    for section in text.split("\n\n") {
        let values: std::collections::BTreeMap<_, _> = section
            .lines()
            .filter_map(|line| line.split_once('='))
            .collect();
        if values.get("name") == Some(&"DesktopID Linux Alpha") {
            return Ok(values.get("library").map(PathBuf::from));
        }
    }
    Ok(None)
}
