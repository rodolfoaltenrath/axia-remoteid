use crate::{core::*, export, helper};
use cryptoki_sys::*;
use std::{ffi::c_void, ptr, slice};

fn padded<const N: usize>(text: &[u8]) -> [u8; N] {
    let mut output = [b' '; N];
    let count = text.len().min(N);
    output[..count].copy_from_slice(&text[..count]);
    output
}
unsafe fn input<'a>(pointer: *mut u8, length: CK_ULONG) -> Result<&'a [u8], CK_RV> {
    if length as usize > MAX_INPUT {
        return Err(CKR_DATA_LEN_RANGE);
    }
    if length == 0 {
        return Ok(&[]);
    }
    if pointer.is_null() {
        return Err(CKR_ARGUMENTS_BAD);
    }
    Ok(unsafe { slice::from_raw_parts(pointer, length as usize) })
}
fn session_info(state: &State, flags: CK_FLAGS) -> CK_STATE {
    match (flags & CKF_RW_SESSION != 0, state.logged_in) {
        (false, false) => CKS_RO_PUBLIC_SESSION,
        (false, true) => CKS_RO_USER_FUNCTIONS,
        (true, false) => CKS_RW_PUBLIC_SESSION,
        (true, true) => CKS_RW_USER_FUNCTIONS,
    }
}
export!(C_Initialize(args: *mut c_void) {
    if !args.is_null() {
        let args = &*(args.cast::<CK_C_INITIALIZE_ARGS>());
        if !args.pReserved.is_null() { return CKR_ARGUMENTS_BAD; }
        let count = [args.CreateMutex.is_some(), args.DestroyMutex.is_some(), args.LockMutex.is_some(), args.UnlockMutex.is_some()].iter().filter(|v| **v).count();
        if count != 0 && count != 4 { return CKR_ARGUMENTS_BAD; }
        if count == 4 && args.flags & CKF_OS_LOCKING_OK == 0 { return CKR_CANT_LOCK; }
    }
    let Ok(mut lock) = STATE.lock() else { return CKR_DEVICE_ERROR; };
    if lock.is_some() { return CKR_CRYPTOKI_ALREADY_INITIALIZED; }
    match State::load() { Ok(state) => { *lock = Some(state); CKR_OK }, Err(rv) => rv }
});
export!(C_Finalize(reserved: *mut c_void) {
    if !reserved.is_null() { return CKR_ARGUMENTS_BAD; }
    let Ok(mut lock) = STATE.lock() else { return CKR_DEVICE_ERROR; };
    let Some(state) = lock.as_ref() else { return CKR_CRYPTOKI_NOT_INITIALIZED; };
    if state.sessions.values().any(|session| session.busy) { return CKR_FUNCTION_FAILED; }
    *lock = None; helper::clear_authorization(); CKR_OK
});
export!(C_GetInfo(info: *mut CK_INFO) {
    with_state(|_| {
        if info.is_null() { return CKR_ARGUMENTS_BAD; }
        *info = CK_INFO { cryptokiVersion:CK_VERSION { major:2, minor:40 },
            manufacturerID:padded(b"Axia RemotedID"), flags:0,
            libraryDescription:padded(b"Axia RemotedID PKCS11"), libraryVersion:CK_VERSION { major:0, minor:1 } };
        CKR_OK
    })
});
export!(C_GetSlotList(_present: CK_BBOOL, slots: *mut CK_SLOT_ID, count: *mut CK_ULONG) {
    with_state(|_| {
        if count.is_null() { return CKR_ARGUMENTS_BAD; }
        if slots.is_null() { *count = 1; return CKR_OK; }
        let capacity = *count; *count = 1;
        if capacity < 1 { return CKR_BUFFER_TOO_SMALL; }
        *slots = SLOT; CKR_OK
    })
});
export!(C_GetSlotInfo(slot: CK_SLOT_ID, info: *mut CK_SLOT_INFO) {
    with_state(|_| {
        if slot != SLOT { return CKR_SLOT_ID_INVALID; }
        if info.is_null() { return CKR_ARGUMENTS_BAD; }
        *info = CK_SLOT_INFO { slotDescription:padded(b"Axia RemotedID cloud certificates"),
            manufacturerID:padded(b"Axia RemotedID"), flags:CKF_TOKEN_PRESENT,
            hardwareVersion:CK_VERSION { major:0, minor:0 }, firmwareVersion:CK_VERSION { major:0, minor:1 } };
        CKR_OK
    })
});
export!(C_GetTokenInfo(slot: CK_SLOT_ID, info: *mut CK_TOKEN_INFO) {
    with_state(|state| {
        if slot != SLOT { return CKR_SLOT_ID_INVALID; }
        if info.is_null() { return CKR_ARGUMENTS_BAD; }
        *info = CK_TOKEN_INFO { label:padded(b"Axia RemotedID"), manufacturerID:padded(b"Axia RemotedID"),
            model:padded(b"Cloud RSA"), serialNumber:padded(b"LOCAL"),
            // No separate token login: references are public and each actual
            // signature gets its own local authorization. Keep protected-path
            // support for callers that explicitly invoke C_Login with no PIN.
            flags:CKF_WRITE_PROTECTED | CKF_USER_PIN_INITIALIZED | CKF_TOKEN_INITIALIZED | CKF_PROTECTED_AUTHENTICATION_PATH | CKF_RNG,
            ulMaxSessionCount:64, ulSessionCount:state.sessions.len() as CK_ULONG,
            ulMaxRwSessionCount:64, ulRwSessionCount:state.sessions.values().filter(|s| s.flags & CKF_RW_SESSION != 0).count() as CK_ULONG,
            ulMaxPinLen:0, ulMinPinLen:0,
            ulTotalPublicMemory:CK_UNAVAILABLE_INFORMATION, ulFreePublicMemory:CK_UNAVAILABLE_INFORMATION,
            ulTotalPrivateMemory:CK_UNAVAILABLE_INFORMATION, ulFreePrivateMemory:CK_UNAVAILABLE_INFORMATION,
            hardwareVersion:CK_VERSION { major:0, minor:0 }, firmwareVersion:CK_VERSION { major:0, minor:1 }, utcTime:[b' ';16] };
        CKR_OK
    })
});
export!(C_GetMechanismList(slot: CK_SLOT_ID, output: *mut CK_MECHANISM_TYPE, count: *mut CK_ULONG) {
    with_state(|_| {
        if slot != SLOT { return CKR_SLOT_ID_INVALID; }
        if count.is_null() { return CKR_ARGUMENTS_BAD; }
        let capacity = if output.is_null() { 0 } else { *count as usize }; *count = MECHANISMS.len() as CK_ULONG;
        if output.is_null() { return CKR_OK; }
        if capacity < MECHANISMS.len() { return CKR_BUFFER_TOO_SMALL; }
        ptr::copy_nonoverlapping(MECHANISMS.as_ptr(), output, MECHANISMS.len()); CKR_OK
    })
});
export!(C_GetMechanismInfo(slot: CK_SLOT_ID, mechanism: CK_MECHANISM_TYPE, info: *mut CK_MECHANISM_INFO) {
    with_state(|_| {
        if slot != SLOT { return CKR_SLOT_ID_INVALID; }
        if !MECHANISMS.contains(&mechanism) { return CKR_MECHANISM_INVALID; }
        if info.is_null() { return CKR_ARGUMENTS_BAD; }
        *info = CK_MECHANISM_INFO { ulMinKeySize:2048, ulMaxKeySize:8192, flags:CKF_SIGN | CKF_VERIFY };
        CKR_OK
    })
});
export!(C_OpenSession(slot: CK_SLOT_ID, flags: CK_FLAGS, _application: *mut c_void, _notify: CK_NOTIFY, handle: *mut CK_SESSION_HANDLE) {
    with_state(|state| {
        if slot != SLOT { return CKR_SLOT_ID_INVALID; }
        if handle.is_null() { return CKR_ARGUMENTS_BAD; }
        if flags & CKF_SERIAL_SESSION == 0 { return CKR_SESSION_PARALLEL_NOT_SUPPORTED; }
        if flags & !(CKF_SERIAL_SESSION | CKF_RW_SESSION) != 0 { return CKR_ARGUMENTS_BAD; }
        if state.sessions.len() >= 64 { return CKR_SESSION_COUNT; }
        let id = next_session();
        state.sessions.insert(id, Session { flags, ..Session::default() }); *handle = id; CKR_OK
    })
});
export!(C_CloseSession(handle: CK_SESSION_HANDLE) {
    with_state(|state| {
        let Some(session) = state.sessions.get(&handle) else { return CKR_SESSION_HANDLE_INVALID; };
        if session.busy { return CKR_FUNCTION_FAILED; }
        state.sessions.remove(&handle); if state.sessions.is_empty() { state.logged_in = false; } CKR_OK
    })
});
export!(C_CloseAllSessions(slot: CK_SLOT_ID) {
    with_state(|state| {
        if slot != SLOT { return CKR_SLOT_ID_INVALID; }
        if state.sessions.values().any(|s| s.busy) { return CKR_FUNCTION_FAILED; }
        state.sessions.clear(); state.logged_in = false; CKR_OK
    })
});
export!(C_GetSessionInfo(handle: CK_SESSION_HANDLE, info: *mut CK_SESSION_INFO) {
    with_state(|state| {
        let Some(session) = state.sessions.get(&handle) else { return CKR_SESSION_HANDLE_INVALID; };
        if info.is_null() { return CKR_ARGUMENTS_BAD; }
        *info = CK_SESSION_INFO { slotID:SLOT, state:session_info(state, session.flags), flags:session.flags, ulDeviceError:0 }; CKR_OK
    })
});
export!(C_Login(handle: CK_SESSION_HANDLE, user: CK_USER_TYPE, _pin: *mut CK_UTF8CHAR, length: CK_ULONG) {
    with_state(|state| {
        if !state.sessions.contains_key(&handle) { return CKR_SESSION_HANDLE_INVALID; }
        if user != CKU_USER { return CKR_USER_TYPE_INVALID; }
        // Compatibility login is local only; C_Sign always prompts credentials.
        if length != 0 { return CKR_PIN_LEN_RANGE; }
        if state.logged_in { return CKR_USER_ALREADY_LOGGED_IN; }
        state.logged_in = true; CKR_OK
    })
});
export!(C_Logout(handle: CK_SESSION_HANDLE) {
    with_state(|state| {
        if !state.sessions.contains_key(&handle) { return CKR_SESSION_HANDLE_INVALID; }
        if state.sessions.values().any(|s| s.busy) { return CKR_FUNCTION_FAILED; }
        helper::clear_authorization();
        if !state.logged_in { return CKR_USER_NOT_LOGGED_IN; }
        state.logged_in = false;
        for session in state.sessions.values_mut() { session.sign = None; session.search = None; }
        CKR_OK
    })
});
export!(C_FindObjectsInit(handle: CK_SESSION_HANDLE, template: *mut CK_ATTRIBUTE, count: CK_ULONG) {
    with_state(|state| {
        let Some(session) = state.sessions.get(&handle) else { return CKR_SESSION_HANDLE_INVALID; };
        if session.search.is_some() { return CKR_OPERATION_ACTIVE; }
        if count > 128 || (template.is_null() && count != 0) { return CKR_ARGUMENTS_BAD; }
        let attrs = if count == 0 { &[][..] } else { slice::from_raw_parts(template, count as usize) };
        let mut filters = Vec::new();
        for attr in attrs {
            let data = match input(attr.pValue.cast(), attr.ulValueLen) { Ok(v) => v, Err(rv) => return rv };
            filters.push((attr.type_, data));
        }
        let matches = state.objects.iter()
            .filter(|(_, object)| filters.iter().all(|(ty, value)| object.attrs.get(ty).is_some_and(|v| v.as_slice() == *value)))
            .map(|(id, _)| *id).collect();
        state.sessions.get_mut(&handle).unwrap().search = Some(Search { matches, offset:0 }); CKR_OK
    })
});
export!(C_FindObjects(handle: CK_SESSION_HANDLE, output: *mut CK_OBJECT_HANDLE, capacity: CK_ULONG, count: *mut CK_ULONG) {
    with_state(|state| {
        let Some(session) = state.sessions.get_mut(&handle) else { return CKR_SESSION_HANDLE_INVALID; };
        let Some(search) = session.search.as_mut() else { return CKR_OPERATION_NOT_INITIALIZED; };
        if count.is_null() || (output.is_null() && capacity != 0) || capacity > 65536 { return CKR_ARGUMENTS_BAD; }
        let size = (capacity as usize).min(search.matches.len() - search.offset);
        if size != 0 { ptr::copy_nonoverlapping(search.matches[search.offset..].as_ptr(), output, size); }
        search.offset += size; *count = size as CK_ULONG; CKR_OK
    })
});
export!(C_FindObjectsFinal(handle: CK_SESSION_HANDLE) {
    with_state(|state| {
        let Some(session) = state.sessions.get_mut(&handle) else { return CKR_SESSION_HANDLE_INVALID; };
        if session.search.take().is_none() { return CKR_OPERATION_NOT_INITIALIZED; } CKR_OK
    })
});
export!(C_GetAttributeValue(handle: CK_SESSION_HANDLE, object: CK_OBJECT_HANDLE, template: *mut CK_ATTRIBUTE, count: CK_ULONG) {
    with_state(|state| {
        if !state.sessions.contains_key(&handle) { return CKR_SESSION_HANDLE_INVALID; }
        let Some(object) = state.objects.get(&object) else { return CKR_OBJECT_HANDLE_INVALID; };
        if count > 128 || (template.is_null() && count != 0) { return CKR_ARGUMENTS_BAD; }
        let attrs = if count == 0 { &mut [][..] } else { slice::from_raw_parts_mut(template, count as usize) };
        let mut rv = CKR_OK;
        for attr in attrs {
            if object.private_key && [CKA_VALUE, CKA_PRIVATE_EXPONENT, CKA_PRIME_1, CKA_PRIME_2, CKA_EXPONENT_1, CKA_EXPONENT_2, CKA_COEFFICIENT].contains(&attr.type_) {
                attr.ulValueLen = CK_UNAVAILABLE_INFORMATION; rv = CKR_ATTRIBUTE_SENSITIVE; continue;
            }
            let Some(bytes) = object.attrs.get(&attr.type_) else {
                attr.ulValueLen = CK_UNAVAILABLE_INFORMATION;
                if rv != CKR_ATTRIBUTE_SENSITIVE { rv = CKR_ATTRIBUTE_TYPE_INVALID; } continue;
            };
            let capacity = if attr.pValue.is_null() { 0 } else { attr.ulValueLen as usize }; attr.ulValueLen = bytes.len() as CK_ULONG;
            if attr.pValue.is_null() { continue; }
            if capacity < bytes.len() {
                if rv == CKR_OK { rv = CKR_BUFFER_TOO_SMALL; } continue;
            }
            if !bytes.is_empty() { ptr::copy_nonoverlapping(bytes.as_ptr(), attr.pValue.cast::<u8>(), bytes.len()); }
        }
        rv
    })
});
export!(C_GetObjectSize(handle: CK_SESSION_HANDLE, object: CK_OBJECT_HANDLE, size: *mut CK_ULONG) {
    with_state(|state| {
        if !state.sessions.contains_key(&handle) { return CKR_SESSION_HANDLE_INVALID; }
        let Some(object) = state.objects.get(&object) else { return CKR_OBJECT_HANDLE_INVALID; };
        if size.is_null() { return CKR_ARGUMENTS_BAD; }
        *size = object.attrs.values().map(|v| v.len() as CK_ULONG).sum(); CKR_OK
    })
});
unsafe fn operation_init(
    handle: CK_SESSION_HANDLE,
    mechanism: *mut CK_MECHANISM,
    object: CK_OBJECT_HANDLE,
    sign: bool,
) -> CK_RV {
    with_state(|state| {
        let Some(session) = state.sessions.get(&handle) else {
            return CKR_SESSION_HANDLE_INVALID;
        };
        if session.busy
            || (if sign {
                session.sign.is_some()
            } else {
                session.verify.is_some()
            })
        {
            return CKR_OPERATION_ACTIVE;
        }
        if mechanism.is_null() {
            return CKR_ARGUMENTS_BAD;
        }
        let mechanism = unsafe { &*mechanism };
        if !MECHANISMS.contains(&mechanism.mechanism) {
            return CKR_MECHANISM_INVALID;
        }
        if mechanism.ulParameterLen != 0 || !mechanism.pParameter.is_null() {
            return CKR_MECHANISM_PARAM_INVALID;
        }
        let Some(object) = state.objects.get(&object) else {
            return CKR_KEY_HANDLE_INVALID;
        };
        if object.attrs.get(&CKA_CLASS)
            != Some(&ulong(if sign {
                CKO_PRIVATE_KEY
            } else {
                CKO_PUBLIC_KEY
            }))
        {
            return CKR_KEY_TYPE_INCONSISTENT;
        }
        let op = Operation {
            mechanism: mechanism.mechanism,
            certificate: object.certificate,
            data: Vec::new(),
            multipart: false,
        };
        let session = state.sessions.get_mut(&handle).unwrap();
        if sign {
            session.sign = Some(op);
        } else {
            session.verify = Some(op);
        }
        CKR_OK
    })
}
export!(C_SignInit(handle: CK_SESSION_HANDLE, mechanism: *mut CK_MECHANISM, object: CK_OBJECT_HANDLE) { operation_init(handle, mechanism, object, true) });
export!(C_VerifyInit(handle: CK_SESSION_HANDLE, mechanism: *mut CK_MECHANISM, object: CK_OBJECT_HANDLE) { operation_init(handle, mechanism, object, false) });

