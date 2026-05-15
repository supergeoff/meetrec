use std::io;
use std::path::{Path, PathBuf};

/// Returns the canonical path for a session directory (does not create it).
pub fn session_dir_path(base: &Path, stamp: &str) -> PathBuf {
    base.join(format!("meeting_{stamp}"))
}

/// Creates `<base>/meeting_<stamp>/` and returns its path.
///
/// If the directory already exists (same-second collision), appends `-2`, `-3`, …
pub fn create_session_dir(base: &Path, stamp: &str) -> io::Result<PathBuf> {
    let primary = session_dir_path(base, stamp);
    if try_create_dir(&primary)? {
        return Ok(primary);
    }
    let mut n = 2u32;
    loop {
        let candidate = base.join(format!("meeting_{stamp}-{n}"));
        if try_create_dir(&candidate)? {
            return Ok(candidate);
        }
        n += 1;
        if n > 999 {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "too many session directory collisions",
            ));
        }
    }
}

/// Returns `true` if the directory was created, `false` if it already existed.
fn try_create_dir(path: &Path) -> io::Result<bool> {
    match std::fs::create_dir(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tmp_base() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir()
            .join(format!("meetrec_session_{}", N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn session_dir_path_format() {
        let base = PathBuf::from("/recordings");
        assert_eq!(
            session_dir_path(&base, "20260515_092200"),
            PathBuf::from("/recordings/meeting_20260515_092200")
        );
    }

    #[test]
    fn creates_directory_on_first_call() {
        let base = tmp_base();
        let result = create_session_dir(&base, "20260515_092200").unwrap();
        assert_eq!(result.file_name().unwrap(), "meeting_20260515_092200");
        assert!(result.is_dir());
    }

    #[test]
    fn deduplicates_on_collision() {
        let base = tmp_base();
        let stamp = "20260515_101030";

        let first = create_session_dir(&base, stamp).unwrap();
        assert_eq!(first, base.join("meeting_20260515_101030"));

        let second = create_session_dir(&base, stamp).unwrap();
        assert_eq!(second, base.join("meeting_20260515_101030-2"));

        let third = create_session_dir(&base, stamp).unwrap();
        assert_eq!(third, base.join("meeting_20260515_101030-3"));
    }

    #[test]
    fn three_files_all_land_in_session_dir() {
        let base = tmp_base();
        let dir = create_session_dir(&base, "20260515_120000").unwrap();
        assert_eq!(dir.join("recording.mp3").parent().unwrap(), dir);
        assert_eq!(dir.join("transcript.txt").parent().unwrap(), dir);
        assert_eq!(dir.join("summary.md").parent().unwrap(), dir);
    }
}
