// SPDX-License-Identifier: GPL-3.0-or-later
//! The browser's profile: where it is, whether the browser runs on it, and
//! its cookie database.

use crate::cookies::Cookies;
use crate::sqlite::{self, Db, Value};
use std::ffi::{c_int, c_short};
use std::fs::{self, File};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

pub const COOKIES: &str = "cookies.sqlite";
const COOKIES_WAL: &str = "cookies.sqlite-wal";

/// The profile folders of LibreWolf and Firefox: the classic ones, the
/// XDG ones of Firefox 147 and later, and LibreWolf's Flatpak.
fn roots(browser: &str) -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map_or_else(|| home.join(".config"), PathBuf::from);
    let librewolf = [
        home.join(".librewolf"),
        config.join("librewolf"),
        home.join(".var/app/io.gitlab.librewolf-community/.librewolf"),
    ];
    let firefox = [home.join(".mozilla/firefox"), config.join("mozilla/firefox")];
    let name = Path::new(browser).file_name().and_then(|n| n.to_str()).unwrap_or(browser);
    if name.contains("firefox") {
        firefox.to_vec()
    } else if name.contains("librewolf") {
        librewolf.to_vec()
    } else {
        librewolf.into_iter().chain(firefox).collect()
    }
}

/// The default profile in a profiles.ini: the one of the installation
/// ([Install...] Default=), else the one marked Default=1, else the only one.
pub fn default_profile(ini: &str, root: &Path) -> Option<PathBuf> {
    let mut install = None;
    let mut profiles = Vec::new(); // (path, relative, default)
    let mut section = String::new();
    for line in ini.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.to_string();
            if section.starts_with("Profile") {
                profiles.push((None, true, false));
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        if section.starts_with("Install") && key == "Default" && install.is_none() {
            install = Some(value.to_string());
        } else if section.starts_with("Profile") {
            let p = profiles.last_mut().unwrap();
            match key {
                "Path" => p.0 = Some(value.to_string()),
                "IsRelative" => p.1 = value == "1",
                "Default" => p.2 = value == "1",
                _ => {}
            }
        }
    }
    let path = |path: &str, relative: bool| if relative { root.join(path) } else { PathBuf::from(path) };
    if let Some(dir) = install {
        let relative = !dir.starts_with('/');
        return Some(path(&dir, relative));
    }
    let profiles: Vec<_> = profiles.into_iter().filter(|p| p.0.is_some()).collect();
    let chosen = profiles.iter().find(|p| p.2).or(if profiles.len() == 1 { profiles.first() } else { None })?;
    Some(path(chosen.0.as_deref().unwrap(), chosen.1))
}

pub fn find_profile(browser: &str) -> Result<PathBuf, String> {
    let roots = roots(browser);
    for root in &roots {
        if let Ok(ini) = fs::read_to_string(root.join("profiles.ini")) {
            return default_profile(&ini, root)
                .ok_or_else(|| format!("no default profile in {}/profiles.ini; give one with -p", root.display()));
        }
    }
    let tried: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
    Err(format!("no profiles.ini in {}; start {browser} once, or give the profile with -p", tried.join(", ")))
}

#[repr(C)]
struct Flock {
    l_type: c_short,
    l_whence: c_short,
    l_start: i64,
    l_len: i64,
    l_pid: c_int,
}

const F_GETLK: c_int = 5;
const F_WRLCK: c_short = 1;
const F_UNLCK: c_short = 2;

unsafe extern "C" {
    fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
}

