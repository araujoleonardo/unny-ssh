use crate::ipc::{AppError, Result};
use atomicwrites::{AllowOverwrite, AtomicFile};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub struct Storage {
    dir: PathBuf,
    _lock: File,
    #[cfg(test)]
    pub(crate) fail_writes: std::sync::atomic::AtomicBool,
}
fn restrict(path: &Path, directory: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if directory { 0o700 } else { 0o600 }),
        )
        .map_err(|_| AppError::io())?;
    }
    #[cfg(windows)]
    {
        use std::process::{Command, Stdio};
        let script = "$ErrorActionPreference='Stop'; $p=$env:UNNY_ACL_PATH; $id=[System.Security.Principal.WindowsIdentity]::GetCurrent().User; $acl=Get-Acl -LiteralPath $p; $acl.SetAccessRuleProtection($true,$false); foreach($r in @($acl.Access)){[void]$acl.RemoveAccessRuleAll($r)}; $rule=New-Object System.Security.AccessControl.FileSystemAccessRule($id,'FullControl','ContainerInherit,ObjectInherit','None','Allow'); if(-not (Test-Path -LiteralPath $p -PathType Container)){$rule=New-Object System.Security.AccessControl.FileSystemAccessRule($id,'FullControl','Allow')}; $acl.AddAccessRule($rule); Set-Acl -LiteralPath $p -AclObject $acl";
        let status = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            // A PowerShell 7 parent can pass incompatible modules to Windows
            // PowerShell 5.1 through an intermediate process such as this app.
            .env_remove("PSModulePath")
            .env("UNNY_ACL_PATH", path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| AppError::io())?;
        if !status.success() {
            return Err(AppError::io());
        }
        let _ = directory;
    }
    Ok(())
}
impl Storage {
    pub fn open(dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&dir).map_err(|_| AppError::io())?;
        restrict(&dir, true)?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options
            .open(dir.join("writer.lock"))
            .map_err(|_| AppError::io())?;
        restrict(&dir.join("writer.lock"), false)?;
        lock.try_lock_exclusive()
            .map_err(|_| AppError::new("BUSY", "O cofre já está aberto em outra instância."))?;
        Ok(Self {
            dir,
            _lock: lock,
            #[cfg(test)]
            fail_writes: std::sync::atomic::AtomicBool::new(false),
        })
    }
    pub(crate) fn write_blob(&self, id: uuid::Uuid, bytes: &[u8]) -> Result<()> {
        if bytes.len() > 40 * 1024 * 1024 {
            return Err(AppError::invalid());
        }
        let dir = self.dir.join("drafts");
        fs::create_dir_all(&dir).map_err(|_| AppError::io())?;
        restrict(&dir, true)?;
        AtomicFile::new(dir.join(id.to_string()), atomicwrites::DisallowOverwrite)
            .write(|file| -> std::io::Result<()> {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    file.set_permissions(fs::Permissions::from_mode(0o600))?;
                }
                file.write_all(bytes)?;
                file.sync_all()
            })
            .map_err(|_| AppError::io())?;
        Ok(())
    }
    pub(crate) fn read_blob(&self, id: uuid::Uuid) -> Result<Vec<u8>> {
        let file =
            File::open(self.dir.join("drafts").join(id.to_string())).map_err(|_| AppError::io())?;
        let mut bytes = Vec::new();
        file.take(40 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| AppError::io())?;
        if bytes.len() > 40 * 1024 * 1024 {
            return Err(AppError::invalid());
        }
        Ok(bytes)
    }
    pub(crate) fn cleanup_blobs(&self, keep: &[uuid::Uuid]) {
        let Ok(entries) = fs::read_dir(self.dir.join("drafts")) else {
            return;
        };
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_file()) {
                if let Ok(id) = uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()) {
                    if !keep.contains(&id) {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }
    }
    pub(crate) fn remove_blob(&self, id: uuid::Uuid) {
        let _ = fs::remove_file(self.dir.join("drafts").join(id.to_string()));
    }
    pub fn audit(&self, operation: &'static str, result: &'static str, duration_ms: u128) {
        crate::adapters::audit::record(&self.dir, operation, result, duration_ms);
    }
    pub fn exists(&self) -> bool {
        self.dir.join("vault.json").exists()
    }
    pub fn read(&self) -> Result<Vec<u8>> {
        let file = File::open(self.dir.join("vault.json")).map_err(|_| AppError::io())?;
        if file.metadata().map_err(|_| AppError::io())?.len() > 16 * 1024 * 1024 {
            return Err(AppError::invalid());
        }
        let mut bytes = Vec::new();
        file.take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| AppError::io())?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(AppError::invalid());
        }
        Ok(bytes)
    }
    pub fn publish(&self, bytes: &[u8]) -> Result<()> {
        #[cfg(test)]
        if self.fail_writes.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(AppError::io());
        }
        self.publish_with(bytes, |_| Ok(()))
    }
    fn publish_with(
        &self,
        bytes: &[u8],
        mut stage: impl FnMut(&str) -> std::io::Result<()>,
    ) -> Result<()> {
        AtomicFile::new(self.dir.join("vault.json"), AllowOverwrite)
            .write(|file| -> std::io::Result<()> {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    file.set_permissions(fs::Permissions::from_mode(0o600))?;
                }
                stage("before_write")?;
                let middle = bytes.len() / 2;
                file.write_all(&bytes[..middle])?;
                stage("partial_write")?;
                file.write_all(&bytes[middle..])?;
                file.sync_all()?;
                stage("before_rename")
            })
            .map_err(|_| AppError::io())?;

        stage("after_commit").map_err(|_| AppError::io())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn windows_acl_ignores_incompatible_powershell_modules() {
        let temp = tempfile::tempdir().unwrap();
        let modules = temp.path().join("modules");
        let security = modules.join("Microsoft.PowerShell.Security");
        fs::create_dir_all(&security).unwrap();
        fs::write(
            security.join("Microsoft.PowerShell.Security.psd1"),
            "@{RootModule='missing.psm1';ModuleVersion='1.0.0';CmdletsToExport=@('Get-Acl','Set-Acl')}",
        )
        .unwrap();
        // Isolate the poisoned environment in a child; other tests run in parallel.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "adapters::storage::tests::crash_worker",
            ])
            .env("PSModulePath", modules)
            .env("UNNY_TEST_CRASH_DIR", temp.path().join("vault"))
            .env("UNNY_TEST_CRASH_STAGE", "before_write")
            .output()
            .unwrap();
        // The worker reaches publication only after restricting the directory
        // and lock file. A module-loading failure instead exits with a test panic.
        assert_eq!(output.status.code(), Some(73));
    }
    #[test]
    fn write_failures_keep_previous_generation() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path().join("vault")).unwrap();
        storage.publish(b"old encrypted generation").unwrap();
        for fail in ["before_write", "partial_write", "before_rename"] {
            let result = storage.publish_with(b"new encrypted generation", |stage| {
                if stage == fail {
                    Err(std::io::Error::other("injected disk failure"))
                } else {
                    Ok(())
                }
            });
            assert!(result.is_err());
            assert_eq!(storage.read().unwrap(), b"old encrypted generation");
        }
    }
    #[cfg(unix)]
    #[test]
    fn rename_failure_keeps_previous_generation() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("vault");
        let storage = Storage::open(path.clone()).unwrap();
        storage.publish(b"old encrypted generation").unwrap();
        let result = storage.publish_with(b"new encrypted generation", |stage| {
            if stage == "before_rename" {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o500))?;
            }
            Ok(())
        });
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert_eq!(storage.read().unwrap(), b"old encrypted generation");
    }
    #[test]
    fn process_interruption_recovers_complete_generation() {
        for stage in [
            "before_write",
            "partial_write",
            "before_rename",
            "after_commit",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("vault");
            let storage = Storage::open(path.clone()).unwrap();
            storage.publish(b"old encrypted generation").unwrap();
            drop(storage);
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "adapters::storage::tests::crash_worker",
                ])
                .env("UNNY_TEST_CRASH_DIR", &path)
                .env("UNNY_TEST_CRASH_STAGE", stage)
                .output()
                .unwrap()
                .status;
            assert_eq!(status.code(), Some(73));
            let recovered = Storage::open(path).unwrap();
            assert_eq!(
                recovered.read().unwrap(),
                if stage == "after_commit" {
                    b"new encrypted generation"
                } else {
                    b"old encrypted generation"
                }
            );
        }
    }
    #[test]
    #[ignore]
    fn crash_worker() {
        let path = std::env::var_os("UNNY_TEST_CRASH_DIR").expect("only invoked by parent fixture");
        let storage = Storage::open(path.into()).unwrap();
        let fail = std::env::var("UNNY_TEST_CRASH_STAGE").unwrap();
        storage
            .publish_with(b"new encrypted generation", |stage| {
                if stage == fail {
                    std::process::exit(73);
                }
                Ok(())
            })
            .unwrap();
    }
}
