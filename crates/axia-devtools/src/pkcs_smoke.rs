use crate::{
    lab, lab_helper,
    pkcs::{Module, ck, expect},
};
use anyhow::{Result, ensure};
use cryptoki_sys::*;
use serde_json::{Value, json};
use std::{collections::BTreeSet, env, fs, mem, path::Path, ptr};

pub fn smoke(root: &Path, module: &Path, client: &Path, report: &Path) -> Result<()> {
    let module = module.canonicalize()?;
    let client = client.canonicalize()?;
    lab_helper::inventory_worker(root, &module, &client)?;
    let temp = tempfile::Builder::new()
        .prefix("axia-pkcs11-lab-")
        .tempdir()?;
    let home = temp.path();
    lab_helper::mark(home)?;
    let state = home.join("identity");
    lab::initialize(&client, home, &state, &lab::fixture(root)?)?;
    let helper = lab_helper::install(home)?;
    let result = lab::successful(
        lab::command(&env::current_exe()?, home)
            .arg("--root")
            .arg(root)
            .args(["pkcs11-worker", "--module"])
            .arg(&module)
            .arg("--home")
            .arg(home)
            .env("AXIA_LAB_HOME", home)
            .env("DESKTOPID_HELPER", &helper),
    )?;
    let mut value: Value = serde_json::from_slice(&result.stdout)?;
    lab_helper::interactive_test(&module, home)?;
    value["module_sha256"] = json!(lab_helper::fingerprint(&module)?);
    value["module_is_elf"] = json!(fs::read(&module)?.starts_with(b"\x7fELF"));
    lab::report(report, &value)?;
    println!(
        "PKCS11: {} laboratory checks passed; report: {}",
        value["checks"].as_array().unwrap().len(),
        report.display()
    );
    Ok(())
}

fn calls(home: &Path) -> Result<Vec<Value>> {
    match fs::read_to_string(home.join("calls.jsonl")) {
        Ok(lines) => lines
            .lines()
            .map(|line| Ok(serde_json::from_str(line)?))
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.into()),
    }
}