unsafe fn operation_update(
    handle: CK_SESSION_HANDLE,
    pointer: *mut u8,
    length: CK_ULONG,
    sign: bool,
) -> CK_RV {
    with_state(|state| {
        let Some(session) = state.sessions.get_mut(&handle) else {
            return CKR_SESSION_HANDLE_INVALID;
        };
        if session.busy {
            return CKR_OPERATION_ACTIVE;
        }
        let operation = if sign {
            &mut session.sign
        } else {
            &mut session.verify
        };
        let Some(op) = operation.as_mut() else {
            return CKR_OPERATION_NOT_INITIALIZED;
        };
        if op.mechanism == CKM_RSA_PKCS {
            *operation = None;
            return CKR_MECHANISM_INVALID;
        }
        let data = match unsafe { input(pointer, length) } {
            Ok(data) => data,
            Err(rv) => {
                *operation = None;
                return rv;
            }
        };
        if op.data.len().saturating_add(data.len()) > MAX_INPUT {
            *operation = None;
            return CKR_DATA_LEN_RANGE;
        }
        op.data.extend_from_slice(data);
        op.multipart = true;
        CKR_OK
    })
}
export!(C_SignUpdate(handle: CK_SESSION_HANDLE, pointer: *mut CK_BYTE, length: CK_ULONG) { operation_update(handle, pointer, length, true) });
export!(C_VerifyUpdate(handle: CK_SESSION_HANDLE, pointer: *mut CK_BYTE, length: CK_ULONG) { operation_update(handle, pointer, length, false) });

