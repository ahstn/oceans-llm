//! Queued entries: sequencing under the queue lock, quarantine and listing (ADR "Freshness"
//! and "Bounds").
//!
//! - `queue/<sequence>-<entry_id>.json`: entries ready to deliver, oldest sequence first.
//! - `unsequenced/<observed_at nanos>-<entry_id>.json`: entries a hook wrote after the lock
//!   timed out. The sender numbers them under the lock and marks them `late_sequenced`.
//! - `quarantine/<entry_id>.json`: entries that won't be sent, with the reason.

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use super::{
    payload::Payload,
    state::{QUARANTINE_DIR, QUEUE_DIR, State, TEMP_PREFIX, UNSEQUENCED_DIR, files},
};

/// One queued fact. `ownership_scope` is the owner it was queued for; it never leaves the
/// machine, but the sender only delivers an entry under that owner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub entry_id: String,
    pub installation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    pub observed_at: String,
    #[serde(default)]
    pub late_sequenced: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ownership_scope: Option<String>,
    pub payload: Payload,
}

/// The gateway's view of an entry: everything except the local ownership stamp.
#[derive(Serialize)]
pub struct WireEntry<'a> {
    entry_id: &'a str,
    installation_id: &'a str,
    sequence: u64,
    observed_at: &'a str,
    late_sequenced: bool,
    payload: &'a Payload,
}

impl Entry {
    pub fn wire(&self) -> Option<WireEntry<'_>> {
        Some(WireEntry {
            entry_id: &self.entry_id,
            installation_id: &self.installation_id,
            sequence: self.sequence?,
            observed_at: &self.observed_at,
            late_sequenced: self.late_sequenced,
            payload: &self.payload,
        })
    }

    fn observed_nanos(&self) -> i128 {
        OffsetDateTime::parse(&self.observed_at, &Rfc3339)
            .map(OffsetDateTime::unix_timestamp_nanos)
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarantineRecord {
    pub reason: String,
    pub quarantined_at: String,
    pub entry: Entry,
}

#[derive(Debug, Default, PartialEq)]
pub struct EnqueueReport {
    pub queued: usize,
    pub unsequenced: usize,
    pub quarantined: usize,
    pub dropped: usize,
}

/// A listing row. It shows the repository, commit, kind and reason, never content or ids.
#[derive(Debug, Serialize, PartialEq)]
pub struct EntrySummary {
    pub kind: &'static str,
    pub repository: Option<String>,
    pub commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct QueueListing {
    pub queued: Vec<EntrySummary>,
    pub quarantined: Vec<EntrySummary>,
    pub dropped: u64,
}

pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("the current time formats as RFC 3339")
}

fn summary(entry: &Entry, reason: Option<&str>) -> EntrySummary {
    let (repository, commit) = entry.payload.listing();
    EntrySummary {
        kind: entry.payload.kind(),
        repository: repository.map(str::to_owned),
        commit: commit.map(str::to_owned),
        reason: reason.map(str::to_owned),
    }
}

pub fn read_entry(path: &Path) -> anyhow::Result<Entry> {
    serde_json::from_slice(&fs::read(path)?)
        .with_context(|| format!("{} is corrupt", path.display()))
}

fn read_quarantine(path: &Path) -> anyhow::Result<QuarantineRecord> {
    serde_json::from_slice(&fs::read(path)?)
        .with_context(|| format!("{} is corrupt", path.display()))
}

/// Entry files in `directory`, sorted by name: sequence order in `queue/`, observed order in
/// `unsequenced/`.
fn entry_files(directory: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = files(directory)?
        .into_iter()
        .filter(|(entry, _)| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            !name.starts_with(TEMP_PREFIX) && name.ends_with(".json")
        })
        .map(|(entry, _)| entry.path())
        .collect();
    paths.sort();
    Ok(paths)
}

impl State {
    fn new_entry(
        &self,
        installation_id: &str,
        scope: Option<&str>,
        observed_at: &str,
        payload: Payload,
    ) -> Entry {
        Entry {
            entry_id: Uuid::new_v4().simple().to_string(),
            installation_id: installation_id.to_owned(),
            sequence: None,
            observed_at: observed_at.to_owned(),
            late_sequenced: false,
            ownership_scope: scope.map(str::to_owned),
            payload,
        }
    }

