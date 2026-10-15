//! Minimal typed PKCS#11 loader for developer tests, not a runtime dependency.
use anyhow::{Result, ensure};
use cryptoki_sys::*;
use libloading::Library;
use serde_json::{Value, json};
use std::{mem, path::Path, ptr};

macro_rules! ck {
    ($module:expr, $name:ident($($arg:expr),* $(,)?)) => {{
        let function = $module.table.$name.ok_or_else(|| anyhow::anyhow!(concat!(stringify!($name), " ausente")))?;
        // SAFETY: typed ABI; callers supply buffers living through this call.
        unsafe { function($($arg),*) }
    }};
}
pub(crate) use ck;

pub fn expect(rv: CK_RV, expected: CK_RV, operation: &str) -> Result<()> {
    ensure!(
        rv == expected,
        "{operation}: esperado {expected:#x}, recebido {rv:#x}"
    );
    Ok(())
}

pub struct Module {
    pub table: CK_FUNCTION_LIST,
    _library: Library,
    initialized: bool,
}

impl Module {
    pub fn load(path: &Path) -> Result<Self> {
        // SAFETY: loading an explicitly selected native module is the purpose of
        // this developer command. No symbol outlives its owned Library.
        let library = unsafe { Library::new(path.canonicalize()?) }?;
        let mut table = ptr::null_mut();
        let get: libloading::Symbol<'_, unsafe extern "C" fn(*mut CK_FUNCTION_LIST_PTR) -> CK_RV> =
            unsafe { library.get(b"C_GetFunctionList\0") }?;
        expect(unsafe { get(&mut table) }, CKR_OK, "C_GetFunctionList")?;
        ensure!(!table.is_null(), "Tabela PKCS11 ausente");
        // SAFETY: the module's successful C_GetFunctionList returns this table.
        let table = unsafe { *table };
        ensure!(
            table.version.major == 2 && table.version.minor == 40,
            "Esperada ABI PKCS11 2.40 do Axia"
        );
        ensure!(
            table.C_GetFunctionList.map(|f| f as usize) == Some(*get as usize),
            "Tabela PKCS11 desviada por biblioteca do host"
        );
        Ok(Self {
            table,
            _library: library,
            initialized: false,
        })
    }
    pub fn initialize(&mut self) -> Result<()> {
        expect(
            ck!(self, C_Initialize(ptr::null_mut())),
            CKR_OK,
            "C_Initialize",
        )?;
        self.initialized = true;
        Ok(())
    }
    pub fn open(&self) -> Result<Session<'_>> {
        let mut count = 0;
        expect(
            ck!(self, C_GetSlotList(CK_TRUE, ptr::null_mut(), &mut count)),
            CKR_OK,
            "C_GetSlotList",
        )?;
        ensure!(count == 1, "Esperado um slot Axia");
        let mut slot = 0;
        expect(
            ck!(self, C_GetSlotList(CK_TRUE, &mut slot, &mut count)),
            CKR_OK,
            "C_GetSlotList",
        )?;
        ensure!(slot == 8, "Esperado slot Axia 8");
        let mut handle = 0;
        expect(
            ck!(
                self,
                C_OpenSession(slot, CKF_SERIAL_SESSION, ptr::null_mut(), None, &mut handle)
            ),
            CKR_OK,
            "C_OpenSession",
        )?;
        Ok(Session {
            module: self,
            handle,
        })
    }
}
impl Drop for Module {
    fn drop(&mut self) {
        if self.initialized
            && let Some(finalize) = self.table.C_Finalize
        {
            // SAFETY: no Session can outlive the borrowed Module.
            unsafe {
                finalize(ptr::null_mut());
            }
        }
    }
}

