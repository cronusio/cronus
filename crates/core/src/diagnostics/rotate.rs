//! A size-rotating file writer for the diagnostic log.
//!
//! One file family per process kind: `<dir>/<kind>.jsonl` is the file being
//! written, `<dir>/<kind>.1.jsonl` the previous one, `<dir>/<kind>.2.jsonl` the
//! one before it, up to `max_files` rotations. When the next write would take
//! the active file past `max_size` the family shifts up by one and the oldest
//! rotation is dropped silently, by design: the log is bounded, and what falls
//! off the end is what nobody needed.
//!
//! The writer never fails the work it describes. A directory that cannot be
//! created, a file that cannot be opened and a rotation that cannot complete
//! all leave [`Write::write`] returning `Ok` for the whole buffer; the line is
//! dropped, counted in [`RotatingWriter::lost`], and the next write tries again
//! from the start, so a log directory that becomes usable later starts
//! receiving lines without a restart.
//!
//! Rotation exists here and not in a library because the rotating appender the
//! workspace would otherwise use rotates by calendar time, and the size bound is
//! the one the log's contract states.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Default size at which the active file rotates: 10 MiB.
pub const DEFAULT_MAX_SIZE: u64 = 10 * 1024 * 1024;

/// Default number of rotated files kept beside the active one.
pub const DEFAULT_MAX_FILES: usize = 5;

/// A [`Write`] implementation over `<dir>/<kind>.jsonl` that rotates by size
/// and swallows I/O failures, counting what it dropped.
#[derive(Debug)]
pub struct RotatingWriter {
    dir: PathBuf,
    kind: String,
    max_size: u64,
    max_files: usize,
    file: Option<File>,
    size: u64,
    lost: u64,
}

impl RotatingWriter {
    /// Create a writer for the file family `<kind>` under `dir`.
    ///
    /// Nothing touches the file system until the first write, so constructing
    /// a writer cannot fail. `max_size` is the byte count at which the active
    /// file rotates; `max_files` is how many rotations are kept (`0` keeps
    /// none: the active file is emptied instead).
    pub fn new(
        dir: impl Into<PathBuf>,
        kind: impl Into<String>,
        max_size: u64,
        max_files: usize,
    ) -> Self {
        RotatingWriter {
            dir: dir.into(),
            kind: kind.into(),
            max_size,
            max_files,
            file: None,
            size: 0,
            lost: 0,
        }
    }

    /// Number of writes dropped because the file system refused them, since
    /// the writer was created.
    pub fn lost(&self) -> u64 {
        self.lost
    }

    /// The directory this writer's file family lives in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn active_path(&self) -> PathBuf {
        self.dir.join(format!("{}.jsonl", self.kind))
    }

    fn rotated_path(&self, index: usize) -> PathBuf {
        self.dir.join(format!("{}.{index}.jsonl", self.kind))
    }

    fn open_if_needed(&mut self) -> io::Result<()> {
        if self.file.is_some() {
            return Ok(());
        }
        create_private_dir(&self.dir)?;
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        restrict_to_owner(&mut options);
        let file = options.open(self.active_path())?;
        self.size = file.metadata()?.len();
        self.file = Some(file);
        Ok(())
    }

    /// Shift the family up by one so the active path is free.
    ///
    /// The shift fills the first empty slot rather than always dropping the
    /// last one, so a rotation that failed half way is finished by the next
    /// attempt without discarding a file that had already moved.
    fn rotate(&mut self) -> io::Result<()> {
        // Close the active file first: a rename of an open file fails on some
        // platforms, and the next write reopens it anyway.
        self.file = None;
        self.size = 0;

        if self.max_files == 0 {
            return remove_if_present(&self.active_path());
        }

        let mut free = (1..=self.max_files).find(|&i| !self.rotated_path(i).exists());
        if free.is_none() {
            remove_if_present(&self.rotated_path(self.max_files))?;
            free = Some(self.max_files);
        }
        let free = free.unwrap_or(self.max_files);

        for index in (1..free).rev() {
            rename_if_present(&self.rotated_path(index), &self.rotated_path(index + 1))?;
        }
        rename_if_present(&self.active_path(), &self.rotated_path(1))
    }

    fn append(&mut self, buf: &[u8]) -> io::Result<()> {
        self.open_if_needed()?;
        let incoming = u64::try_from(buf.len()).unwrap_or(u64::MAX);
        // A line larger than the whole bound is still written, whole, to a
        // fresh file: splitting a record across files would corrupt both.
        if self.size > 0 && self.size.saturating_add(incoming) > self.max_size {
            self.rotate()?;
            self.open_if_needed()?;
        }
        let Some(file) = self.file.as_mut() else {
            return Err(io::Error::other("log file is not open"));
        };
        match file.write_all(buf) {
            Ok(()) => {
                self.size = self.size.saturating_add(incoming);
                Ok(())
            }
            Err(error) => {
                // A failed write may have left a partial line; reopen on the
                // next attempt so the size is read back from the file.
                self.file = None;
                Err(error)
            }
        }
    }
}