    fn queue_path(&self, sequence: u64, entry_id: &str) -> PathBuf {
        self.dir(QUEUE_DIR)
            .join(format!("{sequence:020}-{entry_id}.json"))
    }

    fn unsequenced_path(&self, entry: &Entry) -> PathBuf {
        let nanos = entry.observed_nanos().max(0);
        self.dir(UNSEQUENCED_DIR)
            .join(format!("{nanos:020}-{}.json", entry.entry_id))
    }

    /// Stamps and queues `payloads` under one lock. Each gets the current ownership scope from
    /// the local cache, the installation id, the next sequence and the time it was observed.
    /// Past the 512 MiB cap entries are dropped and counted.
    pub fn enqueue(&self, payloads: Vec<Payload>) -> anyhow::Result<EnqueueReport> {
        let mut report = EnqueueReport::default();
        if payloads.is_empty() {
            return Ok(report);
        }
        let installation_id = self.installation_id()?;
        let scope = self.current_scope();
        let observed_at = now_rfc3339();
        let mut entries: Vec<Entry> = payloads
            .into_iter()
            .map(|payload| {
                self.new_entry(&installation_id, scope.as_deref(), &observed_at, payload)
            })
            .collect();
        let mut used = self.stored_bytes()?;
        let lock = self.lock_queue(self.lock_timeout)?;
        if lock.is_some() {
            let start = self.read_counter()?;
            // The counter moves first: a crash before the entries are written leaves a gap,
            // which is harmless because sequences only need to increase.
            self.write_counter(start + entries.len() as u64)?;
            for (offset, entry) in entries.iter_mut().enumerate() {
                entry.sequence = Some(start + 1 + offset as u64);
            }
        }
        for entry in &entries {
            let bytes = serde_json::to_vec(entry)?;
            if used + bytes.len() as u64 > self.queue_cap_bytes {
                report.dropped += 1;
                continue;
            }
            match entry.sequence {
                Some(sequence) => {
                    let path = self.queue_path(sequence, &entry.entry_id);
                    self.write_atomic(&self.dir(QUEUE_DIR), &path, &bytes)?;
                    report.queued += 1;
                }
                None => {
                    let path = self.unsequenced_path(entry);
                    self.write_atomic(&self.dir(UNSEQUENCED_DIR), &path, &bytes)?;
                    report.unsequenced += 1;
                }
            }
            used += bytes.len() as u64;
        }
        self.add_dropped(report.dropped as u64)?;
        drop(lock);
        Ok(report)
    }

    /// Writes new payloads straight to quarantine, such as a note over 1 MiB.
    pub fn quarantine_payloads(
        &self,
        payloads: Vec<(Payload, &str)>,
    ) -> anyhow::Result<EnqueueReport> {
        let mut report = EnqueueReport::default();
        if payloads.is_empty() {
            return Ok(report);
        }
        let installation_id = self.installation_id()?;
        let scope = self.current_scope();
        let observed_at = now_rfc3339();
        let mut used = self.stored_bytes()?;
        for (payload, reason) in payloads {
            let entry = self.new_entry(&installation_id, scope.as_deref(), &observed_at, payload);
            let record = QuarantineRecord {
                reason: reason.to_owned(),
                quarantined_at: observed_at.clone(),
                entry,
            };
            let bytes = serde_json::to_vec(&record)?;
            if used + bytes.len() as u64 > self.queue_cap_bytes {
                report.dropped += 1;
                continue;
            }
            self.write_quarantine(&record, &bytes)?;
            used += bytes.len() as u64;
            report.quarantined += 1;
        }
        self.add_dropped(report.dropped as u64)?;
        Ok(report)
    }

    fn write_quarantine(&self, record: &QuarantineRecord, bytes: &[u8]) -> anyhow::Result<()> {
        let directory = self.dir(QUARANTINE_DIR);
        let path = directory.join(format!("{}.json", record.entry.entry_id));
        self.write_atomic(&directory, &path, bytes)
    }

    /// Moves a queued file to quarantine with `reason`. It is never retried unless resent.
    pub fn quarantine_file(&self, path: &Path, entry: Entry, reason: &str) -> anyhow::Result<()> {
        let record = QuarantineRecord {
            reason: reason.to_owned(),
            quarantined_at: now_rfc3339(),
            entry,
        };
        self.write_quarantine(&record, &serde_json::to_vec(&record)?)?;
        fs::remove_file(path).with_context(|| format!("could not remove {}", path.display()))
    }

