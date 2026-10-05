//! Mandatory Linux filesystem boundary for the native child, never the broker.
//! ABI 3 is the minimum: rename/link and truncate must also be mediated.
//! This policy makes no claim about network, inherited stdio, or model tools.
use crate::error::RuntimeError;
use std::path::Path;
use tokio::process::Command;

pub(crate) fn attach(
    command: &mut Command,
    binary: &Path,
    home: &Path,
    workdir: &Path,
) -> Result<(), RuntimeError> {
    #[cfg(target_os = "linux")]
    {
        linux::attach(command, binary, home, workdir)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (command, binary, home, workdir);
        Err(RuntimeError::NativeFilesystemIsolation)
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        ffi::CString,
        io,
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::ffi::OsStrExt,
        },
    };

    const EXECUTE: u64 = 1;
    const WRITE_FILE: u64 = 1 << 1;
    const READ_FILE: u64 = 1 << 2;
    const READ_DIR: u64 = 1 << 3;
    // All ABI 3 rights are handled, even those we never grant (devices/fifo/socket).
    const HANDLED: u64 = (1 << 15) - 1;
    const HOME_ACCESS: u64 = READ_FILE
        | READ_DIR
        | WRITE_FILE
        | (1 << 4)
        | (1 << 5)
        | (1 << 7)
        | (1 << 8)
        | (1 << 12)
        | (1 << 13)
        | (1 << 14);

    #[repr(C)]
    struct Ruleset {
        handled_access_fs: u64,
    }
    // Linux UAPI explicitly packs this structure to 12 bytes.
    #[repr(C, packed)]
    struct PathBeneath {
        allowed_access: u64,
        parent_fd: i32,
    }

    fn fd(result: libc::c_long) -> Result<OwnedFd, RuntimeError> {
        if result < 0 {
            return Err(RuntimeError::NativeFilesystemIsolation);
        }
        // SAFETY: successful syscalls here return a newly owned descriptor.
        Ok(unsafe { OwnedFd::from_raw_fd(result as i32) })
    }

    fn require_abi(abi: libc::c_long) -> Result<(), RuntimeError> {
        if abi < 3 {
            Err(RuntimeError::NativeFilesystemIsolation)
        } else {
            Ok(())
        }
    }

    fn add(ruleset: &OwnedFd, path: &Path, access: u64) -> Result<(), RuntimeError> {
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| RuntimeError::NativeFilesystemIsolation)?;
        // SAFETY: CString is terminated; only O_PATH/CLOEXEC, not a readable fd.
        let parent = fd(unsafe { libc::open(path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) } as _)?;
        let rule = PathBeneath {
            allowed_access: access,
            parent_fd: parent.as_raw_fd(),
        };
        // SAFETY: UAPI packed layout, valid ruleset and parent descriptors.
        let result = unsafe {
            libc::syscall(
                libc::SYS_landlock_add_rule,
                ruleset.as_raw_fd(),
                1u32,
                &rule as *const PathBeneath,
                0u32,
            )
        };
        if result < 0 {
            return Err(RuntimeError::NativeFilesystemIsolation);
        }
        Ok(())
    }

    pub(super) fn attach(
        command: &mut Command,
        binary: &Path,
        home: &Path,
        workdir: &Path,
    ) -> Result<(), RuntimeError> {
        // No weakening on unsupported kernels. Prepare everything before fork.
        // SAFETY: version query takes a null attr, zero size and VERSION flag.
        require_abi(unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<Ruleset>(),
                0usize,
                1u32,
            )
        })?;
        let attr = Ruleset {
            handled_access_fs: HANDLED,
        };
        // SAFETY: ABI >=3 accepts the filesystem-only eight-byte UAPI struct.
        let ruleset = fd(unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                &attr as *const Ruleset,
                std::mem::size_of::<Ruleset>(),
                0u32,
            )
        })?;
        add(&ruleset, binary, READ_FILE | EXECUTE)?;
        let catalog = crate::native_model_policy::path_for(binary)?;
        if catalog.exists() {
            let catalog = crate::native_model_policy::validated_path(binary)?;
            add(&ruleset, &catalog, READ_FILE)?;
        }
        add(&ruleset, home, HOME_ACCESS)?;
        add(&ruleset, workdir, READ_FILE | READ_DIR)?;
        for path in ["/etc/ssl/certs", "/usr/share/ca-certificates"] {
            add(&ruleset, Path::new(path), READ_FILE | READ_DIR)?;
        }
        for path in [
            "/etc/resolv.conf",
            "/etc/hosts",
            "/etc/nsswitch.conf",
            "/dev/urandom",
        ] {
            add(&ruleset, Path::new(path), READ_FILE)?;
        }
        for path in ["/etc/host.conf", "/etc/gai.conf"] {
            if Path::new(path).exists() {
                add(&ruleset, Path::new(path), READ_FILE)?;
            }
        }
        add(&ruleset, Path::new("/dev/null"), READ_FILE | WRITE_FILE)?;
        // Fixture-only interpreters/libs. These paths do NOT exist in release policy.
        #[cfg(test)]
        {
            add(&ruleset, Path::new("/bin/bash"), READ_FILE | EXECUTE)?;
            for path in ["/lib", "/lib64", "/usr/lib"] {
                add(&ruleset, Path::new(path), READ_FILE | READ_DIR)?;
            }
            add(
                &ruleset,
                Path::new("/lib64/ld-linux-x86-64.so.2"),
                READ_FILE | EXECUTE,
            )?;
        }
        // SAFETY: only prctl/syscall and OS error retrieval occur after fork.
        // The descriptor is CLOEXEC and remains owned by the parent Command;
        // successful exec closes its child copy, without leaking broker handles.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                    || libc::syscall(libc::SYS_landlock_restrict_self, ruleset.as_raw_fd(), 0u32)
                        != 0
                {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn abi_below_three_is_never_downgraded() {
            for abi in [-1, 0, 1, 2] {
                assert_eq!(
                    require_abi(abi),
                    Err(RuntimeError::NativeFilesystemIsolation)
                );
            }
            assert!(require_abi(3).is_ok());
            assert_eq!(std::mem::size_of::<PathBeneath>(), 12);
        }

        #[tokio::test]
        async fn native_catalog_is_readable_but_not_writable_and_sibling_files_stay_denied() {
            use std::os::unix::fs::PermissionsExt;
            let dir = tempfile::tempdir().unwrap();
            let binary = dir.path().join("native-fixture");
            std::fs::copy("/bin/bash", &binary).unwrap();
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
            let catalog = crate::native_model_policy::provision_fixture(&binary);
            let sibling = dir.path().join("broker-secret-fixture");
            std::fs::write(&sibling, "fixture-only").unwrap();
            let home = dir.path().join("home");
            let work = dir.path().join("work");
            std::fs::create_dir(&home).unwrap();
            std::fs::create_dir(&work).unwrap();
            let mut command = Command::new(&binary);
            attach(&mut command, &binary, &home, &work).unwrap();
            let result = command.env_clear().arg("-c")
                .arg("mapfile -t lines < \"$1\" && [[ ${lines[0]} == '{'* ]] || exit 1; if printf bad > \"$1\"; then exit 2; fi; if mapfile -t values < \"$2\"; then exit 3; fi; exit 0")
                .arg("probe").arg(&catalog).arg(&sibling).output().await.unwrap();
            assert!(
                result.status.success(),
                "fixture status {:?}; stderr {}",
                result.status,
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(crate::native_model_policy::validated_path(&binary).is_ok());
            assert_eq!(std::fs::read_to_string(&sibling).unwrap(), "fixture-only");
        }

        #[tokio::test]
        async fn subprocess_can_use_own_home_but_cannot_read_broker_or_execute_foreign_binary() {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().join("home");
            let work = dir.path().join("work");
            std::fs::create_dir(&home).unwrap();
            std::fs::create_dir(&work).unwrap();
            let key = dir.path().join("broker-key");
            std::fs::write(&key, "fixture-not-secret").unwrap();
            let mut command = Command::new("/bin/bash");
            attach(&mut command, Path::new("/bin/bash"), &home, &work).unwrap();
            command
                .env_clear()
                .arg("-c")
                .arg(
                    "printf fixture > \"$1/auth\" && read -r value < \"$1/auth\"; \
                 [[ $value == fixture ]] || exit 1; \
                 if mapfile -t secret < \"$2\"; then exit 2; fi; \
                 if mapfile -t secret < /etc/passwd; then exit 3; fi; \
                 if mapfile -t secret < /proc/self/mem; then exit 4; fi; \
                 if /usr/bin/true; then exit 5; fi; \
                 if printf bad > \"$3/foreign\"; then exit 6; fi; exit 0",
                )
                .arg("probe")
                .arg(&home)
                .arg(&key)
                .arg(&work);
            let output = command.output().await.unwrap();
            assert!(
                output.status.success(),
                "filesystem subprocess probe failed: {:?}",
                output.status
            );
            assert!(!work.join("foreign").exists());
            assert_eq!(std::fs::read_to_string(key).unwrap(), "fixture-not-secret");
            // Parent's domain is untouched by enforcement in the child pre_exec.
            assert!(std::fs::read_to_string("/etc/passwd").is_ok());
        }
    }
}