impl Write for RotatingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.append(buf).is_err() {
            self.lost = self.lost.saturating_add(1);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = self.file.as_mut()
            && file.flush().is_err()
        {
            self.file = None;
        }
        Ok(())
    }
}

/// Create the log directory readable by its owner alone where the platform has
/// permission bits. A log records arguments, paths and failure detail, and a
/// shared machine should not let another account read it. Where the platform
/// has none (Windows), the directory inherits its parent's access list, which
/// for a per-user location is already the user's own.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)
    }
}

/// New log files are readable and writable by their owner alone (a no-op where
/// the platform has no permission bits).
fn restrict_to_owner(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    {
        let _ = options;
    }
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn rename_if_present(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    /// A scratch directory that removes itself, unique per test and process.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("cronus-diag-rotate-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create scratch directory");
            Scratch(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).expect("read log file")
    }

    #[test]
    fn writes_past_max_size_rotate_to_a_numbered_file() {
        let scratch = Scratch::new();
        let mut writer = RotatingWriter::new(scratch.path(), "cli", 20, 5);
        writer.write_all(b"first-line-1234\n").expect("write");
        writer.write_all(b"second-line-123\n").expect("write");

        assert_eq!(read(&scratch.path().join("cli.jsonl")), "second-line-123\n");
        assert_eq!(
            read(&scratch.path().join("cli.1.jsonl")),
            "first-line-1234\n"
        );
        assert_eq!(writer.lost(), 0);
    }

    #[test]
    fn a_write_that_fits_does_not_rotate() {
        let scratch = Scratch::new();
        let mut writer = RotatingWriter::new(scratch.path(), "cli", 64, 5);
        writer.write_all(b"one\n").expect("write");
        writer.write_all(b"two\n").expect("write");

        assert_eq!(read(&scratch.path().join("cli.jsonl")), "one\ntwo\n");
        assert!(!scratch.path().join("cli.1.jsonl").exists());
    }

    #[test]
    fn rotations_beyond_max_files_drop_the_oldest() {
        let scratch = Scratch::new();
        let mut writer = RotatingWriter::new(scratch.path(), "tui", 10, 2);
        for n in 0..6 {
            writer
                .write_all(format!("line-{n}-xxx\n").as_bytes())
                .expect("write");
        }

        let active = read(&scratch.path().join("tui.jsonl"));
        let first = read(&scratch.path().join("tui.1.jsonl"));
        let second = read(&scratch.path().join("tui.2.jsonl"));
        assert_eq!(active, "line-5-xxx\n");
        assert_eq!(first, "line-4-xxx\n");
        assert_eq!(second, "line-3-xxx\n");
        assert!(!scratch.path().join("tui.3.jsonl").exists());
        let everything = format!("{active}{first}{second}");
        assert!(!everything.contains("line-0-"), "oldest line must be gone");
        assert!(!everything.contains("line-2-"), "older lines must be gone");
    }

    #[test]
    fn zero_max_files_keeps_only_the_active_file() {
        let scratch = Scratch::new();
        let mut writer = RotatingWriter::new(scratch.path(), "cli", 10, 0);
        writer.write_all(b"aaaaaaaa\n").expect("write");
        writer.write_all(b"bbbbbbbb\n").expect("write");

        assert_eq!(read(&scratch.path().join("cli.jsonl")), "bbbbbbbb\n");
        assert!(!scratch.path().join("cli.1.jsonl").exists());
    }

    #[test]
    fn a_line_larger_than_the_bound_is_written_whole() {
        let scratch = Scratch::new();
        let mut writer = RotatingWriter::new(scratch.path(), "cli", 8, 3);
        let big = "x".repeat(40) + "\n";
        writer.write_all(big.as_bytes()).expect("write");

        assert_eq!(read(&scratch.path().join("cli.jsonl")), big);
        assert!(!scratch.path().join("cli.1.jsonl").exists());
    }

    #[test]
    fn an_existing_file_is_appended_to_and_counted_towards_the_bound() {
        let scratch = Scratch::new();
        fs::write(scratch.path().join("cli.jsonl"), b"old-line-12345\n").expect("seed");
        let mut writer = RotatingWriter::new(scratch.path(), "cli", 20, 3);
        writer.write_all(b"new-line-12345\n").expect("write");

        assert_eq!(
            read(&scratch.path().join("cli.1.jsonl")),
            "old-line-12345\n"
        );
        assert_eq!(read(&scratch.path().join("cli.jsonl")), "new-line-12345\n");
    }

    #[test]
    fn an_uncreatable_directory_never_fails_the_caller_and_counts_the_loss() {
        let scratch = Scratch::new();
        // A regular file where the log directory's parent should be: creating
        // the directory beneath it cannot succeed on any platform.
        let blocker = scratch.path().join("blocker");
        fs::write(&blocker, b"not a directory").expect("create blocker");
        let mut writer = RotatingWriter::new(blocker.join("logs"), "cli", 1024, 5);

        assert_eq!(writer.write(b"lost-one\n").expect("write must succeed"), 9);
        assert_eq!(writer.write(b"lost-two\n").expect("write must succeed"), 9);
        writer.flush().expect("flush must succeed");
        assert_eq!(writer.lost(), 2);
    }

    #[test]
    fn a_directory_that_becomes_creatable_receives_the_next_write() {
        let scratch = Scratch::new();
        let blocker = scratch.path().join("blocker");
        fs::write(&blocker, b"not a directory").expect("create blocker");
        let logs = blocker.join("logs");
        let mut writer = RotatingWriter::new(&logs, "cli", 1024, 5);
        writer.write_all(b"dropped\n").expect("write must succeed");
        assert_eq!(writer.lost(), 1);

        fs::remove_file(&blocker).expect("remove blocker");
        writer.write_all(b"kept\n").expect("write");

        assert_eq!(read(&logs.join("cli.jsonl")), "kept\n");
        assert_eq!(
            writer.lost(),
            1,
            "a successful write must not add to the loss"
        );
    }

    #[test]
    fn a_rotation_that_cannot_complete_drops_the_line_and_is_retried() {
        let scratch = Scratch::new();
        let mut writer = RotatingWriter::new(scratch.path(), "cli", 10, 2);
        for line in ["aaaaaaaa\n", "bbbbbbbb\n", "cccccccc\n"] {
            writer.write_all(line.as_bytes()).expect("write");
        }
        // Both rotation slots are taken, so the next rotation must delete the
        // last one. A non-empty directory in its place cannot be removed as a
        // file, which makes the rotation fail.
        let last = scratch.path().join("cli.2.jsonl");
        fs::remove_file(&last).expect("remove last rotation");
        fs::create_dir(&last).expect("create squatter");
        fs::write(last.join("keep"), b"x").expect("fill squatter");
        writer.write_all(b"dddddddd\n").expect("write must succeed");
        assert_eq!(writer.lost(), 1);

        fs::remove_dir_all(&last).expect("remove squatter");
        writer.write_all(b"eeeeeeee\n").expect("write");

        // The retry finished the shift without discarding a file that had
        // already moved; only the dropped line is missing.
        assert_eq!(writer.lost(), 1);
        assert_eq!(read(&scratch.path().join("cli.2.jsonl")), "bbbbbbbb\n");
        assert_eq!(read(&scratch.path().join("cli.1.jsonl")), "cccccccc\n");
        assert_eq!(read(&scratch.path().join("cli.jsonl")), "eeeeeeee\n");
    }

    #[test]
    fn an_empty_write_is_a_no_op() {
        let scratch = Scratch::new();
        let mut writer = RotatingWriter::new(scratch.path(), "cli", 10, 2);
        assert_eq!(writer.write(b"").expect("write"), 0);
        assert!(!scratch.path().join("cli.jsonl").exists());
        assert_eq!(writer.lost(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn the_directory_and_the_file_are_private_to_their_owner() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new();
        let logs = scratch.path().join("logs");
        let mut writer = RotatingWriter::new(&logs, "cli", 1024, 2);
        writer.write_all(b"line\n").expect("write");

        let dir_mode = fs::metadata(&logs).expect("dir").permissions().mode() & 0o777;
        let file_mode = fs::metadata(logs.join("cli.jsonl"))
            .expect("file")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }
}
