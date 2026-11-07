// SPDX-License-Identifier: GPL-3.0-or-later
//! The saved login cookies of a profile: an SQLite file in
//! $XDG_DATA_HOME/logincookies, readable by the user only, with a table of
//! the same columns as the browser's moz_cookies.

use crate::browser;
use crate::cookies::Cookies;
use crate::sqlite::{self, Db};
use std::ffi::c_int;
use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const LOCK_EX: c_int = 2;
const LOCK_NB: c_int = 4;

unsafe extern "C" {
    fn flock(fd: c_int, op: c_int) -> c_int;
}

pub struct Store {
    pub path: PathBuf,
    lock: PathBuf,
    watch: PathBuf,
}

fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()).map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"),
        PathBuf::from,
    )
}

impl Store {
    pub fn new(profile: &Path) -> Result<Store, String> {
        let dir = data_dir().join("logincookies");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .and_then(|()| fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)))
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = profile.file_name().map_or_else(|| "profile".into(), |n| n.to_string_lossy().into_owned());
        let file = |ext: &str| dir.join(format!("{name}.{ext}"));
        Ok(Store { path: file("sqlite"), lock: file("lock"), watch: file("watch") })
    }

    fn open_lock(path: &Path) -> Result<File, String> {
        OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Held while the saved cookies are read and written.
    pub fn lock(&self) -> Result<File, String> {
        let file = Self::open_lock(&self.lock)?;
        if unsafe { flock(file.as_raw_fd(), LOCK_EX) } != 0 {
            return Err(format!("{}: {}", self.lock.display(), std::io::Error::last_os_error()));
        }
        Ok(file)
    }

    /// Held by the one logincookies that saves the cookies of the profile
    /// while the browser runs. `None` when another one holds it.
    pub fn try_watch(&self) -> Result<Option<File>, String> {
        let file = Self::open_lock(&self.watch)?;
        Ok((unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } == 0).then_some(file))
    }

    pub fn read(&self) -> Result<Option<Cookies>, String> {
        if !self.path.exists() {
            return Ok(None);
        }
        let db = Db::open(&self.path, sqlite::OPEN_READWRITE)?;
        browser::read_table(&db, "cookies").map(Some).map_err(|e| format!("{}: {e}", self.path.display()))
    }

    /// Replaces the saved cookies, all at once.
    pub fn write(&self, cookies: &Cookies) -> Result<(), String> {
        let tmp = self.path.with_extension("new");
        let err = |e: String| format!("{}: {e}", tmp.display());
        let _ = fs::remove_file(&tmp);
        OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).map_err(|e| err(e.to_string()))?;
        {
            let db = Db::open(&tmp, sqlite::OPEN_READWRITE).map_err(err)?;
            let cols: Vec<String> = cookies.cols.iter().map(|c| sqlite::ident(c)).collect();
            let marks = vec!["?"; cols.len()].join(", ");
            let insert = format!("INSERT INTO cookies ({}) VALUES ({marks})", cols.join(", "));
            let result = (|| {
                db.exec(&format!("PRAGMA user_version = {}", cookies.version))?;
                db.exec("BEGIN")?;
                db.exec(&format!("CREATE TABLE cookies ({})", cols.join(", ")))?;
                for row in &cookies.rows {
                    db.query(&insert, row)?;
                }
                db.exec("COMMIT")
            })();
            result.map_err(err)?;
        }
        fs::rename(&tmp, &self.path).map_err(|e| err(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::Value;

    #[test]
    fn writes_and_reads_back() {
        let dir = std::env::temp_dir().join(format!("logincookies-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let store = Store { path: dir.join("p.sqlite"), lock: dir.join("p.lock"), watch: dir.join("p.watch") };
        assert!(store.read().unwrap().is_none());
        let cols = ["originAttributes", "name", "value", "host", "path", "expiry", "isSecure"];
        let rows = vec![vec![
            Value::text(""),
            Value::text("SID"),
            Value::Text(vec![0xff, b'"', b'\'', 0]),
            Value::text(".google.com"),
            Value::text("/"),
            Value::Int(1_900_000_000_000),
            Value::Int(1),
        ]];
        let cookies = Cookies::new(16, cols.map(String::from).to_vec(), rows).unwrap();
        store.write(&cookies).unwrap();
        let back = store.read().unwrap().unwrap();
        assert!(back.same_as(&cookies));
        assert_eq!(fs::metadata(&store.path).unwrap().permissions().mode() & 0o777, 0o600);
        let _watch = store.try_watch().unwrap().unwrap();
        assert!(store.try_watch().unwrap().is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
