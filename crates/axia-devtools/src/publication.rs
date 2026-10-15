//! Limited publication guard; reports paths and reasons, never matching values.
use anyhow::{Context, Result, ensure};
use regex::bytes::Regex;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

pub const LAB_FIXTURE: &str = "crates/desktopid-client/tests/fixtures/python-vectors.json";
const LAB_SHA256: &str = "4ad80be20112e24c79abefe0bb80fcd0354bf00ede3e64978d544a4ad71a2770";
const GENERATED: &[&str] = &[
    ".git",
    "target",
    "dist",
    "test-results",
    "__pycache__",
    ".venv",
    ".local-research",
];
const PRIVATE_PARTS: &[&str] = &[
    "desktopid-linux-state",
    "signing-tests",
    "pkcs11-tests",
    "analysis",
    "tools",
    "target",
    "dist",
    "test-results",
    ".local-research",
    ".git",
];
const FORBIDDEN_SUFFIXES: &[&str] = &[
    "exe", "pkg", "dll", "dylib", "so", "pem", "key", "pfx", "p12", "zip", "gz", "rpm",
];

#[derive(Debug, PartialEq, Eq)]
pub struct Issue {
    pub name: PathBuf,
    pub reason: &'static str,
}

pub type SourceFiles = Vec<(PathBuf, Vec<u8>)>;

pub fn digest(data: &[u8]) -> String {
    openssl::sha::sha256(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn candidates(root: &Path) -> Result<Vec<PathBuf>> {
    if let Ok(top) = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        && top.status.success()
        && Path::new(std::str::from_utf8(&top.stdout)?.trim()).canonicalize()? == root
    {
        let result = Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "-z",
            ])
            .output()?;
        ensure!(
            result.status.success(),
            "Falha ao enumerar candidatos no índice Git"
        );
        let names = result
            .stdout
            .split(|b| *b == 0)
            .filter(|b| !b.is_empty())
            .map(|b| {
                Ok(PathBuf::from(
                    std::str::from_utf8(b).context("Nome de arquivo sem UTF-8")?,
                ))
            })
            .collect::<Result<BTreeSet<_>>>()?;
        return Ok(names.into_iter().collect());
    }
    let mut names = BTreeSet::new();
    walk(root, Path::new(""), &mut names)?;
    Ok(names.into_iter().collect())
}

fn walk(root: &Path, relative: &Path, names: &mut BTreeSet<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let name = relative.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            if !GENERATED
                .iter()
                .any(|excluded| entry.file_name() == *excluded)
            {
                walk(root, &name, names)?;
            }
        } else {
            names.insert(name);
        }
    }
    Ok(())
}

pub fn report(count: usize, issues: &[Issue]) {
    for issue in issues {
        println!(
            "{}: {}; content omitted",
            issue.name.display(),
            issue.reason
        );
    }
    println!(
        "Publication check: {count} source files; {} issue(s).",
        issues.len()
    );
}

