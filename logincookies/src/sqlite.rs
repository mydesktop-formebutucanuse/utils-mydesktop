// SPDX-License-Identifier: GPL-3.0-or-later
//! The few calls of libsqlite3 that logincookies needs.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

#[repr(C)]
struct Sqlite3 {
    _private: [u8; 0],
}

#[repr(C)]
struct Stmt {
    _private: [u8; 0],
}

const OK: c_int = 0;
const ROW: c_int = 100;
const DONE: c_int = 101;
const INTEGER: c_int = 1;
const FLOAT: c_int = 2;
const TEXT: c_int = 3;
const BLOB: c_int = 4;
/// SQLITE_TRANSIENT: sqlite copies bound text and blobs.
const TRANSIENT: isize = -1;

pub const OPEN_READWRITE: c_int = 0x02;

#[link(name = "sqlite3")]
unsafe extern "C" {
    fn sqlite3_open_v2(name: *const c_char, db: *mut *mut Sqlite3, flags: c_int, vfs: *const c_char) -> c_int;
    fn sqlite3_close_v2(db: *mut Sqlite3) -> c_int;
    fn sqlite3_errmsg(db: *mut Sqlite3) -> *const c_char;
    fn sqlite3_busy_timeout(db: *mut Sqlite3, ms: c_int) -> c_int;
    fn sqlite3_prepare_v2(
        db: *mut Sqlite3,
        sql: *const c_char,
        len: c_int,
        stmt: *mut *mut Stmt,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_step(stmt: *mut Stmt) -> c_int;
    fn sqlite3_finalize(stmt: *mut Stmt) -> c_int;
    fn sqlite3_column_count(stmt: *mut Stmt) -> c_int;
    fn sqlite3_column_name(stmt: *mut Stmt, i: c_int) -> *const c_char;
    fn sqlite3_column_type(stmt: *mut Stmt, i: c_int) -> c_int;
    fn sqlite3_column_int64(stmt: *mut Stmt, i: c_int) -> i64;
    fn sqlite3_column_double(stmt: *mut Stmt, i: c_int) -> f64;
    fn sqlite3_column_blob(stmt: *mut Stmt, i: c_int) -> *const c_void;
    fn sqlite3_column_bytes(stmt: *mut Stmt, i: c_int) -> c_int;
    fn sqlite3_bind_int64(stmt: *mut Stmt, i: c_int, v: i64) -> c_int;
    fn sqlite3_bind_double(stmt: *mut Stmt, i: c_int, v: f64) -> c_int;
    fn sqlite3_bind_text(stmt: *mut Stmt, i: c_int, p: *const c_char, n: c_int, free: isize) -> c_int;
    fn sqlite3_bind_blob(stmt: *mut Stmt, i: c_int, p: *const c_void, n: c_int, free: isize) -> c_int;
}

/// A column value. Text is kept as bytes: cookie values need not be UTF-8.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

impl Value {
    #[cfg(test)]
    pub fn text(s: &str) -> Value {
        Value::Text(s.as_bytes().to_vec())
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Value::Text(b) | Value::Blob(b) => b,
            _ => &[],
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match *self {
            Value::Int(i) => Some(i),
            Value::Real(f) => Some(f as i64),
            _ => None,
        }
    }
}

/// Column names and rows of a query.
pub type Rows = (Vec<String>, Vec<Vec<Value>>);

pub struct Db(*mut Sqlite3);

impl Db {
    pub fn open(path: &Path, flags: c_int) -> Result<Db, String> {
        let mut name = path.as_os_str().as_bytes().to_vec();
        name.push(0);
        let mut db = ptr::null_mut();
        let rc = unsafe { sqlite3_open_v2(name.as_ptr() as *const c_char, &mut db, flags, ptr::null()) };
        let db = Db(db);
        if rc != OK {
            return Err(format!("{}: {}", path.display(), db.error()));
        }
        unsafe { sqlite3_busy_timeout(db.0, 2000) };
        Ok(db)
    }

    fn error(&self) -> String {
        if self.0.is_null() {
            return "out of memory".into();
        }
        unsafe { CStr::from_ptr(sqlite3_errmsg(self.0)) }.to_string_lossy().into_owned()
    }

    /// Runs one statement and returns what it selects.
    pub fn query(&self, sql: &str, params: &[Value]) -> Result<Rows, String> {
        let mut stmt = ptr::null_mut();
        let rc = unsafe {
            sqlite3_prepare_v2(self.0, sql.as_ptr() as *const c_char, sql.len() as c_int, &mut stmt, ptr::null_mut())
        };
        if rc != OK {
            return Err(self.error());
        }
        let result = self.run(stmt, params);
        unsafe { sqlite3_finalize(stmt) };
        result
    }

    fn run(&self, stmt: *mut Stmt, params: &[Value]) -> Result<Rows, String> {
        for (i, v) in params.iter().enumerate() {
            let i = i as c_int + 1;
            let rc = unsafe {
                match v {
                    Value::Null => OK,
                    Value::Int(n) => sqlite3_bind_int64(stmt, i, *n),
                    Value::Real(f) => sqlite3_bind_double(stmt, i, *f),
                    Value::Text(b) => {
                        sqlite3_bind_text(stmt, i, b.as_ptr() as *const c_char, b.len() as c_int, TRANSIENT)
                    }
                    Value::Blob(b) => {
                        sqlite3_bind_blob(stmt, i, b.as_ptr() as *const c_void, b.len() as c_int, TRANSIENT)
                    }
                }
            };
            if rc != OK {
                return Err(self.error());
            }
        }
        let n = unsafe { sqlite3_column_count(stmt) };
        let cols = (0..n)
            .map(|i| unsafe { CStr::from_ptr(sqlite3_column_name(stmt, i)) }.to_string_lossy().into_owned())
            .collect();
        let mut rows = Vec::new();
        loop {
            match unsafe { sqlite3_step(stmt) } {
                ROW => rows.push((0..n).map(|i| unsafe { column(stmt, i) }).collect()),
                DONE => return Ok((cols, rows)),
                _ => return Err(self.error()),
            }
        }
    }

    pub fn exec(&self, sql: &str) -> Result<(), String> {
        self.query(sql, &[]).map(drop)
    }

    /// The first value selected by `sql`.
    pub fn value(&self, sql: &str) -> Result<Value, String> {
        let (_, rows) = self.query(sql, &[])?;
        Ok(rows.into_iter().next().and_then(|r| r.into_iter().next()).unwrap_or(Value::Null))
    }
}

/// SAFETY: `stmt` has a row and `i` is one of its columns.
unsafe fn column(stmt: *mut Stmt, i: c_int) -> Value {
    unsafe {
        match sqlite3_column_type(stmt, i) {
            INTEGER => Value::Int(sqlite3_column_int64(stmt, i)),
            FLOAT => Value::Real(sqlite3_column_double(stmt, i)),
            t @ (TEXT | BLOB) => {
                let p = sqlite3_column_blob(stmt, i) as *const u8;
                let n = sqlite3_column_bytes(stmt, i) as usize;
                let bytes = if p.is_null() { Vec::new() } else { std::slice::from_raw_parts(p, n).to_vec() };
                if t == TEXT { Value::Text(bytes) } else { Value::Blob(bytes) }
            }
            _ => Value::Null,
        }
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        unsafe { sqlite3_close_v2(self.0) };
    }
}

/// `name` quoted as an SQL identifier.
pub fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
