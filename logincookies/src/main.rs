// SPDX-License-Identifier: GPL-3.0-or-later
//! logincookies: keeps the login cookies of LibreWolf (or Firefox) when the
//! browser deletes its cookies on exit, and only those.
//!
//! It starts the browser. While the browser runs, the cookies whose names
//! look like the ones of a login (session, auth, token, SID...) are saved
//! apart, every time the cookie database changes. The next time, before
//! the browser starts, they are put back into its database. The other
//! cookies stay deleted.

/// eprintln! that ignores errors: logincookies often runs without a
/// terminal, and with panic = "abort" a closed stderr would kill it.
macro_rules! say {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

mod browser;
mod cookies;
mod sqlite;
mod store;

use browser::Restored;
use cookies::{Cookies, Rules};
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, exit};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use store::Store;

const USAGE: &str = "usage: logincookies [-v] [-b BROWSER] [-p PROFILE] [--] [ARGS...]
       logincookies [-b BROWSER] [-p PROFILE] list [-a]
       logincookies [-b BROWSER] [-p PROFILE] forget SITE...
  (no command)  put the saved login cookies back, start the browser with ARGS
                and save its login cookies until it exits
  list          the saved login cookies; -a: all the cookies the browser has
                now, login cookies marked with *
  forget SITE   delete the saved cookies of SITE (www.naver.com, or its URL):
                the ones it gets and the ones of its subdomains
  -b BROWSER    the browser command (librewolf; firefox works the same)
  -p PROFILE    the profile folder (default: the browser's default profile)
  -v            print the cookies it saves and puts back";

/// How often the cookie database is checked while the browser runs.
const POLL: Duration = Duration::from_secs(2);

struct Opts {
    browser: String,
    profile: Option<PathBuf>,
    verbose: bool,
}

impl Opts {
    fn profile(&self) -> Result<PathBuf, String> {
        let dir = match &self.profile {
            Some(dir) => dir.clone(),
            None => browser::find_profile(&self.browser)?,
        };
        std::fs::canonicalize(&dir).map_err(|e| format!("profile {}: {e}", dir.display()))
    }
}

fn usage() -> ! {
    say!("{USAGE}");
    exit(2)
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// $XDG_CONFIG_HOME/logincookies/rules, if there is one.
fn rules() -> Result<Rules, String> {
    let config = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()).map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"),
        PathBuf::from,
    );
    let path = config.join("logincookies/rules");
    match std::fs::read_to_string(&path) {
        Ok(text) => Rules::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Rules::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// The saved cookies that are still login cookies and not expired.
fn usable(saved: &Cookies, rules: &Rules) -> Cookies {
    let now = now_ms();
    let mut out = saved.clone();
    out.rows = saved.rows.iter().filter(|r| saved.is_login(r, rules) && !saved.expired(r, now)).cloned().collect();
    out
}

/// "+ .google.com SID" for each cookie in `new` and not in `old`, "-" for
/// the other way.
fn changes(old: Option<&Cookies>, new: &Cookies) -> Vec<String> {
    let names = |c: &Cookies| -> HashSet<(String, String)> {
        c.rows.iter().map(|r| (c.host(r), c.name(r))).collect()
    };
    let (old, new) = (old.map(names).unwrap_or_default(), names(new));
    let mut lines: Vec<String> = new.difference(&old).map(|(h, n)| format!("+ {h} {n}")).collect();
    lines.extend(old.difference(&new).map(|(h, n)| format!("- {h} {n}")));
    lines.sort();
    lines
}

fn put_back(opts: &Opts, profile: &std::path::Path, store: &Store, rules: &Rules) {
    let result = (|| {
        let _lock = store.lock()?;
        match store.read()? {
            Some(saved) => browser::restore(profile, &usable(&saved, rules)).map(Some),
            None => Ok(None),
        }
    })();
    match result {
        Ok(Some(Restored::Added(n))) if opts.verbose && n > 0 => say!("logincookies: put back {n} login cookies"),
        Ok(Some(Restored::NoDatabase)) if opts.verbose => {
            say!("logincookies: the browser has no {} yet, nothing put back", browser::COOKIES)
        }
        Ok(Some(Restored::OtherVersion)) => say!(
            "logincookies: the saved login cookies are from another version of {}: \
             not put back, log in again once",
            browser::COOKIES
        ),
        Err(e) => say!("logincookies: putting back the login cookies: {e}"),
        _ => {}
    }
}

/// Saves the login cookies the browser has, reporting each error once.
struct Saver<'a> {
    profile: &'a std::path::Path,
    store: &'a Store,
    rules: &'a Rules,
    verbose: bool,
    error: Option<String>,
}

impl Saver<'_> {
    fn save(&mut self) -> bool {
        match self.try_save() {
            Ok(lines) => {
                for line in lines.iter().filter(|_| self.verbose) {
                    say!("logincookies: {line}");
                }
                self.error = None;
                true
            }
            Err(e) => {
                if self.error.as_ref() != Some(&e) {
                    say!("logincookies: {e}");
                }
                self.error = Some(e);
                false
            }
        }
    }

    fn try_save(&self) -> Result<Vec<String>, String> {
        let Some(browser) = browser::read(self.profile)? else { return Ok(Vec::new()) };
        let _lock = self.store.lock()?;
        let saved = self.store.read()?;
        let merged = cookies::merge(saved.as_ref(), &browser, self.rules, now_ms());
        let unchanged = match &saved {
            Some(saved) => saved.same_as(&merged),
            None => merged.rows.is_empty(),
        };
        if unchanged {
            return Ok(Vec::new());
        }
        self.store.write(&merged)?;
        Ok(changes(saved.as_ref(), &merged))
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

fn run(opts: &Opts, args: Vec<OsString>) -> Result<i32, String> {
    let profile = opts.profile()?;
    let store = Store::new(&profile)?;
    let watch = store.try_watch()?;
    // Read once: a mistake in them should not stop the cookies being saved.
    let rules = rules().unwrap_or_else(|e| {
        if watch.is_some() {
            say!("logincookies: {e}\n  going by the cookie names alone");
        }
        Rules::default()
    });
    if watch.is_some() && !browser::running(&profile) {
        put_back(opts, &profile, &store, &rules);
    }
    let mut cmd = Command::new(&opts.browser);
    if let Some(dir) = &opts.profile {
        cmd.arg("--profile").arg(dir);
    }
    let mut child = cmd.args(&args).spawn().map_err(|e| format!("{}: {e}", opts.browser))?;
    if watch.is_none() {
        // Another logincookies saves the cookies of this profile.
        return child.wait().map(exit_code).map_err(|e| e.to_string());
    }
    let mut saver = Saver { profile: &profile, store: &store, rules: &rules, verbose: opts.verbose, error: None };
    let mut last = None;
    let mut status = None;
    loop {
        std::thread::sleep(POLL);
        if status.is_none() {
            status = child.try_wait().map_err(|e| e.to_string())?;
        }
        // The command may only have asked a running browser to open a
        // window, so its end is not the browser's.
        let done = status.is_some() && !browser::running(&profile);
        let stamp = browser::stamp(&profile);
        if last.as_ref() != Some(&stamp) && saver.save() {
            last = Some(stamp);
        }
        if done {
            return Ok(status.map_or(0, exit_code));
        }
    }
}

fn list(opts: &Opts, all: bool) -> Result<i32, String> {
    let profile = opts.profile()?;
    let rules = rules()?;
    let cookies = if all {
        match browser::read(&profile)? {
            Some(cookies) => cookies,
            None => return Ok(0),
        }
    } else {
        let store = Store::new(&profile)?;
        let _lock = store.lock()?;
        match store.read()? {
            Some(saved) => usable(&saved, &rules),
            None => return Ok(0),
        }
    };
    let mut rows: Vec<(String, String, String, bool)> = cookies
        .rows
        .iter()
        .map(|r| {
            let host = cookies.host(r).trim_start_matches('.').to_string();
            let expires = cookies.expiry_ms(r).map_or_else(String::new, cookies::date);
            (host, cookies.name(r), expires, cookies.is_login(r, &rules))
        })
        .collect();
    rows.sort();
    rows.dedup();
    let width = rows.iter().map(|r| r.0.len()).max().unwrap_or(0).min(40);
    for (host, name, expires, login) in rows {
        let mark = if !all { "" } else if login { "* " } else { "  " };
        println!("{mark}{host:width$}  {expires}  {name}");
    }
    Ok(0)
}

fn forget(opts: &Opts, sites: &[OsString]) -> Result<i32, String> {
    if sites.is_empty() {
        usage();
    }
    let sites: Vec<String> = sites.iter().map(|s| cookies::normalize_site(&s.to_string_lossy())).collect();
    let profile = opts.profile()?;
    let store = Store::new(&profile)?;
    let _lock = store.lock()?;
    let Some(mut saved) = store.read()? else { return Ok(0) };
    let before = saved.rows.len();
    let rows = saved.rows.iter().filter(|r| !sites.iter().any(|s| cookies::sent_to(&saved.host(r), s))).cloned();
    saved.rows = rows.collect();
    let n = before - saved.rows.len();
    if n > 0 {
        store.write(&saved)?;
    }
    println!("forgot {n} cookies");
    if n > 0 && browser::running(&profile) {
        say!(
            "logincookies: the browser runs: its cookies of {} are saved again unless you log out there first",
            sites.join(", ")
        );
    }
    Ok(0)
}

fn main() {
    let mut args = std::env::args_os().skip(1).peekable();
    let mut opts = Opts { browser: "librewolf".into(), profile: None, verbose: false };
    let mut command = true;
    while let Some(arg) = args.peek().and_then(|a| a.to_str()) {
        match arg {
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            "-v" => opts.verbose = true,
            "-b" => {
                args.next();
                opts.browser = args.peek().and_then(|a| a.to_str()).unwrap_or_else(|| usage()).to_string();
            }
            "-p" => {
                args.next();
                opts.profile = Some(args.peek().unwrap_or_else(|| usage()).into());
            }
            "--" => {
                command = false;
                args.next();
                break;
            }
            _ => break,
        }
        args.next();
    }
    let rest: Vec<OsString> = args.collect();
    let first = rest.first().and_then(|a| a.to_str()).filter(|_| command);
    let result = match first {
        Some("list") => match rest[1..].iter().map(|a| a.to_str()).collect::<Vec<_>>()[..] {
            [] => list(&opts, false),
            [Some("-a")] => list(&opts, true),
            _ => usage(),
        },
        Some("forget") => forget(&opts, &rest[1..]),
        _ => run(&opts, rest),
    };
    match result {
        Ok(code) => exit(code),
        Err(e) => {
            say!("logincookies: {e}");
            exit(1)
        }
    }
}
