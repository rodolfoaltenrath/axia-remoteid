use crate::{diagnostic, publication};
use anyhow::{Context, Result, ensure};
use flate2::{Compression, GzBuilder};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};
use tar::{Builder, EntryType, Header};
use tempfile::NamedTempFile;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const DOCUMENTS: &[&str] = &[
    "README.md",
    "docs/TESTING.md",
    "docs/DEVELOPMENT.md",
    "docs/PROTOCOL.md",
    "CONTINUIDADE_IA_FEDORA.md",
    "LICENSE",
    "NOTICE",
];
const BINARIES: &[&str] = &["desktopid-client", "libdesktopid_pkcs11.so"];

pub fn version(root: &Path) -> Result<String> {
    let manifest: toml::Value = toml::from_str(&fs::read_to_string(root.join("Cargo.toml"))?)?;
    let version = manifest
        .get("workspace")
        .and_then(|v| v.get("package"))
        .and_then(|v| v.get("version"))
        .and_then(|v| v.as_str())
        .context("Versão ausente no workspace")?;
    ensure!(
        !version.is_empty()
            && version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b)),
        "Versão inválida para nome de pacote"
    );
    Ok(version.into())
}

fn temporary(output: &Path) -> Result<NamedTempFile> {
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    Ok(NamedTempFile::new_in(parent)?)
}

fn commit(mut temp: NamedTempFile, output: &Path) -> Result<()> {
    temp.flush()?;
    temp.as_file().sync_all()?;
    // Do not follow an existing symlink at the destination.
    temp.persist(output).map_err(|e| e.error)?;
    Ok(())
}

fn checksum(output: &Path) -> Result<()> {
    let digest = publication::digest(&fs::read(output)?);
    let name = output
        .file_name()
        .and_then(|n| n.to_str())
        .context("Nome de pacote sem UTF-8")?;
    let sum = output.with_file_name(format!("{name}.sha256"));
    let mut temp = temporary(&sum)?;
    writeln!(temp, "{digest}  {name}")?;
    commit(temp, &sum)?;
    println!(
        "{}: {} bytes; SHA256 {digest}; no account state",
        output.display(),
        fs::metadata(output)?.len()
    );
    Ok(())
}

pub fn sources(root: &Path, output: &Path) -> Result<()> {
    let names = publication::candidates(root)?;
    let (files, issues) = publication::snapshot(root, &names)?;
    if !issues.is_empty() {
        publication::report(names.len(), &issues);
        anyhow::bail!("Publicação rejeitada; nenhum arquivo foi empacotado");
    }
    let mut temp = temporary(output)?;
    {
        let mut archive = ZipWriter::new(temp.as_file_mut());
        for (name, data) in files {
            let name = name.to_str().context("Nome de fonte sem UTF-8")?;
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .compression_level(Some(9))
                .last_modified_time(zip::DateTime::default())
                .unix_permissions(if name.ends_with(".sh") { 0o755 } else { 0o644 });
            archive.start_file(format!("desktopid-linux/{name}"), options)?;
            archive.write_all(&data)?;
        }
        archive.finish()?;
    }
    commit(temp, output)?;
    checksum(output)
}

fn runtime_entries(root: &Path, target: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut entries = BTreeMap::new();
    for name in BINARIES {
        let path = target.join(name);
        ensure!(
            fs::symlink_metadata(&path)?.file_type().is_file(),
            "Artefato ELF precisa ser um arquivo regular: {name}"
        );
        let data = fs::read(path)?;
        // Validate ELF64 little endian x86_64, ET_EXEC or ET_DYN before labeling
        // an archive as Fedora x86_64. Do not execute an artifact to inspect it.
        ensure!(
            data.len() >= 20
                && &data[..4] == b"\x7fELF"
                && data[4] == 2
                && data[5] == 1
                && matches!(u16::from_le_bytes([data[16], data[17]]), 2 | 3)
                && u16::from_le_bytes([data[18], data[19]]) == 62,
            "Esperado ELF x86_64 nativo: {name}"
        );
        entries.insert((*name).into(), data);
    }
    let names: Vec<_> = DOCUMENTS.iter().map(PathBuf::from).collect();
    let (documents, issues) = publication::snapshot(root, &names)?;
    ensure!(
        issues.is_empty() && documents.len() == DOCUMENTS.len(),
        "Documentação do pacote ausente ou rejeitada pelo verificador de publicação"
    );
    for (name, data) in documents {
        entries.insert(name.to_str().context("Nome sem UTF-8")?.into(), data);
    }
    Ok(entries)
}