pub struct Session<'a> {
    pub module: &'a Module,
    pub handle: CK_SESSION_HANDLE,
}
impl Session<'_> {
    pub fn find(&self, class: Option<CK_OBJECT_CLASS>) -> Result<Vec<CK_OBJECT_HANDLE>> {
        let mut class = class;
        let mut attribute = CK_ATTRIBUTE {
            type_: CKA_CLASS,
            pValue: class
                .as_mut()
                .map_or(ptr::null_mut(), |c| (c as *mut CK_ULONG).cast()),
            ulValueLen: mem::size_of::<CK_ULONG>() as CK_ULONG,
        };
        expect(
            ck!(
                self.module,
                C_FindObjectsInit(
                    self.handle,
                    if class.is_some() {
                        &mut attribute
                    } else {
                        ptr::null_mut()
                    },
                    class.is_some() as CK_ULONG
                )
            ),
            CKR_OK,
            "C_FindObjectsInit",
        )?;
        let result = (|| {
            let mut handles = Vec::new();
            loop {
                let mut block = [0; 64];
                let mut count = 0;
                expect(
                    ck!(
                        self.module,
                        C_FindObjects(
                            self.handle,
                            block.as_mut_ptr(),
                            block.len() as CK_ULONG,
                            &mut count
                        )
                    ),
                    CKR_OK,
                    "C_FindObjects",
                )?;
                ensure!(
                    count as usize <= block.len(),
                    "Contagem PKCS11 excede buffer"
                );
                handles.extend_from_slice(&block[..count as usize]);
                if count == 0 {
                    break;
                }
                ensure!(handles.len() <= 65536, "Enumeração PKCS11 não terminou");
            }
            Ok(handles)
        })();
        let final_rv = ck!(self.module, C_FindObjectsFinal(self.handle));
        let handles = result?;
        expect(final_rv, CKR_OK, "C_FindObjectsFinal")?;
        Ok(handles)
    }
    pub fn attribute(&self, object: CK_OBJECT_HANDLE, kind: CK_ATTRIBUTE_TYPE) -> Result<Vec<u8>> {
        let mut attribute = CK_ATTRIBUTE {
            type_: kind,
            pValue: ptr::null_mut(),
            ulValueLen: 0,
        };
        expect(
            ck!(
                self.module,
                C_GetAttributeValue(self.handle, object, &mut attribute, 1)
            ),
            CKR_OK,
            "C_GetAttributeValue",
        )?;
        ensure!(
            attribute.ulValueLen <= 4 * 1024 * 1024,
            "Atributo PKCS11 excede limite"
        );
        let mut value = vec![0; attribute.ulValueLen as usize];
        attribute.pValue = value.as_mut_ptr().cast();
        expect(
            ck!(
                self.module,
                C_GetAttributeValue(self.handle, object, &mut attribute, 1)
            ),
            CKR_OK,
            "C_GetAttributeValue",
        )?;
        ensure!(
            attribute.ulValueLen as usize <= value.len(),
            "Atributo excedeu buffer"
        );
        value.truncate(attribute.ulValueLen as usize);
        Ok(value)
    }
    pub fn login(&self) -> Result<()> {
        expect(
            ck!(
                self.module,
                C_Login(self.handle, CKU_USER, ptr::null_mut(), 0)
            ),
            CKR_OK,
            "C_Login",
        )
    }
    pub fn logout(&self) -> Result<()> {
        expect(ck!(self.module, C_Logout(self.handle)), CKR_OK, "C_Logout")
    }
}
impl Drop for Session<'_> {
    fn drop(&mut self) {
        if let Some(close) = self.module.table.C_CloseSession {
            unsafe {
                close(self.handle);
            }
        }
    }
}

pub fn inventory(path: &Path) -> Result<Value> {
    let mut module = Module::load(path)?;
    module.initialize()?;
    let session = module.open()?;
    let certs = session.find(Some(CKO_CERTIFICATE))?.len();
    let public = session.find(Some(CKO_PUBLIC_KEY))?.len();
    let before = session.find(Some(CKO_PRIVATE_KEY))?.len();
    session.login()?;
    let private = session.find(Some(CKO_PRIVATE_KEY))?.len();
    session.logout()?;
    Ok(
        json!({"scope":"local PKCS11 inventory only","certificate_count":certs,"public_key_count":public,"private_references_before_local_login":before,"private_key_reference_count":private,"remote_signature_requested":false,"remote_login_registration_or_wallet_requested":false,"credentials_requested":false,"certificate_names_or_identifiers_disclosed":false}),
    )
}
