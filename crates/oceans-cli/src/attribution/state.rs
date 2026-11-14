//! The attribution state directory: `$XDG_STATE_HOME/oceans/attribution`, else
//! `~/.local/state/oceans/attribution` (`%LOCALAPPDATA%\oceans\attribution` on Windows).
//!
//! Everything in it is owner-only (directories `0700`, files `0600`):
//! `installation_id`, `counter`, `lock`, `sender.lock`, `dropped`, `ownership-cache.json`,
//! `queue/`, `unsequenced/`, `quarantine/` and `pr-sync/` (PR sync throttle timestamps).

use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::ownership::Ownership;
use crate::config::Env;

pub const QUEUE_DIR: &str = "queue";
pub const UNSEQUENCED_DIR: &str = "unsequenced";
pub const QUARANTINE_DIR: &str = "quarantine";
const PR_SYNC_DIR: &str = "pr-sync";
const INSTALLATION_ID_FILE: &str = "installation_id";
const COUNTER_FILE: &str = "counter";
const DROPPED_FILE: &str = "dropped";
const LOCK_FILE: &str = "lock";
const SENDER_LOCK_FILE: &str = "sender.lock";
const OWNERSHIP_CACHE_FILE: &str = "ownership-cache.json";
/// Temporary files carry this prefix so the sender can remove ones a crash left behind.
pub const TEMP_PREFIX: &str = ".tmp-";
/// Hooks wait at most this long for the queue lock before writing to `unsequenced/`.
pub const LOCK_TIMEOUT: Duration = Duration::from_secs(1);
/// The queue, unsequenced and quarantine folders together hold at most this much.
pub const QUEUE_CAP_BYTES: u64 = 512 * 1024 * 1024;
const STALE_TEMP_AGE: Duration = Duration::from_secs(60 * 60);
const QUARANTINE_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const MAX_CACHED_LOOKUPS: usize = 16;
const MAX_SMALL_FILE_BYTES: u64 = 1024 * 1024;

pub struct State {
    root: PathBuf,
    pub(crate) lock_timeout: Duration,
    pub(crate) queue_cap_bytes: u64,
}

/// An exclusive file lock, released when dropped.
pub struct LockGuard(File);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedOwnership {
    pub gateway_url: String,
    pub key_fingerprint: String,
    pub ownership_scope: String,
    pub owner_kind: String,
    pub owner_id: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct OwnershipCache {
    #[serde(default)]
    current: Option<CachedOwnership>,
    #[serde(default)]
    lookups: Vec<CachedOwnership>,
}

impl OwnershipCache {
    fn cache_lookup(&mut self, lookup: CachedOwnership) {
        self.lookups.retain(|cached| {
            cached.gateway_url != lookup.gateway_url
                || cached.key_fingerprint != lookup.key_fingerprint
        });
        self.lookups.insert(0, lookup);
        self.lookups.truncate(MAX_CACHED_LOOKUPS);
    }

    /// Compare-and-set: makes `lookup` current only while `current` is still `seen` (or is
    /// already `lookup`), then updates `seen` to match. Returns whether `current` changed.
    fn replace_current_if(
        &mut self,
        seen: &mut Option<CachedOwnership>,
        lookup: CachedOwnership,
    ) -> bool {
        if self.current.as_ref() == Some(&lookup) {
            *seen = Some(lookup);
            return false;
        }
        if self.current != *seen {
            return false;
        }
        self.current = Some(lookup.clone());
        *seen = Some(lookup);
        true
    }
}

pub fn state_root(env: &Env) -> Option<PathBuf> {
    let base = env.dir_var("XDG_STATE_HOME");
    #[cfg(windows)]
    let base = base.or_else(|| env.dir_var("LOCALAPPDATA"));
    base.or_else(|| env.home().map(|home| home.join(".local").join("state")))
        .map(|base| base.join("oceans").join("attribution"))
}

impl State {
    pub fn from_env(env: &Env) -> anyhow::Result<Self> {
        let root = state_root(env)
            .context("could not find a state directory; set HOME or XDG_STATE_HOME")?;
        Self::open(root)
    }

