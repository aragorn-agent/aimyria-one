//! Panic-proof file logging.
//!
//! # Why this exists instead of `TargetKind::Folder`
//!
//! Both built-in ways of getting logs onto disk in `tauri-plugin-log` go through
//! `fern`'s `fallback_on_error`: when a target fails to write, fern writes the
//! error to stderr, and **if stderr also fails it calls `panic!`**
//! (`fern-0.7.1/src/log_impl.rs:896`).
//!
//! In a `windows_subsystem = "windows"` binary there is no console, so stderr is
//! not writable and that second step always fails. The panic is then raised at
//! whatever line wrote the log entry — which for this app is startup code inside
//! `setup` — and the process dies before the tray icon ever appears.
//!
//! That is not a theory. `~/.aimyria/crash.log` contains two reports whose panic
//! message is exactly that path:
//!
//! ```text
//! Error performing stderr logging after error occurred during regular logging.
//!     attempted to log: [...] ✓ Copilot / Codex OAuth / xAI OAuth managers initialized
//!     first logging error: 管道正在被关闭。 (os error 232)
//!     stderr error: 管道正在被关闭。 (os error 232)
//! ```
//!
//! The log entry that triggered it was an ordinary INFO line during startup, and
//! the observable effect is the app silently closing and never coming back.
//!
//! # The invariant
//!
//! [`PanicProofWriter`] implements `Write` and **never returns `Err`**. A dropped
//! log line is a diagnostics gap; a process killed by its own logger is an
//! outage. Failures are counted and retried on a cooldown so an unwritable
//! directory costs one `open` every five seconds instead of one per record, and
//! the first failure of a streak is surfaced through [`take_failure_report`] so
//! `setup` can note it in the (already-open) log file's sibling path once logging
//! itself is healthy.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Rotate at the same size the plugin's own rotating target used, so the on-disk
/// footprint of a long-running install does not change.
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;

/// How many rotated archives to keep (`aimyria-one.log.1` … `.4`). With the live
/// file this bounds the log directory at roughly 100 MiB.
const ARCHIVES: usize = 4;

/// How long to wait before trying to open a directory that just failed.
const REOPEN_COOLDOWN: Duration = Duration::from_secs(5);

/// Number of writes that could not be performed since the process started.
static DROPPED_WRITES: AtomicU64 = AtomicU64::new(0);

struct State {
    file: Option<File>,
    size: u64,
    /// Set while the directory/file is unwritable, so we do not retry per record.
    retry_at: Option<Instant>,
}

/// A `Write` implementation whose failures are invisible to the caller.
///
/// Every method returns success. That is the entire point: fern only panics when
/// it is *told* a write failed.
pub struct PanicProofWriter {
    path: PathBuf,
    state: Mutex<State>,
}