    /// Numbers entries hooks wrote to `unsequenced/`, oldest observation first, and moves
    /// them to the queue as `late_sequenced`. Returns how many moved; none when the lock is busy.
    pub fn sequence_unsequenced(&self, timeout: Duration) -> anyhow::Result<usize> {
        let paths = entry_files(&self.dir(UNSEQUENCED_DIR))?;
        if paths.is_empty() {
            return Ok(0);
        }
        let Some(_lock) = self.lock_queue(timeout)? else {
            return Ok(0);
        };
        let mut entries = Vec::new();
        let mut corrupt = 0;
        for path in paths {
            match read_entry(&path) {
                Ok(entry) => entries.push((path, entry)),
                Err(_) => {
                    let _ = fs::remove_file(&path);
                    corrupt += 1;
                }
            }
        }
        entries.sort_by_key(|(_, entry)| entry.observed_nanos());
        let start = self.read_counter()?;
        self.write_counter(start + entries.len() as u64)?;
        let moved = entries.len();
        for (offset, (path, mut entry)) in entries.into_iter().enumerate() {
            let sequence = start + 1 + offset as u64;
            entry.sequence = Some(sequence);
            entry.late_sequenced = true;
            let target = self.queue_path(sequence, &entry.entry_id);
            self.write_atomic(&self.dir(QUEUE_DIR), &target, &serde_json::to_vec(&entry)?)?;
            fs::remove_file(&path)?;
        }
        self.add_dropped(corrupt)?;
        Ok(moved)
    }

    /// Queued entry files in delivery order. Sequences are unique per installation, so file
    /// name order is sequence order.
    pub fn queued_files(&self) -> anyhow::Result<Vec<PathBuf>> {
        entry_files(&self.dir(QUEUE_DIR))
    }

    pub fn has_pending(&self) -> anyhow::Result<bool> {
        Ok(
            !self.queued_files()?.is_empty()
                || !entry_files(&self.dir(UNSEQUENCED_DIR))?.is_empty(),
        )
    }

    /// Deletes an unreadable queue file and counts it as dropped.
    pub fn drop_corrupt(&self, path: &Path) -> anyhow::Result<()> {
        fs::remove_file(path)?;
        self.add_dropped(1)
    }

    pub fn listing(&self) -> anyhow::Result<QueueListing> {
        let mut queued = Vec::new();
        for directory in [UNSEQUENCED_DIR, QUEUE_DIR] {
            for path in entry_files(&self.dir(directory))? {
                if let Ok(entry) = read_entry(&path) {
                    queued.push(summary(&entry, None));
                }
            }
        }
        let mut quarantined = Vec::new();
        for path in entry_files(&self.dir(QUARANTINE_DIR))? {
            if let Ok(record) = read_quarantine(&path) {
                quarantined.push(summary(&record.entry, Some(&record.reason)));
            }
        }
        Ok(QueueListing {
            queued,
            quarantined,
            dropped: self.dropped()?,
        })
    }

    /// Deletes every quarantined entry. Returns how many were deleted.
    pub fn discard_quarantine(&self) -> anyhow::Result<usize> {
        let paths = entry_files(&self.dir(QUARANTINE_DIR))?;
        for path in &paths {
            fs::remove_file(path)?;
        }
        Ok(paths.len())
    }

    /// Moves quarantined entries back to the queue. An entry keeps its stamped owner, so it is
    /// delivered only once that owner's key is configured again; an entry queued before any
    /// owner was known takes the current one. Entries keep their sequence, so a resent note
    /// can't replace a newer one.
    pub fn resend_quarantine(&self) -> anyhow::Result<usize> {
        let scope = self.current_scope();
        let mut moved = 0;
        for path in entry_files(&self.dir(QUARANTINE_DIR))? {
            let Ok(record) = read_quarantine(&path) else {
                continue;
            };
            let mut entry = record.entry;
            if entry.ownership_scope.is_none() {
                entry.ownership_scope.clone_from(&scope);
            }
            let (directory, target) = match entry.sequence {
                Some(sequence) => (QUEUE_DIR, self.queue_path(sequence, &entry.entry_id)),
                None => (UNSEQUENCED_DIR, self.unsequenced_path(&entry)),
            };
            self.write_atomic(&self.dir(directory), &target, &serde_json::to_vec(&entry)?)?;
            fs::remove_file(&path)?;
            moved += 1;
        }
        Ok(moved)
    }
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