    /// Opens the state directory, creating it owner-only. Refuses one other users can access.
    pub fn open(root: PathBuf) -> anyhow::Result<Self> {
        ensure_private_dir(&root)?;
        for name in [QUEUE_DIR, UNSEQUENCED_DIR, QUARANTINE_DIR, PR_SYNC_DIR] {
            ensure_private_dir(&root.join(name))?;
        }
        Ok(Self {
            root,
            lock_timeout: LOCK_TIMEOUT,
            queue_cap_bytes: QUEUE_CAP_BYTES,
        })
    }

    pub fn dir(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// Writes `bytes` to `target` through a temporary file in `directory` and a rename, so a
    /// reader never sees a partial file.
    pub(crate) fn write_atomic(
        &self,
        directory: &Path,
        target: &Path,
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        let mut pending = tempfile::Builder::new()
            .prefix(TEMP_PREFIX)
            .tempfile_in(directory)?;
        pending.write_all(bytes)?;
        pending.as_file().sync_all()?;
        pending
            .persist(target)
            .map_err(|error| error.error)
            .with_context(|| format!("could not write {}", target.display()))?;
        // The rename is durable only once the directory is synced. Without this a power loss
        // can bring back the old sequence counter, and the next entries would reuse sequences.
        #[cfg(unix)]
        std::fs::File::open(directory)
            .and_then(|directory| directory.sync_all())
            .with_context(|| format!("could not sync {}", directory.display()))?;
        Ok(())
    }

    /// Takes the queue lock, waiting at most `timeout`. `None` means it timed out.
    pub fn lock_queue(&self, timeout: Duration) -> anyhow::Result<Option<LockGuard>> {
        let file = open_lock_file(&self.root.join(LOCK_FILE))?;
        let deadline = Instant::now() + timeout;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Some(LockGuard(file))),
                Err(TryLockError::WouldBlock) => {}
                Err(TryLockError::Error(error)) => {
                    return Err(error).context("could not lock the attribution queue");
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            thread::sleep((deadline - now).min(Duration::from_millis(5)));
        }
    }