unsafe fn perform_sign(
    handle: CK_SESSION_HANDLE,
    data: Option<(*mut u8, CK_ULONG)>,
    output: *mut u8,
    length: *mut CK_ULONG,
) -> CK_RV {
    let plan = {
        let Ok(mut lock) = STATE.lock() else {
            return CKR_DEVICE_ERROR;
        };
        let Some(state) = lock.as_mut() else {
            return CKR_CRYPTOKI_NOT_INITIALIZED;
        };
        let Some(session) = state.sessions.get_mut(&handle) else {
            return CKR_SESSION_HANDLE_INVALID;
        };
        if session.busy {
            return CKR_OPERATION_ACTIVE;
        }
        let Some(op) = session.sign.as_ref() else {
            return CKR_OPERATION_NOT_INITIALIZED;
        };
        if length.is_null() {
            session.sign = None;
            return CKR_ARGUMENTS_BAD;
        }
        let entry = &state.certificates[op.certificate];
        let required = match entry.certificate.public_key().and_then(|p| p.rsa()) {
            Ok(rsa) => rsa.size() as CK_ULONG,
            Err(_) => return CKR_KEY_TYPE_INCONSISTENT,
        };
        let capacity = if output.is_null() {
            0
        } else {
            unsafe { *length }
        };
        unsafe {
            *length = required;
        }
        // These paths do not consume an operation or invoke the helper.
        if output.is_null() {
            return CKR_OK;
        }
        if capacity < required {
            return CKR_BUFFER_TOO_SMALL;
        }
        let mut op = session.sign.take().unwrap();
        if let Some((pointer, count)) = data {
            if op.multipart {
                return CKR_FUNCTION_FAILED;
            }
            op.data = match unsafe { input(pointer, count) } {
                Ok(v) => v.to_vec(),
                Err(rv) => return rv,
            };
        } else if op.mechanism == CKM_RSA_PKCS {
            return CKR_MECHANISM_INVALID;
        }
        let (algorithm, digest) = match input_digest(op.mechanism, &op.data) {
            Ok(value) => value,
            Err(rv) => return rv,
        };
        let index = op.certificate;
        let entry = desktopid_client::client::WalletCertificate {
            key_name: state.certificates[index].key_name.clone(),
            certificate: state.certificates[index].certificate.clone(),
        };
        let fingerprint = match helper::fingerprint(state, index) {
            Ok(v) => v,
            Err(rv) => return rv,
        };
        state.sessions.get_mut(&handle).unwrap().busy = true;
        (
            state.root.clone(),
            entry,
            fingerprint,
            algorithm,
            digest,
            required,
        )
    };
    // Release global state while user authorizes the operation in a child process.
    let (root, entry, fingerprint, algorithm, digest, required) = plan;
    let result =
        helper::helper_signature(&root, &fingerprint, algorithm, &digest).and_then(|signature| {
            check_signature(&entry, algorithm, &digest, &signature)?;
            Ok(signature)
        });
    if result.is_err() {
        helper::clear_authorization();
    }
    with_state(|state| {
        let Some(session) = state.sessions.get_mut(&handle) else {
            return CKR_SESSION_HANDLE_INVALID;
        };
        session.busy = false;
        match result {
            Ok(signature) if signature.len() == required as usize => {
                unsafe {
                    ptr::copy_nonoverlapping(signature.as_ptr(), output, signature.len());
                }
                CKR_OK
            }
            Ok(_) => CKR_SIGNATURE_LEN_RANGE,
            Err(rv) => rv,
        }
    })
}
export!(C_Sign(handle: CK_SESSION_HANDLE, pointer: *mut CK_BYTE, count: CK_ULONG, output: *mut CK_BYTE, length: *mut CK_ULONG) { perform_sign(handle, Some((pointer, count)), output, length) });
export!(C_SignFinal(handle: CK_SESSION_HANDLE, output: *mut CK_BYTE, length: *mut CK_ULONG) { perform_sign(handle, None, output, length) });

