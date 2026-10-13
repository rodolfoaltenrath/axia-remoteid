// PKCS11 v2.40 API signatures from cryptoki-sys 0.5.0 / OASIS headers.
// All entries are non-null. Unimplemented operations return an explicit error.
use crate::{core::with_state, export};
use cryptoki_sys::*;
export!(C_InitToken(_arg1: CK_SLOT_ID, _arg2: *mut CK_UTF8CHAR, _arg3: CK_ULONG, _arg4: *mut CK_UTF8CHAR) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_InitPIN(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_UTF8CHAR, _arg3: CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_SetPIN(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_UTF8CHAR, _arg3: CK_ULONG, _arg4: *mut CK_UTF8CHAR, _arg5: CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_GetOperationState(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: *mut CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_SetOperationState(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: CK_ULONG, _arg4: CK_OBJECT_HANDLE, _arg5: CK_OBJECT_HANDLE) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_CreateObject(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_ATTRIBUTE, _arg3: CK_ULONG, _arg4: *mut CK_OBJECT_HANDLE) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_CopyObject(_arg1: CK_SESSION_HANDLE, _arg2: CK_OBJECT_HANDLE, _arg3: *mut CK_ATTRIBUTE, _arg4: CK_ULONG, _arg5: *mut CK_OBJECT_HANDLE) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_DestroyObject(_arg1: CK_SESSION_HANDLE, _arg2: CK_OBJECT_HANDLE) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_SetAttributeValue(_arg1: CK_SESSION_HANDLE, _arg2: CK_OBJECT_HANDLE, _arg3: *mut CK_ATTRIBUTE, _arg4: CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_EncryptInit(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_MECHANISM, _arg3: CK_OBJECT_HANDLE) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_Encrypt(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: CK_ULONG, _arg4: *mut CK_BYTE, _arg5: *mut CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_EncryptUpdate(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: CK_ULONG, _arg4: *mut CK_BYTE, _arg5: *mut CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_EncryptFinal(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: *mut CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_DecryptInit(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_MECHANISM, _arg3: CK_OBJECT_HANDLE) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_Decrypt(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: CK_ULONG, _arg4: *mut CK_BYTE, _arg5: *mut CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_DecryptUpdate(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: CK_ULONG, _arg4: *mut CK_BYTE, _arg5: *mut CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_DecryptFinal(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: *mut CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_DigestInit(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_MECHANISM) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_Digest(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: CK_ULONG, _arg4: *mut CK_BYTE, _arg5: *mut CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_DigestUpdate(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_DigestKey(_arg1: CK_SESSION_HANDLE, _arg2: CK_OBJECT_HANDLE) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_DigestFinal(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: *mut CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_SignRecoverInit(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_MECHANISM, _arg3: CK_OBJECT_HANDLE) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_SignRecover(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: CK_ULONG, _arg4: *mut CK_BYTE, _arg5: *mut CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_VerifyRecoverInit(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_MECHANISM, _arg3: CK_OBJECT_HANDLE) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_VerifyRecover(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: CK_ULONG, _arg4: *mut CK_BYTE, _arg5: *mut CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_DigestEncryptUpdate(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: CK_ULONG, _arg4: *mut CK_BYTE, _arg5: *mut CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_DecryptDigestUpdate(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: CK_ULONG, _arg4: *mut CK_BYTE, _arg5: *mut CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_SignEncryptUpdate(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: CK_ULONG, _arg4: *mut CK_BYTE, _arg5: *mut CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_DecryptVerifyUpdate(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: CK_ULONG, _arg4: *mut CK_BYTE, _arg5: *mut CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_GenerateKey(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_MECHANISM, _arg3: *mut CK_ATTRIBUTE, _arg4: CK_ULONG, _arg5: *mut CK_OBJECT_HANDLE) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_GenerateKeyPair(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_MECHANISM, _arg3: *mut CK_ATTRIBUTE, _arg4: CK_ULONG, _arg5: *mut CK_ATTRIBUTE, _arg6: CK_ULONG, _arg7: *mut CK_OBJECT_HANDLE, _arg8: *mut CK_OBJECT_HANDLE) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_WrapKey(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_MECHANISM, _arg3: CK_OBJECT_HANDLE, _arg4: CK_OBJECT_HANDLE, _arg5: *mut CK_BYTE, _arg6: *mut CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_UnwrapKey(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_MECHANISM, _arg3: CK_OBJECT_HANDLE, _arg4: *mut CK_BYTE, _arg5: CK_ULONG, _arg6: *mut CK_ATTRIBUTE, _arg7: CK_ULONG, _arg8: *mut CK_OBJECT_HANDLE) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_DeriveKey(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_MECHANISM, _arg3: CK_OBJECT_HANDLE, _arg4: *mut CK_ATTRIBUTE, _arg5: CK_ULONG, _arg6: *mut CK_OBJECT_HANDLE) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_SeedRandom(_arg1: CK_SESSION_HANDLE, _arg2: *mut CK_BYTE, _arg3: CK_ULONG) { with_state(|_| CKR_FUNCTION_NOT_SUPPORTED) });
export!(C_GetFunctionStatus(_arg1: CK_SESSION_HANDLE) { with_state(|_| CKR_FUNCTION_NOT_PARALLEL) });
export!(C_CancelFunction(_arg1: CK_SESSION_HANDLE) { with_state(|_| CKR_FUNCTION_NOT_PARALLEL) });
export!(C_GetFunctionList(output: *mut *mut CK_FUNCTION_LIST) {
    if output.is_null() { return CKR_ARGUMENTS_BAD; }
    *output = std::ptr::from_ref(&FUNCTIONS).cast_mut(); CKR_OK
});
static FUNCTIONS: CK_FUNCTION_LIST = CK_FUNCTION_LIST {
    version: CK_VERSION {
        major: 2,
        minor: 40,
    },
    C_Initialize: Some(crate::C_Initialize),
    C_Finalize: Some(crate::C_Finalize),
    C_GetInfo: Some(crate::C_GetInfo),
    C_GetFunctionList: Some(crate::C_GetFunctionList),
    C_GetSlotList: Some(crate::C_GetSlotList),
    C_GetSlotInfo: Some(crate::C_GetSlotInfo),
    C_GetTokenInfo: Some(crate::C_GetTokenInfo),
    C_GetMechanismList: Some(crate::C_GetMechanismList),
    C_GetMechanismInfo: Some(crate::C_GetMechanismInfo),
    C_InitToken: Some(crate::C_InitToken),
    C_InitPIN: Some(crate::C_InitPIN),
    C_SetPIN: Some(crate::C_SetPIN),
    C_OpenSession: Some(crate::C_OpenSession),
    C_CloseSession: Some(crate::C_CloseSession),
    C_CloseAllSessions: Some(crate::C_CloseAllSessions),
    C_GetSessionInfo: Some(crate::C_GetSessionInfo),
    C_GetOperationState: Some(crate::C_GetOperationState),
    C_SetOperationState: Some(crate::C_SetOperationState),
    C_Login: Some(crate::C_Login),
    C_Logout: Some(crate::C_Logout),
    C_CreateObject: Some(crate::C_CreateObject),
    C_CopyObject: Some(crate::C_CopyObject),
    C_DestroyObject: Some(crate::C_DestroyObject),
    C_GetObjectSize: Some(crate::C_GetObjectSize),
    C_GetAttributeValue: Some(crate::C_GetAttributeValue),
    C_SetAttributeValue: Some(crate::C_SetAttributeValue),
    C_FindObjectsInit: Some(crate::C_FindObjectsInit),
    C_FindObjects: Some(crate::C_FindObjects),
    C_FindObjectsFinal: Some(crate::C_FindObjectsFinal),
    C_EncryptInit: Some(crate::C_EncryptInit),
    C_Encrypt: Some(crate::C_Encrypt),
    C_EncryptUpdate: Some(crate::C_EncryptUpdate),
    C_EncryptFinal: Some(crate::C_EncryptFinal),
    C_DecryptInit: Some(crate::C_DecryptInit),
    C_Decrypt: Some(crate::C_Decrypt),
    C_DecryptUpdate: Some(crate::C_DecryptUpdate),
    C_DecryptFinal: Some(crate::C_DecryptFinal),
    C_DigestInit: Some(crate::C_DigestInit),
    C_Digest: Some(crate::C_Digest),
    C_DigestUpdate: Some(crate::C_DigestUpdate),
    C_DigestKey: Some(crate::C_DigestKey),
    C_DigestFinal: Some(crate::C_DigestFinal),
    C_SignInit: Some(crate::C_SignInit),
    C_Sign: Some(crate::C_Sign),
    C_SignUpdate: Some(crate::C_SignUpdate),
    C_SignFinal: Some(crate::C_SignFinal),
    C_SignRecoverInit: Some(crate::C_SignRecoverInit),
    C_SignRecover: Some(crate::C_SignRecover),
    C_VerifyInit: Some(crate::C_VerifyInit),
    C_Verify: Some(crate::C_Verify),
    C_VerifyUpdate: Some(crate::C_VerifyUpdate),
    C_VerifyFinal: Some(crate::C_VerifyFinal),
    C_VerifyRecoverInit: Some(crate::C_VerifyRecoverInit),
    C_VerifyRecover: Some(crate::C_VerifyRecover),
    C_DigestEncryptUpdate: Some(crate::C_DigestEncryptUpdate),
    C_DecryptDigestUpdate: Some(crate::C_DecryptDigestUpdate),
    C_SignEncryptUpdate: Some(crate::C_SignEncryptUpdate),
    C_DecryptVerifyUpdate: Some(crate::C_DecryptVerifyUpdate),
    C_GenerateKey: Some(crate::C_GenerateKey),
    C_GenerateKeyPair: Some(crate::C_GenerateKeyPair),
    C_WrapKey: Some(crate::C_WrapKey),
    C_UnwrapKey: Some(crate::C_UnwrapKey),
    C_DeriveKey: Some(crate::C_DeriveKey),
    C_SeedRandom: Some(crate::C_SeedRandom),
    C_GenerateRandom: Some(crate::C_GenerateRandom),
    C_GetFunctionStatus: Some(crate::C_GetFunctionStatus),
    C_CancelFunction: Some(crate::C_CancelFunction),
    C_WaitForSlotEvent: Some(crate::C_WaitForSlotEvent),
};
