use super::*;
use std::{
    os::unix::{fs::symlink, process::CommandExt},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn every_discovered_alias_and_internal_executable_blocks_configuration() {
    for browser in BROWSERS {
        for name in browser.commands.iter().chain(browser.executables) {
            let root = tempfile::tempdir().unwrap();
            let process = root.path().join("123");
            fs::create_dir(&process).unwrap();
            fs::write(
                process.join("cmdline"),
                format!("/usr/bin/{name}\0--flag\0"),
            )
            .unwrap();
            assert!(
                running_at(root.path(), unsafe { libc::geteuid() }).unwrap(),
                "{name}"
            );
            fs::write(process.join("cmdline"), b"unrelated-launcher\0").unwrap();
            symlink(format!("/opt/browser/{name}"), process.join("exe")).unwrap();
            assert!(
                running_at(root.path(), unsafe { libc::geteuid() }).unwrap(),
                "ELF {name}"
            );
        }
    }
}

#[test]
fn executable_detection_survives_empty_arguments_and_deleted_executable() {
    let root = tempfile::tempdir().unwrap();
    let process = root.path().join("123");
    fs::create_dir(&process).unwrap();
    symlink("/opt/browser/chrome (deleted)", process.join("exe")).unwrap();
    fs::write(process.join("cmdline"), b"").unwrap();
    assert!(running_at(root.path(), unsafe { libc::geteuid() }).unwrap());
    fs::remove_file(process.join("cmdline")).unwrap();
    assert!(running_at(root.path(), unsafe { libc::geteuid() }).unwrap());
}

#[test]
fn matching_is_exact_and_does_not_inspect_other_users_or_argument_values() {
    let root = tempfile::tempdir().unwrap();
    let process = root.path().join("123");
    fs::create_dir(&process).unwrap();
    for command in [
        b"sh\0/usr/bin/google-chrome-stable\0".as_slice(),
        b"chrome_crashpad_handler\0",
        b"google-chrome-stable-backup\0",
        b"not-chromium\0",
        b"\xffunknown\0",
        b"",
    ] {
        fs::write(process.join("cmdline"), command).unwrap();
        assert!(!running_at(root.path(), unsafe { libc::geteuid() }).unwrap());
    }
    fs::write(process.join("cmdline"), b"google-chrome-stable\0").unwrap();
    assert!(!running_at(root.path(), unsafe { libc::geteuid() }.wrapping_add(1)).unwrap());
}

#[test]
fn exiting_tasks_are_ignored_but_uninspectable_live_tasks_stop_configuration() {
    let root = tempfile::tempdir().unwrap();
    // A vanished PID must not be mistaken for a live unreadable process.
    symlink("missing-task", root.path().join("123")).unwrap();
    fs::create_dir(root.path().join("not-a-pid")).unwrap();
    assert!(!running_at(root.path(), unsafe { libc::geteuid() }).unwrap());
    let process = root.path().join("456");
    fs::create_dir_all(process.join("cmdline")).unwrap();
    assert!(running_at(root.path(), unsafe { libc::geteuid() }).is_err());
    assert!(
        running_at(&root.path().join("missing-proc"), unsafe {
            libc::geteuid()
        })
        .is_err()
    );
}

struct RunningChild(Child);
impl Drop for RunningChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn real_procfs_detects_stable_wrapper_alias_and_disguised_argv_via_executable() {
    let root = tempfile::tempdir().unwrap();
    let browser = root.path().join("chrome");
    fs::copy("/usr/bin/sleep", &browser).unwrap();
    for (binary, argv0) in [
        (Path::new("/usr/bin/sleep"), "/usr/bin/google-chrome-stable"),
        (browser.as_path(), "unrelated-launcher"),
    ] {
        let mut child = RunningChild(
            Command::new(binary)
                .arg0(argv0)
                .arg("30")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        // Limit the scan to this real PID: other tests or the desktop session
        // must not create a false positive for this regression.
        let proc = root.path().join(format!("proc-{}", child.0.id()));
        fs::create_dir(&proc).unwrap();
        symlink(
            format!("/proc/{}", child.0.id()),
            proc.join(child.0.id().to_string()),
        )
        .unwrap();
        // spawn() can return while procfs still exposes transient exec state.
        // Wait using an independent oracle, then assert production detection
        // once; retrying running_at() itself could conceal a detection defect.
        let task = Path::new("/proc").join(child.0.id().to_string());
        let executable = binary.canonicalize().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let ready = fs::read_link(task.join("exe")).is_ok_and(|path| path == executable)
                && fs::read(task.join("cmdline")).is_ok_and(|bytes| {
                    bytes.split(|byte| *byte == 0).next() == Some(argv0.as_bytes())
                });
            if ready {
                break;
            }
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "laboratory process exited before readiness"
            );
            assert!(
                Instant::now() < deadline,
                "laboratory process never reached expected exec state"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(running_at(&proc, unsafe { libc::geteuid() }).unwrap());
        drop(child);
        assert!(!running_at(&proc, unsafe { libc::geteuid() }).unwrap());
    }
}