unsafe fn perform_verify(
    handle: CK_SESSION_HANDLE,
    data: Option<(*mut u8, CK_ULONG)>,
    signature: *mut u8,
    length: CK_ULONG,
) -> CK_RV {
    with_state(|state| {
        let Some(session) = state.sessions.get_mut(&handle) else {
            return CKR_SESSION_HANDLE_INVALID;
        };
        let Some(mut op) = session.verify.take() else {
            return CKR_OPERATION_NOT_INITIALIZED;
        };
        if let Some((pointer, count)) = data {
            if op.multipart {
                return CKR_FUNCTION_FAILED;
            }
            op.data = match unsafe { input(pointer, count) } {
                Ok(v) => v.to_vec(),
                Err(rv) => return rv,
            };
        } else if op.mechanism == CKM_RSA_PKCS {
            return CKR_MECHANISM_INVALID;
        }
        let (algorithm, digest) = match input_digest(op.mechanism, &op.data) {
            Ok(value) => value,
            Err(rv) => return rv,
        };
        let entry = &state.certificates[op.certificate];
        let expected = match entry.certificate.public_key().and_then(|p| p.rsa()) {
            Ok(rsa) => rsa.size() as CK_ULONG,
            Err(_) => return CKR_KEY_TYPE_INCONSISTENT,
        };
        if length != expected {
            return CKR_SIGNATURE_LEN_RANGE;
        }
        let signature = match unsafe { input(signature, length) } {
            Ok(v) => v,
            Err(rv) => return rv,
        };
        match check_signature(entry, algorithm, &digest, signature) {
            Ok(()) => CKR_OK,
            Err(rv) => rv,
        }
    })
}
export!(C_Verify(handle: CK_SESSION_HANDLE, pointer: *mut CK_BYTE, count: CK_ULONG, signature: *mut CK_BYTE, length: CK_ULONG) { perform_verify(handle, Some((pointer, count)), signature, length) });
export!(C_VerifyFinal(handle: CK_SESSION_HANDLE, signature: *mut CK_BYTE, length: CK_ULONG) { perform_verify(handle, None, signature, length) });
export!(C_GenerateRandom(handle: CK_SESSION_HANDLE, pointer: *mut CK_BYTE, count: CK_ULONG) {
    with_state(|state| {
        if !state.sessions.contains_key(&handle) { return CKR_SESSION_HANDLE_INVALID; }
        if count as usize > MAX_INPUT || (pointer.is_null() && count != 0) { return CKR_ARGUMENTS_BAD; }
        if count == 0 { return CKR_OK; }
        match openssl::rand::rand_bytes(slice::from_raw_parts_mut(pointer, count as usize)) { Ok(()) => CKR_OK, Err(_) => CKR_FUNCTION_FAILED }
    })
});
export!(C_WaitForSlotEvent(flags: CK_FLAGS, _slot: *mut CK_SLOT_ID, reserved: *mut c_void) {
    with_state(|_| {
        if !reserved.is_null() { return CKR_ARGUMENTS_BAD; }
        if flags == CKF_DONT_BLOCK { CKR_NO_EVENT } else { CKR_FUNCTION_NOT_SUPPORTED }
    })
});