fn write_tar(
    output: &Path,
    prefix: &str,
    entries: &BTreeMap<String, Vec<u8>>,
    private: bool,
    alias: bool,
) -> Result<()> {
    let mut temp = temporary(output)?;
    {
        let compressed = GzBuilder::new()
            .mtime(0)
            .write(temp.as_file_mut(), Compression::best());
        let mut archive = Builder::new(compressed);
        for (name, data) in entries {
            let mut header = Header::new_gnu();
            header.set_size(data.len() as u64);
            let executable = BINARIES.contains(&name.as_str());
            header.set_mode(match (private, executable) {
                (true, true) => 0o700,
                (true, false) => 0o600,
                (false, true) => 0o755,
                _ => 0o644,
            });
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(0);
            archive.append_data(&mut header, format!("{prefix}/{name}"), data.as_slice())?;
        }
        if alias {
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Link);
            header.set_size(0);
            header.set_mode(0o700);
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(0);
            header.set_link_name(format!("{prefix}/desktopid-client"))?;
            // Hardlink must follow its target in a sequential extraction.
            archive.append_data(
                &mut header,
                format!("{prefix}/axia-remotedid"),
                std::io::empty(),
            )?;
        }
        archive.into_inner()?.finish()?;
    }
    commit(temp, output)
}

pub fn linux(root: &Path, target: &Path, output: &Path) -> Result<()> {
    let mut entries = runtime_entries(root, target)?;
    let testing = String::from_utf8(entries["docs/TESTING.md"].clone())?
        .replace("(DEVELOPMENT.md)", "(docs/DEVELOPMENT.md)");
    entries.insert("LEIA-ME.md".into(), testing.into_bytes());
    let mut hashes: BTreeMap<_, _> = entries
        .iter()
        .map(|(name, data)| (name.clone(), publication::digest(data)))
        .collect();
    hashes.insert("axia-remotedid".into(), hashes["desktopid-client"].clone());
    let os = diagnostic::os_release()?;
    let manifest = json!({
        "schema_version": 1, "product": "Axia RemotedID",
        "scope": "experimental PKCS11 runtime; integrations pending", "version": version(root)?,
        "built_on": os.get("PRETTY_NAME").map(String::as_str).unwrap_or("Linux"),
        "channel": "alpha", "machine": std::env::consts::ARCH,
        "validated_target": {"os": "fedora", "version": "44", "machine": "x86_64"},
        "runtime_dependencies": ["gtk4", "nss", "openssl-libs", "libgcc", "zlib", "ca-certificates"],
        "installs_browsers": false, "requires_interpreter": false, "includes_account_state": false,
        "files": hashes
    });
    entries.insert(
        "manifest.json".into(),
        serde_json::to_vec_pretty(&manifest)?,
    );
    write_tar(output, "axia-remotedid", &entries, true, true)?;
    checksum(output)
}