pub fn worker(root: &Path, path: &Path, home: &Path) -> Result<()> {
    lab_helper::validate_home(home)?;
    let fixture = lab::fixture(root)?;
    let mut document = lab::decode(&fixture["document_base64"])?;
    let certificate =
        openssl::x509::X509::from_pem(fixture["certificate_pem"].as_str().unwrap().as_bytes())?;
    let mut checks = Vec::new();
    let mut module = Module::load(path)?;
    let mut slot_count = 0;
    expect(
        ck!(
            module,
            C_GetSlotList(CK_TRUE, ptr::null_mut(), &mut slot_count)
        ),
        CKR_CRYPTOKI_NOT_INITIALIZED,
        "C_GetSlotList before init",
    )?;
    let offset = mem::offset_of!(CK_FUNCTION_LIST, C_Initialize);
    ensure!(
        (mem::size_of::<CK_FUNCTION_LIST>() - offset) / mem::size_of::<usize>() == 68,
        "Tabela PKCS11 sem 68 funções"
    );
    // SAFETY: C-repr structure has 68 consecutive nullable function pointers.
    let table = unsafe {
        std::slice::from_raw_parts(
            (&module.table as *const CK_FUNCTION_LIST)
                .cast::<u8>()
                .add(offset)
                .cast::<usize>(),
            68,
        )
    };
    ensure!(table.iter().all(|entry| *entry != 0), "Função PKCS11 nula");
    checks.push("v2.40-table-with-68-non-null-functions");
    module.initialize()?;
    expect(
        ck!(module, C_GetFunctionStatus(0)),
        CKR_FUNCTION_NOT_PARALLEL,
        "C_GetFunctionStatus",
    )?;
    expect(
        ck!(module, C_CancelFunction(0)),
        CKR_FUNCTION_NOT_PARALLEL,
        "C_CancelFunction",
    )?;
    expect(
        ck!(module, C_Initialize(ptr::null_mut())),
        CKR_CRYPTOKI_ALREADY_INITIALIZED,
        "C_Initialize twice",
    )?;
    expect(
        ck!(
            module,
            C_GetSlotList(CK_TRUE, ptr::null_mut(), &mut slot_count)
        ),
        CKR_OK,
        "C_GetSlotList",
    )?;
    ensure!(slot_count == 1, "Contagem de slots incorreta");
    let mut slot = 0;
    slot_count = 0;
    expect(
        ck!(module, C_GetSlotList(CK_TRUE, &mut slot, &mut slot_count)),
        CKR_BUFFER_TOO_SMALL,
        "short slots",
    )?;
    expect(
        ck!(module, C_GetSlotList(CK_TRUE, &mut slot, &mut slot_count)),
        CKR_OK,
        "slots",
    )?;
    ensure!(slot == 8, "Slot incorreto");
    checks.push("initialization-and-slot-size-negotiation");
    let session = module.open()?;
    let other = module.open()?;
    let s = session.handle;
    ensure!(
        session.find(None)?.len() == 3 && session.find(Some(CKO_PRIVATE_KEY))?.len() == 1,
        "Objetos públicos iniciais incorretos"
    );
    session.login()?;
    ensure!(
        session.find(None)?.len() == 3 && other.find(Some(CKO_PRIVATE_KEY))?.len() == 1,
        "Login alterou referências"
    );
    let cert = session.find(Some(CKO_CERTIFICATE))?[0];
    let public = session.find(Some(CKO_PUBLIC_KEY))?[0];
    let private = session.find(Some(CKO_PRIVATE_KEY))?[0];
    checks.push("public-remote-key-references-and-optional-protected-path-login");
    ensure!(
        session.attribute(cert, CKA_VALUE)? == certificate.to_der()?,
        "Certificado DER mudou"
    );
    ensure!(
        session.attribute(cert, CKA_ID)? == session.attribute(public, CKA_ID)?
            && session.attribute(public, CKA_ID)? == session.attribute(private, CKA_ID)?,
        "IDs não correspondem"
    );
    let mut sensitive = CK_ATTRIBUTE {
        type_: CKA_PRIVATE_EXPONENT,
        pValue: ptr::null_mut(),
        ulValueLen: 0,
    };
    expect(
        ck!(module, C_GetAttributeValue(s, private, &mut sensitive, 1)),
        CKR_ATTRIBUTE_SENSITIVE,
        "private exponent",
    )?;
    ensure!(
        sensitive.ulValueLen == CK_UNAVAILABLE_INFORMATION,
        "Comprimento sensível incorreto"
    );
    let mut short = [0u8];
    let mut mixed = [
        CK_ATTRIBUTE {
            type_: CKA_VALUE,
            pValue: short.as_mut_ptr().cast(),
            ulValueLen: 1,
        },
        CK_ATTRIBUTE {
            type_: CKA_CLASS,
            pValue: ptr::null_mut(),
            ulValueLen: 0,
        },
    ];
    expect(
        ck!(module, C_GetAttributeValue(s, cert, mixed.as_mut_ptr(), 2)),
        CKR_BUFFER_TOO_SMALL,
        "mixed attributes",
    )?;
    ensure!(
        mixed[0].ulValueLen > 1 && mixed[1].ulValueLen == mem::size_of::<CK_ULONG>() as CK_ULONG,
        "Semântica de atributos perdida"
    );
    checks.push("DER-identities-private-exponent-protection-and-attribute-buffer-semantics");
    let mut mechanism = CK_MECHANISM {
        mechanism: CKM_SHA256_RSA_PKCS,
        pParameter: ptr::null_mut(),
        ulParameterLen: 0,
    };
    let mut output = vec![0; 256];
    let mut length = 0;
    expect(
        ck!(module, C_SignInit(s, &mut mechanism, private)),
        CKR_OK,
        "SignInit",
    )?;
    expect(
        ck!(module, C_SignInit(s, &mut mechanism, private)),
        CKR_OPERATION_ACTIVE,
        "SignInit twice",
    )?;
    expect(
        ck!(
            module,
            C_Sign(
                s,
                document.as_mut_ptr(),
                document.len() as CK_ULONG,
                ptr::null_mut(),
                &mut length
            )
        ),
        CKR_OK,
        "sign length",
    )?;
    ensure!(
        length == 256 && calls(home)?.is_empty(),
        "Consulta de tamanho assinou"
    );
    length = 1;
    expect(
        ck!(
            module,
            C_Sign(
                s,
                document.as_mut_ptr(),
                document.len() as CK_ULONG,
                output.as_mut_ptr(),
                &mut length
            )
        ),
        CKR_BUFFER_TOO_SMALL,
        "short sign",
    )?;
    ensure!(
        length == 256 && calls(home)?.is_empty(),
        "Buffer curto assinou"
    );
    expect(
        ck!(
            module,
            C_Sign(
                other.handle,
                document.as_mut_ptr(),
                document.len() as CK_ULONG,
                output.as_mut_ptr(),
                &mut length
            )
        ),
        CKR_OPERATION_NOT_INITIALIZED,
        "other sign",
    )?;
    expect(
        ck!(
            module,
            C_Sign(
                s,
                document.as_mut_ptr(),
                document.len() as CK_ULONG,
                output.as_mut_ptr(),
                &mut length
            )
        ),
        CKR_OK,
        "sign",
    )?;
    ensure!(
        calls(home)?.len() == 1 && output == lab::decode(&fixture["signatures"]["SHA256"])?,
        "Assinatura difere do fixture ou repetiu"
    );
    expect(
        ck!(
            module,
            C_Sign(
                s,
                document.as_mut_ptr(),
                document.len() as CK_ULONG,
                output.as_mut_ptr(),
                &mut length
            )
        ),
        CKR_OPERATION_NOT_INITIALIZED,
        "consumed sign",
    )?;
    checks.push("size-and-short-buffer-do-not-sign;one-effective-call-consumes-operation");
    expect(
        ck!(module, C_VerifyInit(s, &mut mechanism, public)),
        CKR_OK,
        "VerifyInit",
    )?;
    expect(
        ck!(
            module,
            C_Verify(
                s,
                document.as_mut_ptr(),
                document.len() as CK_ULONG,
                output.as_mut_ptr(),
                256
            )
        ),
        CKR_OK,
        "Verify",
    )?;
    let mut changed = b"changed".to_vec();
    expect(
        ck!(module, C_VerifyInit(s, &mut mechanism, public)),
        CKR_OK,
        "VerifyInit changed",
    )?;
    expect(
        ck!(
            module,
            C_Verify(
                s,
                changed.as_mut_ptr(),
                changed.len() as CK_ULONG,
                output.as_mut_ptr(),
                256
            )
        ),
        CKR_SIGNATURE_INVALID,
        "tamper",
    )?;
    checks.push("local-verification-and-tamper-rejection");
    for (kind, label) in [
        (CKM_SHA256_RSA_PKCS, "SHA256"),
        (CKM_SHA384_RSA_PKCS, "SHA384"),
        (CKM_SHA512_RSA_PKCS, "SHA512"),
    ] {
        mechanism.mechanism = kind;
        expect(
            ck!(module, C_SignInit(s, &mut mechanism, private)),
            CKR_OK,
            "multipart init",
        )?;
        expect(
            ck!(module, C_SignUpdate(s, document.as_mut_ptr(), 10)),
            CKR_OK,
            "update first",
        )?;
        expect(
            ck!(
                module,
                C_SignUpdate(
                    s,
                    document[10..].as_mut_ptr(),
                    (document.len() - 10) as CK_ULONG
                )
            ),
            CKR_OK,
            "update last",
        )?;
        let before = calls(home)?.len();
        length = 0;
        expect(
            ck!(module, C_SignFinal(s, ptr::null_mut(), &mut length)),
            CKR_OK,
            "final size",
        )?;
        ensure!(calls(home)?.len() == before, "Consulta final assinou");
        length = 256;
        expect(
            ck!(module, C_SignFinal(s, output.as_mut_ptr(), &mut length)),
            CKR_OK,
            "final",
        )?;
        ensure!(
            calls(home)?.len() == before + 1
                && output == lab::decode(&fixture["signatures"][label])?,
            "Multipart incorreto"
        );
    }
    checks.push("multipart-sha256-sha384-sha512-and-final-length-query");
    let mut encoded = vec![
        0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        0x05, 0x00, 0x04, 0x20,
    ];
    encoded.extend_from_slice(&openssl::sha::sha256(&document));
    mechanism.mechanism = CKM_RSA_PKCS;
    expect(
        ck!(module, C_SignInit(s, &mut mechanism, private)),
        CKR_OK,
        "raw init",
    )?;
    expect(
        ck!(
            module,
            C_Sign(
                s,
                encoded.as_mut_ptr(),
                encoded.len() as CK_ULONG,
                output.as_mut_ptr(),
                &mut length
            )
        ),
        CKR_OK,
        "digestinfo sign",
    )?;
    ensure!(
        output == lab::decode(&fixture["signatures"]["SHA256"])?,
        "DigestInfo foi re-hasheado"
    );
    let before = calls(home)?.len();
    let mut raw = b"raw".to_vec();
    expect(
        ck!(module, C_SignInit(s, &mut mechanism, private)),
        CKR_OK,
        "invalid raw init",
    )?;
    expect(
        ck!(
            module,
            C_Sign(s, raw.as_mut_ptr(), 3, output.as_mut_ptr(), &mut length)
        ),
        CKR_DATA_INVALID,
        "invalid raw",
    )?;
    ensure!(calls(home)?.len() == before, "Input bruto chamou helper");
    checks.push("RSA-PKCS-DigestInfo-not-double-hashed;raw-input-rejected-before-helper");
    let pids: BTreeSet<_> = calls(home)?
        .iter()
        .map(|v| v["helper_pid"].as_u64().unwrap())
        .collect();
    ensure!(pids.len() == 1, "Helper não foi reutilizado");
    checks.push("same-helper-process-survives-multiple-signature-operations");
    for (mode, expected) in [
        ("cancel", CKR_FUNCTION_CANCELED),
        ("invalid", CKR_SIGNATURE_INVALID),
    ] {
        lab::private_write(&home.join("helper-mode"), mode.as_bytes())?;
        mechanism.mechanism = CKM_SHA256_RSA_PKCS;
        expect(
            ck!(module, C_SignInit(s, &mut mechanism, private)),
            CKR_OK,
            "failed helper init",
        )?;
        expect(
            ck!(
                module,
                C_Sign(
                    s,
                    document.as_mut_ptr(),
                    document.len() as CK_ULONG,
                    output.as_mut_ptr(),
                    &mut length
                )
            ),
            expected,
            "failed helper sign",
        )?;
        expect(
            ck!(
                module,
                C_Sign(
                    s,
                    document.as_mut_ptr(),
                    document.len() as CK_ULONG,
                    output.as_mut_ptr(),
                    &mut length
                )
            ),
            CKR_OPERATION_NOT_INITIALIZED,
            "failed operation consumed",
        )?;
    }
    checks.push("cancellation-and-invalid-helper-signature-terminate-without-retry");
    mechanism.mechanism = CKM_RSA_PKCS_PSS;
    expect(
        ck!(module, C_SignInit(s, &mut mechanism, private)),
        CKR_MECHANISM_INVALID,
        "PSS unsupported",
    )?;
    session.logout()?;
    ensure!(
        other.find(Some(CKO_PRIVATE_KEY))?.len() == 1,
        "Logout perdeu referências públicas"
    );
    lab::private_write(&home.join("helper-mode"), b"")?;
    let before = calls(home)?.len();
    mechanism.mechanism = CKM_SHA256_RSA_PKCS;
    expect(
        ck!(module, C_SignInit(other.handle, &mut mechanism, private)),
        CKR_OK,
        "without login init",
    )?;
    expect(
        ck!(
            module,
            C_Sign(
                other.handle,
                document.as_mut_ptr(),
                document.len() as CK_ULONG,
                output.as_mut_ptr(),
                &mut length
            )
        ),
        CKR_OK,
        "without login sign",
    )?;
    let rows = calls(home)?;
    let current_pid = rows.last().unwrap()["helper_pid"].as_u64().unwrap();
    ensure!(
        rows.len() == before + 1
            && output == lab::decode(&fixture["signatures"]["SHA256"])?
            && !pids.contains(&current_pid),
        "Logout não encerrou helper"
    );
    checks.push("signature-without-token-login-still-requires-one-effective-helper-call");
    drop(session);
    drop(other);
    drop(module);
    ensure!(
        !Path::new(&format!("/proc/{current_pid}")).exists(),
        "Finalização deixou helper vivo"
    );
    checks.push("unsupported-PSS-and-session-cleanup");
    for args in [
        vec!["--show-info"],
        vec!["--list-slots"],
        vec!["--list-mechanisms"],
        vec!["--list-objects", "--login"],
    ] {
        lab::successful(
            lab::command(Path::new("pkcs11-tool"), home)
                .arg("--module")
                .arg(path)
                .args(args)
                .env("DESKTOPID_STATE_DIR", home.join("identity")),
        )?;
    }
    checks.push("OpenSC-loads-function-table-slots-mechanisms-and-objects");
    let input = home.join("native-input.txt");
    let output = home.join("native-signature.bin");
    lab::private_write(&input, &document)?;
    let before = calls(home)?.len();
    lab::successful(
        lab::command(Path::new("pkcs11-tool"), home)
            .arg("--module")
            .arg(path)
            .args([
                "--slot",
                "8",
                "--login",
                "--sign",
                "--mechanism",
                "SHA256-RSA-PKCS",
                "--id",
                &crate::publication::digest(&certificate.to_der()?),
            ])
            .arg("--input-file")
            .arg(input)
            .arg("--output-file")
            .arg(&output)
            .env("DESKTOPID_STATE_DIR", home.join("identity"))
            .env("DESKTOPID_HELPER", home.join(lab_helper::NAME))
            .env("AXIA_LAB_HOME", home),
    )?;
    ensure!(
        calls(home)?.len() == before + 1
            && fs::read(&output)? == lab::decode(&fixture["signatures"]["SHA256"])?,
        "OpenSC assinatura efetiva falhou"
    );
    checks.push("OpenSC-effective-signature-with-exact-real-test-arguments");
    println!(
        "{}",
        json!({"schema_version":1,"successful":true,"scope":"native-pkcs11-laboratory","checks":checks,"real_cloud_request_sent":false,"real_credentials_used":false,"gnome_authorization_window_tested":false,"browser_or_pjeoffice_tested":false})
    );
    Ok(())
}