/// Whether a browser runs on the profile: it holds a lock on .parentlock
/// (and points the `lock` symlink at its pid) for as long as it runs.
pub fn running(profile: &Path) -> bool {
    if let Ok(file) = File::open(profile.join(".parentlock")) {
        let mut lock = Flock { l_type: F_WRLCK, l_whence: 0, l_start: 0, l_len: 0, l_pid: 0 };
        if unsafe { fcntl(file.as_raw_fd(), F_GETLK, &mut lock as *mut Flock) } == 0 {
            return lock.l_type != F_UNLCK;
        }
    }
    // "127.0.1.1:+1234"
    fs::read_link(profile.join("lock"))
        .ok()
        .and_then(|target| target.to_str()?.rsplit_once(":+")?.1.parse::<u32>().ok())
        .is_some_and(|pid| Path::new(&format!("/proc/{pid}")).exists())
}

/// Size and modification time of the cookie database and its write-ahead
/// log: when they are the same, the cookies are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp(Vec<Option<(u64, i64, i64)>>);

pub fn stamp(profile: &Path) -> Stamp {
    Stamp(
        [COOKIES, COOKIES_WAL]
            .iter()
            .map(|f| fs::metadata(profile.join(f)).ok().map(|m| (m.len(), m.mtime(), m.mtime_nsec())))
            .collect(),
    )
}