/// Take a single in-memory snapshot: inspect exactly the bytes that are archived.
pub fn snapshot(root: &Path, names: &[PathBuf]) -> Result<(SourceFiles, Vec<Issue>)> {
    let private_pem = Regex::new(r"-----BEGIN (?:RSA |EC |ENCRYPTED |OPENSSH )?PRIVATE KEY-----")?;
    let jwt = Regex::new(
        r"(?-u:\b)eyJ[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}(?-u:\b)",
    )?;
    let email = Regex::new(r"(?i)[A-Za-z0-9._%+-]+@(?:gmail|hotmail|outlook|yahoo)\.[A-Za-z.]+")?;
    let mut files = Vec::new();
    let mut issues = Vec::new();
    for name in names {
        let reject = |issues: &mut Vec<Issue>, reason| {
            issues.push(Issue {
                name: name.clone(),
                reason,
            })
        };
        if !safe_relative(name) {
            reject(&mut issues, "symbolic link or file outside project");
            continue;
        }
        let mut path = root.to_path_buf();
        let mut missing = false;
        let mut unsafe_path = false;
        for component in name.components() {
            path.push(component);
            match fs::symlink_metadata(&path) {
                Ok(metadata) => {
                    if metadata.is_symlink() {
                        unsafe_path = true;
                        break;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    missing = true;
                    break;
                }
                Err(error) => return Err(error).context("Falha ao verificar caminho candidato"),
            }
        }
        if unsafe_path {
            reject(&mut issues, "symbolic link or file outside project");
            continue;
        }
        // Deleted tracked files are absent from the next source snapshot.
        if missing {
            continue;
        }
        if !fs::symlink_metadata(&path)?.is_file() {
            reject(&mut issues, "non-regular source file");
            continue;
        }
        let parts: Vec<_> = name
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect();
        let basename = name.file_name().unwrap_or_default().to_string_lossy();
        let extension = name
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        if FORBIDDEN_SUFFIXES.contains(&extension.as_str())
            || ["wallet.json", "config.json", ".env"].contains(&basename.as_ref())
            || basename.starts_with(".env.")
            || basename.starts_with("registration")
            || parts.iter().any(|p| PRIVATE_PARTS.contains(&p.as_ref()))
            || parts.iter().any(|p| p == "desktopid-linux") && parts.iter().any(|p| p == ".config")
        {
            reject(
                &mut issues,
                "private state, binary or generated investigation artifact",
            );
            continue;
        }
        let data = fs::read(&path).context("Falha ao ler candidato à publicação")?;
        if private_pem.is_match(&data) {
            let normalized = normalize_crlf(&data);
            if name != Path::new(LAB_FIXTURE) || digest(&normalized) != LAB_SHA256 {
                reject(&mut issues, "unreviewed private key");
            }
        }
        if jwt.is_match(&data) {
            reject(&mut issues, "possible authentication token");
        }
        if email.is_match(&data) {
            reject(&mut issues, "personal email address");
        }
        files.push((name.clone(), data));
    }
    Ok((files, issues))
}

fn normalize_crlf(data: &[u8]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .filter_map(|(i, b)| {
            if *b == b'\r' && data.get(i + 1) == Some(&b'\n') {
                None
            } else {
                Some(*b)
            }
        })
        .collect()
}

pub fn inspect(root: &Path, names: &[PathBuf]) -> Result<Vec<Issue>> {
    Ok(snapshot(root, names)?.1)
}

pub fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn tracked_ignored_state_cannot_escape_git_guard() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().canonicalize()?;
        ensure!(
            Command::new("git")
                .args(["init", "-q"])
                .arg(&root)
                .status()?
                .success()
        );
        fs::write(root.join(".gitignore"), "wallet.json\n")?;
        fs::write(root.join("wallet.json"), "{}")?;
        ensure!(
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["add", "-f", "wallet.json"])
                .status()?
                .success()
        );
        let names = candidates(&root)?;
        assert!(names.contains(&PathBuf::from("wallet.json")));
        assert!(!inspect(&root, &names)?.is_empty());
        Ok(())
    }

    #[test]
    fn only_exact_reviewed_fixture_and_crlf_equivalent_are_allowed() -> Result<()> {
        let root = tempfile::tempdir()?;
        let name = PathBuf::from(LAB_FIXTURE);
        let fixture = root.path().join(&name);
        fs::create_dir_all(fixture.parent().unwrap())?;
        let data = include_bytes!("../../desktopid-client/tests/fixtures/python-vectors.json");
        fs::write(&fixture, data)?;
        assert!(inspect(root.path(), std::slice::from_ref(&name))?.is_empty());
        fs::write(
            &fixture,
            String::from_utf8_lossy(data).replace('\n', "\r\n"),
        )?;
        assert!(inspect(root.path(), std::slice::from_ref(&name))?.is_empty());
        fs::copy(&fixture, root.path().join("copy.json"))?;
        assert!(!inspect(root.path(), &["copy.json".into()])?.is_empty());
        fs::write(&fixture, [data.as_slice(), b"\n"].concat())?;
        assert!(!inspect(root.path(), &[name])?.is_empty());
        Ok(())
    }

    #[test]
    fn tokens_and_emails_are_redacted() -> Result<()> {
        let root = tempfile::tempdir()?;
        let token = format!(
            "eyJ{}.{}.{}",
            "a".repeat(24),
            "b".repeat(24),
            "c".repeat(24)
        );
        let email = ["synthetic", "@", "gmail", ".com"].concat();
        fs::write(root.path().join("oops.txt"), format!("{token}\n{email}"))?;
        let issues = inspect(root.path(), &["oops.txt".into()])?;
        assert_eq!(issues.len(), 2);
        assert!(!format!("{issues:?}").contains(&token));
        assert!(!format!("{issues:?}").contains(&email));
        Ok(())
    }

    #[test]
    fn rejects_symlinks_parent_traversal_and_non_regular_files() -> Result<()> {
        let root = tempfile::tempdir()?;
        let other = tempfile::tempdir()?;
        fs::write(other.path().join("plain.txt"), "safe")?;
        symlink(other.path(), root.path().join("linked"))?;
        symlink("missing", root.path().join("broken"))?;
        fs::create_dir(root.path().join("directory"))?;
        let issues = inspect(
            root.path(),
            &[
                "linked/plain.txt".into(),
                "broken".into(),
                "../outside".into(),
                "directory".into(),
            ],
        )?;
        assert_eq!(issues.len(), 4);
        Ok(())
    }

    #[test]
    fn fallback_excludes_generated_directories_but_reports_links() -> Result<()> {
        let root = tempfile::tempdir()?;
        fs::create_dir(root.path().join("dist"))?;
        fs::write(root.path().join("dist/secret.key"), "secret")?;
        fs::write(root.path().join("source.rs"), "// source")?;
        symlink("dist", root.path().join("link"))?;
        assert_eq!(
            candidates(root.path())?,
            vec![PathBuf::from("link"), PathBuf::from("source.rs")]
        );
        Ok(())
    }
}