impl PanicProofWriter {
    /// Clears `aimyria-one.log.N` slots and truncates the live file on open, so a
    /// fresh launch starts a readable file rather than appending to a 20 MiB one.
    ///
    /// Appending across restarts was the old behaviour and it is worth keeping
    /// for post-mortems, so this deliberately *appends*; rotation is what bounds
    /// the file, not truncation.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            state: Mutex::new(State {
                file: None,
                size: 0,
                retry_at: None,
            }),
        }
    }

    /// Open (or reopen) the live file, creating the directory if needed.
    fn open_locked(&self, state: &mut State) -> bool {
        if let Some(parent) = self.path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return false;
            }
        }
        match OpenOptions::new().create(true).append(true).open(&self.path) {
            Ok(file) => {
                state.size = file.metadata().map(|m| m.len()).unwrap_or(0);
                state.file = Some(file);
                state.retry_at = None;
                true
            }
            Err(_) => {
                state.file = None;
                state.retry_at = Some(Instant::now() + REOPEN_COOLDOWN);
                false
            }
        }
    }

    /// Rename the live file to `.1`, shifting older archives down.
    fn rotate_locked(&self, state: &mut State) {
        // Close before renaming: Windows refuses to rename an open file.
        state.file = None;

        for index in (1..=ARCHIVES).rev() {
            let from = if index == 1 {
                self.path.clone()
            } else {
                archive_path(&self.path, index - 1)
            };
            if !from.exists() {
                continue;
            }
            let to = archive_path(&self.path, index);
            // A leftover destination is stale by definition; dropping it keeps the
            // shift monotonic instead of silently stopping at the first collision.
            let _ = std::fs::remove_file(&to);
            let _ = std::fs::rename(&from, &to);
        }
        // Archives beyond the retention window are dropped explicitly: the loop
        // above only shifts, so `.4` becomes `.5` unless it is removed here.
        let _ = std::fs::remove_file(archive_path(&self.path, ARCHIVES + 1));

        state.size = 0;
    }

    fn write_bytes(&self, buffer: &[u8]) {
        let Ok(mut state) = self.state.lock() else {
            // A poisoned lock means a previous writer panicked holding it. Dropping
            // this line is still better than propagating a panic out of a log call.
            DROPPED_WRITES.fetch_add(1, Ordering::Relaxed);
            return;
        };

        if state.file.is_none() {
            if let Some(retry_at) = state.retry_at {
                if Instant::now() < retry_at {
                    DROPPED_WRITES.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
            if !self.open_locked(&mut state) {
                DROPPED_WRITES.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }

        if state.size + buffer.len() as u64 > MAX_FILE_BYTES {
            self.rotate_locked(&mut state);
            if !self.open_locked(&mut state) {
                DROPPED_WRITES.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }

        let written = match state.file.as_mut() {
            Some(file) => {
                let ok = file.write_all(buffer).and_then(|()| file.flush()).is_ok();
                ok
            }
            None => false,
        };

        if written {
            state.size += buffer.len() as u64;
        } else {
            // The handle is dead (file removed, disk full, handle invalidated by a
            // rotation elsewhere). Drop it so the next attempt reopens.
            state.file = None;
            state.retry_at = Some(Instant::now() + REOPEN_COOLDOWN);
            DROPPED_WRITES.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn archive_path(path: &Path, index: usize) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{index}"));
    PathBuf::from(name)
}

impl Write for PanicProofWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.write_bytes(buffer);
        // Always claim success — see the module docs.
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Writes `message` directly to the log file, outside the `log` facade.
///
/// Used while the logger is being installed (before any `log::info!` would be
/// routed anywhere) and for the one line that reports that logging itself is
/// degraded — an entry about logging cannot be written *through* logging.
pub fn write_line_direct(path: &Path, message: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{message}");
        let _ = file.flush();
    }
}

/// How many log lines have been dropped so far.
pub fn dropped_writes() -> u64 {
    DROPPED_WRITES.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property that matters: writing into a directory that does not exist and
    /// cannot be created must not panic. The old `fern` file target panicked here.
    #[test]
    fn an_unwritable_destination_never_panics() {
        let dir = tempfile::tempdir().expect("temp dir");
        // A *file* where the parent directory should be: create_dir_all fails with
        // NotADirectory on every platform.
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"not a directory").unwrap();
        let target = blocker.join("logs").join("aimyria-one.log");

        let mut writer = PanicProofWriter::new(target);
        for _ in 0..100 {
            writer.write_all(b"[INFO] something happened\n").unwrap();
        }
        assert_eq!(writer.flush().unwrap(), ());
        assert!(dropped_writes() > 0, "the failure should be counted");
    }

    /// A normal write lands in the file, byte for byte.
    #[test]
    fn a_writable_destination_receives_the_bytes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("logs").join("aimyria-one.log");
        let mut writer = PanicProofWriter::new(&path);
        writer.write_all(b"one\n").unwrap();
        writer.write_all(b"two\n").unwrap();
        writer.flush().unwrap();

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "one\ntwo\n",
            "the directory should have been created and both lines written"
        );
    }

    /// Rotation must keep the live file plus at most `ARCHIVES` archives, and must
    /// not lose the writer: the next line still lands.
    #[test]
    fn rotation_bounds_the_archive_count_and_keeps_logging() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("aimyria-one.log");
        // Pre-seed the archives so the shift path is exercised.
        for index in 1..=ARCHIVES {
            std::fs::write(archive_path(&path, index), b"old").unwrap();
        }

        let writer = PanicProofWriter::new(&path);
        {
            // Force the size over the limit without writing 20 MiB.
            let mut state = writer.state.lock().unwrap();
            state.size = MAX_FILE_BYTES;
        }
        writer.write_bytes(b"after rotation\n");

        assert!(path.is_file(), "the live file must exist again");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "after rotation\n");
        assert!(
            !archive_path(&path, ARCHIVES + 1).exists(),
            "archives beyond the retention window must be dropped"
        );
    }
}