    /// Takes the single-sender lock without waiting. `None` means another sender runs.
    pub fn try_lock_sender(&self) -> anyhow::Result<Option<LockGuard>> {
        let file = open_lock_file(&self.root.join(SENDER_LOCK_FILE))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(LockGuard(file))),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error).context("could not lock the sender"),
        }
    }

    /// The per-installation id, created on first use. Racing hooks agree on one id because it
    /// is created without replacing an existing file.
    pub fn installation_id(&self) -> anyhow::Result<String> {
        let path = self.root.join(INSTALLATION_ID_FILE);
        for _ in 0..2 {
            match read_small(&path) {
                Ok(text) => {
                    let id = Uuid::parse_str(text.trim())
                        .with_context(|| format!("{} is corrupt", path.display()))?;
                    return Ok(id.to_string());
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).context("could not read the installation id"),
            }
            let mut pending = tempfile::Builder::new()
                .prefix(TEMP_PREFIX)
                .tempfile_in(&self.root)?;
            let id = Uuid::new_v4().to_string();
            pending.write_all(id.as_bytes())?;
            pending.as_file().sync_all()?;
            match pending.persist_noclobber(&path) {
                Ok(_) => return Ok(id),
                Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error.error).context("could not save the installation id");
                }
            }
        }
        bail!("could not create the installation id")
    }

    /// The last allocated sequence number. Call only while holding the queue lock.
    pub(crate) fn read_counter(&self) -> anyhow::Result<u64> {
        read_number(&self.root.join(COUNTER_FILE))
    }

    /// Call only while holding the queue lock.
    pub(crate) fn write_counter(&self, value: u64) -> anyhow::Result<()> {
        self.write_atomic(
            &self.root,
            &self.root.join(COUNTER_FILE),
            value.to_string().as_bytes(),
        )
    }

    pub fn dropped(&self) -> anyhow::Result<u64> {
        read_number(&self.root.join(DROPPED_FILE))
    }

    pub(crate) fn add_dropped(&self, count: u64) -> anyhow::Result<()> {
        if count == 0 {
            return Ok(());
        }
        let total = self.dropped().unwrap_or(0).saturating_add(count);
        self.write_atomic(
            &self.root,
            &self.root.join(DROPPED_FILE),
            total.to_string().as_bytes(),
        )
    }

    /// Bytes held by the queue, unsequenced and quarantine folders, for the 512 MiB cap.
    pub(crate) fn stored_bytes(&self) -> anyhow::Result<u64> {
        let mut total = 0;
        for name in [QUEUE_DIR, UNSEQUENCED_DIR, QUARANTINE_DIR] {
            for (_, metadata) in files(&self.root.join(name))? {
                total += metadata.len();
            }
        }
        Ok(total)
    }

    /// The scope hooks stamp on new entries. Hooks read only this local file.
    pub fn current_scope(&self) -> Option<String> {
        self.ownership_cache()
            .current
            .map(|current| current.ownership_scope)
    }

    /// The current ownership, for a later `*_unless_changed` update to compare against.
    pub fn current_ownership(&self) -> Option<CachedOwnership> {
        self.ownership_cache().current
    }

    pub fn cached_ownership(
        &self,
        gateway_url: &str,
        fingerprint: &str,
    ) -> Option<CachedOwnership> {
        self.ownership_cache().lookups.into_iter().find(|lookup| {
            lookup.gateway_url == gateway_url && lookup.key_fingerprint == fingerprint
        })
    }

    /// Caches a lookup for `(gateway_url, fingerprint)` and makes it the current scope.
    pub fn record_ownership(
        &self,
        gateway_url: &str,
        fingerprint: &str,
        ownership: &Ownership,
    ) -> anyhow::Result<()> {
        let lookup = cached_lookup(gateway_url, fingerprint, ownership);
        let mut cache = self.ownership_cache();
        cache.cache_lookup(lookup.clone());
        cache.current = Some(lookup);
        self.write_ownership_cache(&cache)
    }

    /// Caches a lookup like `record_ownership`, but makes it the current scope only while the
    /// current ownership is still `seen_current`, read before the lookup started. A login that
    /// ran meanwhile keeps its scope. `seen_current` becomes the lookup once it is current.
    pub fn record_ownership_unless_changed(
        &self,
        gateway_url: &str,
        fingerprint: &str,
        ownership: &Ownership,
        seen_current: &mut Option<CachedOwnership>,
    ) -> anyhow::Result<()> {
        let lookup = cached_lookup(gateway_url, fingerprint, ownership);
        let mut cache = self.ownership_cache();
        cache.cache_lookup(lookup.clone());
        cache.replace_current_if(seen_current, lookup);
        self.write_ownership_cache(&cache)
    }

    /// Makes an existing lookup current again, for a key switched back to an earlier owner,
    /// under the same rule as `record_ownership_unless_changed`.
    pub fn make_current_unless_changed(
        &self,
        lookup: CachedOwnership,
        seen_current: &mut Option<CachedOwnership>,
    ) -> anyhow::Result<()> {
        let mut cache = self.ownership_cache();
        if !cache.replace_current_if(seen_current, lookup) {
            return Ok(());
        }
        self.write_ownership_cache(&cache)
    }

    pub fn forget_ownership(&self, gateway_url: &str, fingerprint: &str) -> anyhow::Result<()> {
        let mut cache = self.ownership_cache();
        cache.lookups.retain(|cached| {
            cached.gateway_url != gateway_url || cached.key_fingerprint != fingerprint
        });
        self.write_ownership_cache(&cache)
    }

    fn ownership_cache(&self) -> OwnershipCache {
        read_small(&self.root.join(OWNERSHIP_CACHE_FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn write_ownership_cache(&self, cache: &OwnershipCache) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec_pretty(cache)?;
        self.write_atomic(&self.root, &self.root.join(OWNERSHIP_CACHE_FILE), &bytes)
    }

    /// Whether the last PR sync for `repository` is older than `window`.
    pub fn pr_sync_due(&self, repository: &str, window: Duration, now: SystemTime) -> bool {
        let last = read_number(&self.pr_sync_path(repository)).unwrap_or(0);
        let now = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        now.saturating_sub(last) >= window.as_secs()
    }

    pub fn mark_pr_sync(&self, repository: &str, now: SystemTime) -> anyhow::Result<()> {
        let seconds = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        self.write_atomic(
            &self.root.join(PR_SYNC_DIR),
            &self.pr_sync_path(repository),
            seconds.to_string().as_bytes(),
        )
    }

    fn pr_sync_path(&self, repository: &str) -> PathBuf {
        let digest = format!("{:x}", Sha256::digest(repository.as_bytes()));
        self.root.join(PR_SYNC_DIR).join(&digest[..16])
    }

    /// Removes temporary files a crash left behind and quarantined entries older than 30 days.
    pub fn cleanup(&self, now: SystemTime) -> anyhow::Result<()> {
        let age = |metadata: &fs::Metadata| {
            metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .unwrap_or_default()
        };
        for name in ["", QUEUE_DIR, UNSEQUENCED_DIR, QUARANTINE_DIR, PR_SYNC_DIR] {
            let quarantine = name == QUARANTINE_DIR;
            for (entry, metadata) in files(&self.root.join(name))? {
                let temporary = entry.file_name().to_string_lossy().starts_with(TEMP_PREFIX);
                let expired = (temporary && age(&metadata) > STALE_TEMP_AGE)
                    || (quarantine && !temporary && age(&metadata) > QUARANTINE_RETENTION);
                if expired {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        Ok(())
    }
}

fn cached_lookup(gateway_url: &str, fingerprint: &str, ownership: &Ownership) -> CachedOwnership {
    CachedOwnership {
        gateway_url: gateway_url.to_owned(),
        key_fingerprint: fingerprint.to_owned(),
        ownership_scope: ownership.ownership_scope.clone(),
        owner_kind: ownership.owner_kind.clone(),
        owner_id: ownership.owner_id.clone(),
    }
}

/// The regular files in `directory` with their metadata. Hooks and the sender rename files
/// concurrently, so one that disappears while listing is skipped.
pub(crate) fn files(directory: &Path) -> io::Result<Vec<(fs::DirEntry, fs::Metadata)>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        match entry.metadata() {
            Ok(metadata) if metadata.is_file() => files.push((entry, metadata)),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(files)
}

fn open_lock_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn read_small(path: &Path) -> io::Result<String> {
    let mut text = String::new();
    File::open(path)?
        .take(MAX_SMALL_FILE_BYTES)
        .read_to_string(&mut text)?;
    Ok(text)
}

fn read_number(path: &Path) -> anyhow::Result<u64> {
    match read_small(path) {
        Ok(text) => text
            .trim()
            .parse()
            .with_context(|| format!("{} is corrupt", path.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error).with_context(|| format!("could not read {}", path.display())),
    }
}

fn ensure_private_dir(path: &Path) -> anyhow::Result<()> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => check_private(path, &metadata),
        Ok(_) => bail!("{} is not a directory", path.display()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            create_private_dir(path).with_context(|| format!("could not create {}", path.display()))
        }
        Err(error) => Err(error).with_context(|| format!("could not inspect {}", path.display())),
    }
}

#[cfg(unix)]
fn create_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

/// The queue holds raw notes, so it must stay unreadable by other users.
#[cfg(unix)]
fn check_private(path: &Path, metadata: &fs::Metadata) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if metadata.permissions().mode() & 0o077 != 0 {
        bail!(
            "{} is accessible by other users, so attribution data won't be queued there; run chmod 700 {}",
            path.display(),
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_private(_path: &Path, _metadata: &fs::Metadata) -> anyhow::Result<()> {
    Ok(())
}