/// The browser's cookies, `None` before it has a cookie database. The
/// browser keeps its database locked while it runs, so they are read from a
/// copy, taken again if the database changed while it was copied.
pub fn read(profile: &Path) -> Result<Option<Cookies>, String> {
    if !profile.join(COOKIES).exists() {
        return Ok(None);
    }
    let tmp = TempDir::new()?;
    for attempt in 0.. {
        let before = stamp(profile);
        copy(&profile.join(COOKIES), &tmp.0.join(COOKIES))?;
        let wal = profile.join(COOKIES_WAL);
        let _ = fs::remove_file(tmp.0.join(COOKIES_WAL));
        match fs::copy(&wal, tmp.0.join(COOKIES_WAL)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(format!("{}: {e}", wal.display())),
            _ => {}
        }
        if stamp(profile) == before {
            break;
        }
        if attempt == 5 {
            return Err(format!("{COOKIES} kept changing while it was copied"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let db = Db::open(&tmp.0.join(COOKIES), sqlite::OPEN_READWRITE)?;
    match db.value("PRAGMA quick_check")? {
        Value::Text(ok) if ok == b"ok" => {}
        _ => return Err(format!("the copy of {COOKIES} was damaged")),
    }
    let cookies = read_table(&db, "moz_cookies")?;
    // Before the copy goes, with its -shm file.
    drop(db);
    Ok(Some(cookies))
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    fs::copy(from, to).map(drop).map_err(|e| format!("{}: {e}", from.display()))
}

fn columns(db: &Db, table: &str) -> Result<Vec<String>, String> {
    let (_, rows) = db.query(&format!("PRAGMA table_info({})", sqlite::ident(table)), &[])?;
    // cid, name, type, notnull, dflt_value, pk
    Ok(rows.iter().map(|r| String::from_utf8_lossy(r[1].as_bytes()).into_owned()).collect())
}

/// The rows of a cookie table without its id, and the schema version of
/// the database.
pub fn read_table(db: &Db, table: &str) -> Result<Cookies, String> {
    let cols: Vec<String> = columns(db, table)?.into_iter().filter(|c| c != "id").collect();
    if cols.is_empty() {
        return Err(format!("no {table} table"));
    }
    let list: Vec<String> = cols.iter().map(|c| sqlite::ident(c)).collect();
    let (_, rows) = db.query(&format!("SELECT {} FROM {}", list.join(", "), sqlite::ident(table)), &[])?;
    let version = db.value("PRAGMA user_version")?.as_int().unwrap_or(0);
    Cookies::new(version, cols, rows)
}

pub enum Restored {
    Added(usize),
    /// The browser has never written its cookie database.
    NoDatabase,
    /// The saved cookies are from another version of the database.
    OtherVersion,
}

/// Puts saved cookies the browser does not have into its database. The
/// browser must not run.
pub fn restore(profile: &Path, saved: &Cookies) -> Result<Restored, String> {
    let path = profile.join(COOKIES);
    if saved.rows.is_empty() {
        return Ok(Restored::Added(0));
    }
    if !path.exists() {
        return Ok(Restored::NoDatabase);
    }
    let db = Db::open(&path, sqlite::OPEN_READWRITE)?;
    let version = db.value("PRAGMA user_version")?.as_int().unwrap_or(0);
    let cols = columns(&db, "moz_cookies")?;
    if version != saved.version || cols.iter().filter(|c| *c != "id").ne(saved.cols.iter()) {
        return Ok(Restored::OtherVersion);
    }
    let list: Vec<String> = saved.cols.iter().map(|c| sqlite::ident(c)).collect();
    let marks = vec!["?"; list.len()].join(", ");
    // A cookie the browser has (it did not delete it on exit) is newer
    // than the saved one.
    let sql = format!("INSERT OR IGNORE INTO moz_cookies ({}) VALUES ({marks}) RETURNING 1", list.join(", "));
    db.exec("BEGIN")?;
    let mut added = 0;
    for row in &saved.rows {
        match db.query(&sql, row) {
            Ok((_, r)) => added += r.len(),
            Err(e) => {
                let _ = db.exec("ROLLBACK");
                return Err(e);
            }
        }
    }
    db.exec("COMMIT")?;
    Ok(Restored::Added(added))
}

/// A private directory for the copies of the database, removed with what
/// it holds. In $XDG_RUNTIME_DIR (memory) when there is one.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<TempDir, String> {
        static N: AtomicU32 = AtomicU32::new(0);
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|d| !d.is_empty() && Path::new(d).is_dir())
            .map_or_else(std::env::temp_dir, PathBuf::from);
        let dir = base.join(format!("logincookies.{}.{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = fs::remove_dir_all(&dir);
        fs::DirBuilder::new().mode(0o700).create(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(TempDir(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_default_comes_first() {
        let ini = "[Install4F96D1932A9F858E]\nDefault=abcd.default-release\nLocked=1\n\n\
                   [Profile1]\nName=other\nIsRelative=1\nPath=efgh.other\nDefault=1\n\n\
                   [Profile0]\nName=default-release\nIsRelative=1\nPath=abcd.default-release\n";
        assert_eq!(default_profile(ini, Path::new("/h/.librewolf")), Some("/h/.librewolf/abcd.default-release".into()));
    }

    #[test]
    fn marked_or_only_profile() {
        let ini = "[General]\nStartWithLastProfile=1\n\n[Profile0]\nName=a\nIsRelative=1\nPath=a.a\n\n\
                   [Profile1]\nName=b\nIsRelative=0\nPath=/data/b\nDefault=1\n";
        assert_eq!(default_profile(ini, Path::new("/r")), Some("/data/b".into()));
        let ini = "[Profile0]\nName=a\nIsRelative=1\nPath=a.a\n";
        assert_eq!(default_profile(ini, Path::new("/r")), Some("/r/a.a".into()));
        let ini = "[Profile0]\nPath=a\n[Profile1]\nPath=b\n";
        assert_eq!(default_profile(ini, Path::new("/r")), None);
    }

    #[test]
    fn locked_profile_is_running() {
        let dir = TempDir::new().unwrap();
        assert!(!running(&dir.0));
        fs::write(dir.0.join(".parentlock"), "").unwrap();
        assert!(!running(&dir.0));
        // Another process holds an fcntl() lock on it, as the browser does.
        let lock = "import fcntl, sys, time; f = open(sys.argv[1], 'w'); fcntl.lockf(f, fcntl.LOCK_EX); time.sleep(5)";
        let Ok(mut child) = std::process::Command::new("python3")
            .args(["-c", lock])
            .arg(dir.0.join(".parentlock"))
            .spawn()
        else {
            return; // no python3 to hold the lock
        };
        let mut seen = false;
        for _ in 0..50 {
            if running(&dir.0) {
                seen = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = child.kill();
        let _ = child.wait();
        assert!(seen);
    }
}
