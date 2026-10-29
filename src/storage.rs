use crate::{config::SecurityMode, security};
use anyhow::{Result, ensure};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub struct Repository {
    root: PathBuf,
    mode: SecurityMode,
    _lock: File,
}
/// Return whether a string is a lowercase hexadecimal SHA-256 identity.
pub fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Repository {
    /// Create or open a repository while enforcing its security mode.
    pub fn open(root: &Path, mode: SecurityMode) -> Result<Self> {
        security::private_dir(root)?;
        let lock_path = root.join(".lock");
        crate::discovery::check_path(&lock_path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(lock_path)?;
        lock.try_lock()
            .map_err(|_| anyhow::anyhow!("repository is in use by another process"))?;
        let mode_path = root.join("security.json");
        if mode_path.exists() {
            security::validate_file(&mode_path, SecurityMode::Standard)?;
            let stored: SecurityMode = serde_json::from_slice(&fs::read(&mode_path)?)?;
            ensure!(
                stored == mode,
                "repository security mode cannot be changed automatically"
            );
        } else {
            let mut file = tempfile::NamedTempFile::new_in(root)?;
            serde_json::to_writer(&mut file, &mode)?;
            file.flush()?;
            file.as_file().sync_all()?;
            file.persist_noclobber(&mode_path)?;
        }
        for name in ["objects", "manifests", "views", "reports", ".staging"] {
            security::private_dir(&root.join(name))?;
        }
        // The exclusive repository lock proves no other writer owns these staging directories.
        for entry in fs::read_dir(root.join(".staging"))? {
            let entry = entry?;
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("run-")
                || name.to_string_lossy().starts_with("view-")
            {
                ensure!(entry.file_type()?.is_dir(), "unexpected staging entry");
                crate::discovery::check_path(&entry.path())?;
                fs::remove_dir_all(entry.path())?;
            }
        }
        Ok(Self {
            root: root.canonicalize()?,
            mode,
            _lock: lock,
        })
    }
    /// Open an existing repository without changing its recorded security mode.
    pub fn existing(root: &Path) -> Result<Self> {
        crate::discovery::check_path(root)?;
        let mode_path = root.join("security.json");
        security::validate_file(&mode_path, SecurityMode::Standard)?;
        let mode = serde_json::from_slice(&fs::read(mode_path)?)?;
        let lock_path = root.join(".lock");
        security::validate_file(&lock_path, SecurityMode::Standard)?;
        let lock = File::open(lock_path)?;
        lock.try_lock()
            .map_err(|_| anyhow::anyhow!("repository is in use by another process"))?;
        for name in ["objects", "manifests", "views", "reports", ".staging"] {
            crate::discovery::check_path(&root.join(name))?;
            ensure!(root.join(name).is_dir(), "incomplete repository");
        }
        Ok(Self {
            root: root.canonicalize()?,
            mode,
            _lock: lock,
        })
    }
    /// Return the canonical repository root.
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// Return the repository's fixed security mode.
    pub fn mode(&self) -> SecurityMode {
        self.mode
    }
    /// Return the private staging directory path.
    pub fn staging(&self) -> PathBuf {
        self.root.join(".staging")
    }
    /// Resolve a validated SHA-256 identity to canonical object storage.
    pub fn object(&self, hash: &str) -> Result<PathBuf> {
        ensure!(valid_hash(hash), "invalid canonical hash");
        Ok(self.root.join("objects").join(&hash[..2]).join(hash))
    }
    /// Seal and atomically publish one staged canonical object without overwriting.
    pub fn commit(&self, file: tempfile::NamedTempFile, hash: &str, size: u64) -> Result<()> {
        let target = self.object(hash)?;
        security::private_dir(target.parent().expect("object parent"))?;
        security::seal(file.as_file(), self.mode)?;
        file.as_file().sync_all()?;
        // Closing the writer before publication avoids retaining a writable sealed handle.
        let path = file.into_temp_path();
        match path.persist_noclobber(&target) {
            Ok(_) => (),
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
        security::validate_file(&target, self.mode)?;
        ensure!(
            fs::metadata(&target)?.len() == size,
            "existing canonical object has unexpected size"
        );
        File::open(target.parent().expect("object parent"))?.sync_all()?;
        Ok(())
    }
}