pub fn rpm(
    root: &Path,
    target: &Path,
    destination: &Path,
    builder: &Path,
    container: bool,
    image: &str,
) -> Result<()> {
    let os = diagnostic::os_release()?;
    ensure!(
        os.get("ID").map(String::as_str) == Some("fedora")
            && os.get("VERSION_ID").map(String::as_str) == Some("44")
            && std::env::consts::ARCH == "x86_64",
        "Gere o RPM no Fedora 44 x86_64"
    );
    let mut entries = runtime_entries(root, target)?;
    let desktop = PathBuf::from("packaging/axia-remotedid-alpha.desktop");
    let (files, issues) = publication::snapshot(root, &[desktop])?;
    ensure!(
        files.len() == 1 && issues.is_empty(),
        "Entrada de desktop ausente ou rejeitada"
    );
    entries.insert("axia-remotedid-alpha.desktop".into(), files[0].1.clone());
    let spec_data = fs::read(root.join("packaging/axia-remotedid.spec"))?;
    fs::create_dir_all(destination)?;
    let destination = destination.canonicalize()?;
    let work_root = destination.join("rpm-build");
    fs::create_dir_all(&work_root)?;
    // Unique work directory avoids publishing a stale RPM after a failed build.
    let top = tempfile::Builder::new()
        .prefix(&format!("{}-", &publication::digest(&spec_data)[..12]))
        .tempdir_in(&work_root)?;
    for directory in ["BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS"] {
        fs::create_dir(top.path().join(directory))?;
    }
    // tempfile directories default to 0700; allow an unprivileged rpmbuild in
    // the disposable container to read its explicitly mounted inputs.
    fs::set_permissions(top.path(), fs::Permissions::from_mode(0o755))?;
    let source = top.path().join("SOURCES/axia-remotedid-runtime.tar.gz");
    write_tar(&source, "axia-remotedid-runtime", &entries, false, false)?;
    fs::set_permissions(&source, fs::Permissions::from_mode(0o644))?;
    let spec = top.path().join("SPECS/axia-remotedid.spec");
    fs::write(&spec, spec_data)?;
    let status = if container {
        ensure!(
            !image.is_empty() && !image.starts_with('-'),
            "Imagem Podman inválida"
        );
        Command::new("podman").args(["run", "--rm", "--security-opt", "label=disable"])
            .arg("-v").arg(format!("{}:/build/SOURCES:ro", top.path().join("SOURCES").display()))
            .arg("-v").arg(format!("{}:/build/SPECS:ro", top.path().join("SPECS").display()))
            .arg("-v").arg(format!("{}:/build/RPMS:rw", top.path().join("RPMS").display()))
            .arg(image).args(["/bin/sh", "-ec", "dnf -y install rpm-build desktop-file-utils openssl-libs gtk4; rpmbuild -bb --define '_topdir /build' --define 'dist .fc44' /build/SPECS/axia-remotedid.spec"]).status()?
    } else {
        Command::new(builder)
            .args(["-bb", "--define"])
            .arg(format!("_topdir {}", top.path().display()))
            .args(["--define", "dist .fc44"])
            .arg(&spec)
            .status()
            .context("Não foi possível executar rpmbuild")?
    };
    ensure!(
        status.success(),
        "rpmbuild falhou; nenhum RPM foi publicado"
    );
    let packages = fs::read_dir(top.path().join("RPMS/x86_64"))?
        .filter_map(|entry| entry.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("axia-remotedid-") && n.ends_with(".x86_64.rpm"))
        })
        .collect::<Vec<_>>();
    ensure!(
        packages.len() == 1,
        "Esperado exatamente um RPM de execução"
    );
    ensure!(
        fs::symlink_metadata(&packages[0])?.is_file(),
        "RPM gerado precisa ser arquivo regular"
    );
    let output = destination.join(packages[0].file_name().context("Nome do RPM ausente")?);
    let mut temp = temporary(&output)?;
    std::io::copy(&mut fs::File::open(&packages[0])?, temp.as_file_mut())?;
    commit(temp, &output)?;
    checksum(&output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use std::io::Read;

    fn project() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
    }

    fn artifacts(target: &Path) -> Result<()> {
        let mut elf = vec![0; 64];
        elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
        elf[16] = 3;
        elf[18] = 62;
        for name in BINARIES {
            fs::write(target.join(name), &elf)?;
        }
        fs::write(target.join("wallet.json"), "must never be packed")?;
        Ok(())
    }

    #[test]
    fn runtime_is_reproducible_and_manifest_matches_extracted_hardlink() -> Result<()> {
        let target = tempfile::tempdir()?;
        artifacts(target.path())?;
        let output = tempfile::tempdir()?;
        let a = output.path().join("a.tar.gz");
        let b = output.path().join("b.tar.gz");
        linux(&project(), target.path(), &a)?;
        linux(&project(), target.path(), &b)?;
        assert_eq!(fs::read(&a)?, fs::read(&b)?);
        let extracted = output.path().join("extracted");
        tar::Archive::new(GzDecoder::new(fs::File::open(&a)?)).unpack(&extracted)?;
        let directory = extracted.join("axia-remotedid");
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("manifest.json"))?)?;
        for (name, expected) in manifest["files"].as_object().unwrap() {
            assert_eq!(
                publication::digest(&fs::read(directory.join(name))?),
                expected.as_str().unwrap()
            );
        }
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            fs::metadata(directory.join("desktopid-client"))?.ino(),
            fs::metadata(directory.join("axia-remotedid"))?.ino()
        );
        assert!(!directory.join("wallet.json").exists());
        assert!(!manifest["installs_browsers"].as_bool().unwrap());
        assert!(!manifest["requires_interpreter"].as_bool().unwrap());
        assert_eq!(
            fs::metadata(directory.join("desktopid-client"))?
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        Ok(())
    }

    #[test]
    fn source_zip_is_reproducible_and_rejects_secrets_before_touching_output() -> Result<()> {
        let root = tempfile::tempdir()?;
        let output = tempfile::tempdir()?;
        fs::write(root.path().join("source.rs"), "// source")?;
        fs::write(root.path().join("build.sh"), "#!/bin/sh\n")?;
        let a = output.path().join("a.zip");
        let b = output.path().join("b.zip");
        sources(root.path(), &a)?;
        sources(root.path(), &b)?;
        let original = fs::read(&a)?;
        assert_eq!(original, fs::read(&b)?);
        let mut archive = zip::ZipArchive::new(fs::File::open(&a)?)?;
        let mut data = String::new();
        archive
            .by_name("desktopid-linux/source.rs")?
            .read_to_string(&mut data)?;
        assert_eq!(data, "// source");
        assert_eq!(
            archive
                .by_name("desktopid-linux/build.sh")?
                .unix_mode()
                .unwrap()
                & 0o777,
            0o755
        );
        fs::write(root.path().join("wallet.json"), "{}")?;
        assert!(sources(root.path(), &a).is_err());
        assert_eq!(original, fs::read(&a)?);
        Ok(())
    }

    #[test]
    fn invalid_elf_is_rejected_before_overwriting_existing_archive() -> Result<()> {
        let target = tempfile::tempdir()?;
        artifacts(target.path())?;
        fs::write(target.path().join("desktopid-client"), "not ELF")?;
        let output = target.path().join("existing.tar.gz");
        fs::write(&output, "preserve")?;
        assert!(linux(&project(), target.path(), &output).is_err());
        assert_eq!(fs::read(&output)?, b"preserve");
        Ok(())
    }

    #[test]
    fn failed_rpm_build_preserves_previous_output_and_cannot_reuse_stale_rpm() -> Result<()> {
        let target = tempfile::tempdir()?;
        artifacts(target.path())?;
        let output = tempfile::tempdir()?;
        let package = output.path().join("axia-remotedid-old.x86_64.rpm");
        fs::write(&package, "preserve previous release")?;
        let stale = output.path().join("rpm-build/stale/RPMS/x86_64");
        fs::create_dir_all(&stale)?;
        fs::write(
            stale.join("axia-remotedid-stale.x86_64.rpm"),
            "never publish",
        )?;
        assert!(
            rpm(
                &project(),
                target.path(),
                output.path(),
                Path::new("/bin/false"),
                false,
                "unused"
            )
            .is_err()
        );
        assert_eq!(fs::read(&package)?, b"preserve previous release");
        assert!(
            !output
                .path()
                .join("axia-remotedid-stale.x86_64.rpm")
                .exists()
        );
        Ok(())
    }
}
