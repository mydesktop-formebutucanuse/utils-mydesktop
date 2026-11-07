// SPDX-License-Identifier: GPL-3.0-or-later
//! Which cookies are login cookies, and which of them to keep.

use crate::sqlite::Value;
use std::collections::{HashMap, HashSet};

/// Lower-case parts of names that mark login cookies anywhere in the name:
/// PHPSESSID, user_session, auth_token, LOGIN_INFO, remember_web_...
const LOGIN_PARTS: &[&str] = &["sess", "auth", "token", "login", "logged", "remember", "jwt", "acct", "account"];
/// Lower-case words that mark login cookies when no letter touches them:
/// connect.sid, c_user, ds_user_id, but not sidebar or username.
const LOGIN_WORDS: &[&str] = &["sid", "user"];
/// Login cookies of big sites that the parts and words miss, lower case.
const LOGIN_NAMES: &[&str] = &[
    "nid_aut", "nid_ses", "nid_jkl", // Naver
    "_kawlt", "_kawltea", "_karmt", "_karmtea", // Kakao
    "xs", // Facebook (with c_user)
    "ct0", "twid", "kdt", // X
    "li_at", // LinkedIn
    "myacinfo", // Apple
    "sp_dc", "sp_key", // Spotify
    "netflixid", "securenetflixid",
    "at-main", "x-main", "ubid-main", // Amazon
    "__host-gaps", // Google
];
const LOGIN_PREFIXES: &[&str] = &["wordpress_"];
/// Analytics and tracking cookies whose names look like login cookies.
const NOT_LOGIN_PREFIXES: &[&str] = &[
    "_ga", "_gid", "__utm", "_hj", "_pin_", "__stripe_", "ajs_", "amp_", "optimizely", "intercom-",
];
const NOT_LOGIN_NAMES: &[&str] = &["s_sess"];

/// Whether a word occurs in `s` with no ASCII letter right before or after.
fn has_word(s: &str, word: &str) -> bool {
    s.match_indices(word).any(|(i, _)| {
        let before = s[..i].bytes().next_back();
        let after = s[i + word.len()..].bytes().next();
        !before.is_some_and(|c| c.is_ascii_alphabetic()) && !after.is_some_and(|c| c.is_ascii_alphabetic())
    })
}

/// Whether the name of a cookie looks like the one of a login cookie.
pub fn guess(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if NOT_LOGIN_NAMES.contains(&lower.as_str()) || NOT_LOGIN_PREFIXES.iter().any(|p| lower.starts_with(p)) {
        return false;
    }
    LOGIN_NAMES.contains(&lower.as_str())
        || LOGIN_PREFIXES.iter().any(|p| lower.starts_with(p))
        || LOGIN_PARTS.iter().any(|p| lower.contains(p))
        || LOGIN_WORDS.iter().any(|w| has_word(&lower, w))
        // Google: SID, HSID, APISID, __Secure-3PSID, SIDCC...
        || name.contains("SID")
}

/// `*` matches any characters. Both are lower case.
fn glob(pattern: &str, s: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or("");
    let Some(mut rest) = s.strip_prefix(first) else { return false };
    let mut parts: Vec<&str> = parts.collect();
    let Some(last) = parts.pop() else { return rest.is_empty() };
    for part in parts {
        match rest.find(part) {
            Some(i) => rest = &rest[i + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

/// Whether a cookie host (`.naver.com`, `nid.naver.com`) is `site` or
/// one of its subdomains. `site` is lower case.
pub fn in_site(host: &str, site: &str) -> bool {
    let host = host.trim_start_matches('.').to_ascii_lowercase();
    host == site || host.strip_suffix(site).is_some_and(|h| h.ends_with('.'))
}

/// Whether a cookie of `host` belongs to `site` (lower case): the site gets
/// it (`.naver.com` for `www.naver.com`), or it is one of a subdomain.
pub fn sent_to(host: &str, site: &str) -> bool {
    in_site(host, site) || in_site(site, &host.trim_start_matches('.').to_ascii_lowercase())
}

/// A site as given by the user: `naver.com`, `.naver.com`, `https://www.naver.com/`.
pub fn normalize_site(s: &str) -> String {
    let s = s.trim().to_ascii_lowercase();
    let s = s.split_once("://").map_or(s.as_str(), |(_, rest)| rest);
    let s = s.split(['/', '?', '#']).next().unwrap_or("");
    s.trim_start_matches('.').to_string()
}

enum What {
    Name(String),
    Site(String),
}

struct Rule {
    keep: bool,
    what: What,
}

/// The user's rules, which come before the guess.
#[derive(Default)]
pub struct Rules(Vec<Rule>);

impl Rules {
    /// One rule a line: NAME, !NAME, @SITE or !@SITE. `#` starts a comment.
    pub fn parse(text: &str) -> Result<Rules, String> {
        let mut rules = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line.contains(char::is_whitespace) {
                return Err(format!("line {}: one name or site a line: {line}", i + 1));
            }
            let (keep, rest) = match line.strip_prefix('!') {
                Some(rest) => (false, rest),
                None => (true, line),
            };
            let what = match rest.strip_prefix('@') {
                Some(site) => What::Site(normalize_site(site)),
                None => What::Name(rest.to_ascii_lowercase()),
            };
            if matches!(&what, What::Name(s) | What::Site(s) if s.is_empty()) {
                return Err(format!("line {}: nothing after {line}", i + 1));
            }
            rules.push(Rule { keep, what });
        }
        Ok(Rules(rules))
    }

    /// Whether the cookie `name` of `host` is a login cookie: the rules
    /// that say no come first, then the ones that say yes, then the guess.
    pub fn is_login(&self, name: &str, host: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        let applies = |r: &Rule| match &r.what {
            What::Name(pattern) => glob(pattern, &lower),
            What::Site(site) => in_site(host, site),
        };
        if self.0.iter().any(|r| !r.keep && applies(r)) {
            return false;
        }
        self.0.iter().any(|r| r.keep && applies(r)) || guess(name)
    }
}

/// Expiry in milliseconds. The cookie database had seconds before
/// Firefox switched it to milliseconds; no cookie lives until the year 5138.
fn expiry_ms(v: i64) -> i64 {
    if v > 100_000_000_000 { v } else { v.saturating_mul(1000) }
}

/// Columns that change without the cookie changing.
const TIME_COLUMNS: &[&str] = &["lastAccessed", "updateTime"];

/// Rows of a moz_cookies table without its id column, as the browser has
/// them or as saved, with the schema version of the browser's database.
#[derive(Clone, Debug)]
pub struct Cookies {
    pub version: i64,
    pub cols: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    name: usize,
    host: usize,
    path: usize,
    expiry: usize,
    origin_attributes: Option<usize>,
}

type Key = (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>);

impl Cookies {
    pub fn new(version: i64, cols: Vec<String>, rows: Vec<Vec<Value>>) -> Result<Cookies, String> {
        let col = |name: &str| cols.iter().position(|c| c == name);
        let need = |name: &str| col(name).ok_or_else(|| format!("unknown cookie database: no {name} column"));
        Ok(Cookies {
            name: need("name")?,
            host: need("host")?,
            path: need("path")?,
            expiry: need("expiry")?,
            origin_attributes: col("originAttributes"),
            version,
            cols,
            rows,
        })
    }

    fn empty_like(other: &Cookies) -> Cookies {
        Cookies { rows: Vec::new(), ..other.clone() }
    }

    pub fn name(&self, row: &[Value]) -> String {
        String::from_utf8_lossy(row[self.name].as_bytes()).into_owned()
    }

    pub fn host(&self, row: &[Value]) -> String {
        String::from_utf8_lossy(row[self.host].as_bytes()).into_owned()
    }

    pub fn expiry_ms(&self, row: &[Value]) -> Option<i64> {
        row[self.expiry].as_int().map(expiry_ms)
    }

    fn origin_attributes<'a>(&self, row: &'a [Value]) -> &'a [u8] {
        self.origin_attributes.map_or(&[], |i| row[i].as_bytes())
    }

    /// Name, host, path and origin attributes: what makes a cookie
    /// another cookie (moz_uniqueid).
    fn key(&self, row: &[Value]) -> Key {
        let b = |i: usize| row[i].as_bytes().to_vec();
        (b(self.name), b(self.host), b(self.path), self.origin_attributes(row).to_vec())
    }

    pub fn is_login(&self, row: &[Value], rules: &Rules) -> bool {
        // A third-party cookie, kept apart for one site (partitioned): a
        // login is with the site itself.
        let partitioned = self.origin_attributes(row).windows(13).any(|w| w == b"partitionKey=");
        !partitioned && rules.is_login(&self.name(row), &self.host(row))
    }

    pub fn expired(&self, row: &[Value], now_ms: i64) -> bool {
        self.expiry_ms(row).is_some_and(|e| e <= now_ms)
    }

    /// Whether the same cookies have the same values, expiry etc., leaving
    /// out when they were last used.
    pub fn same_as(&self, other: &Cookies) -> bool {
        if self.version != other.version || self.cols != other.cols || self.rows.len() != other.rows.len() {
            return false;
        }
        let skip: Vec<usize> =
            TIME_COLUMNS.iter().filter_map(|t| self.cols.iter().position(|c| c == t)).collect();
        self.rows.iter().zip(&other.rows).all(|(a, b)| {
            a.iter().zip(b).enumerate().all(|(i, (x, y))| skip.contains(&i) || x == y)
        })
    }

    fn sort(&mut self) {
        let (host, name, path) = (self.host, self.name, self.path);
        self.rows.sort_by(|a, b| {
            (a[host].as_bytes(), a[name].as_bytes(), a[path].as_bytes())
                .cmp(&(b[host].as_bytes(), b[name].as_bytes(), b[path].as_bytes()))
        });
    }
}

/// The login cookies to save, from the ones saved before and the cookies
/// the browser has now.
///
/// The browser's login cookies replace the saved ones. A saved cookie the
/// browser no longer has stays saved when the browser has no cookie of its
/// host at all: the browser deleted every cookie on exit (or has not been
/// given the saved ones back yet). When the host still has other cookies,
/// the site itself removed it (a logout), and it goes. Expired cookies go.
pub fn merge(saved: Option<&Cookies>, browser: &Cookies, rules: &Rules, now_ms: i64) -> Cookies {
    let mut out = Cookies::empty_like(browser);
    let hosts: HashSet<&[u8]> = browser.rows.iter().map(|r| r[browser.host].as_bytes()).collect();
    let login: HashMap<Key, &Vec<Value>> = browser
        .rows
        .iter()
        .filter(|r| browser.is_login(r, rules) && !browser.expired(r, now_ms))
        .map(|r| (browser.key(r), r))
        .collect();
    // Saved by another version of the browser: the columns may not mean
    // the same any more.
    if let Some(saved) = saved.filter(|s| s.version == browser.version && s.cols == browser.cols) {
        for row in &saved.rows {
            if !login.contains_key(&saved.key(row))
                && !hosts.contains(row[saved.host].as_bytes())
                && saved.is_login(row, rules)
                && !saved.expired(row, now_ms)
            {
                out.rows.push(row.clone());
            }
        }
    }
    out.rows.extend(login.into_values().cloned());
    out.sort();
    out
}

/// Days since 1970-01-01 as a civil date (Howard Hinnant's algorithm).
pub fn date(ms: i64) -> String {
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_login_cookies() {
        for name in [
            "SID", "HSID", "SAPISID", "__Secure-3PSID", "SIDCC", "LOGIN_INFO", "PHPSESSID", "JSESSIONID",
            "sessionid", "user_session", "__Host-user_session_same_site", "_gh_sess", "logged_in", "dotcom_user",
            "connect.sid", "auth_token", "ct0", "c_user", "xs", "NID_AUT", "NID_SES", "_kawlt", "reddit_session",
            "token_v2", "ESTSAUTHPERSISTENT", "wordpress_logged_in_abc", "wordpress_sec_abc", "remember_web_59ba",
            "__Secure-next-auth.session-token", "laravel_session", "XSRF-TOKEN", "ds_user_id", "li_at", "acct",
        ] {
            assert!(guess(name), "{name}");
        }
        for name in [
            "_ga", "_ga_ABC123", "_gid", "__utma", "_fbp", "NNB", "NID", "_hjSessionUser_123", "_hjSession_123",
            "sidebar_collapsed", "username_hint", "_uetsid", "IDE", "test_cookie", "_pin_unauth", "AMP_TOKEN",
            "ajs_user_id", "__stripe_sid", "s_sess", "optimizelySession", "intercom-session-abc", "theme", "lang",
            "VISITOR_INFO1_LIVE", "YSC", "datr", "guest_id", "personalization_id", "__cf_bm",
        ] {
            assert!(!guess(name), "{name}");
        }
    }

    #[test]
    fn globs() {
        assert!(glob("abc", "abc"));
        assert!(!glob("abc", "abcd"));
        assert!(glob("abc*", "abcd"));
        assert!(glob("*cd", "abcd"));
        assert!(glob("a*c*e", "abcde"));
        assert!(!glob("a*c*e", "abcdef"));
        assert!(glob("*", ""));
        assert!(!glob("a*a", "a"));
        assert!(glob("a*a", "aa"));
    }

    #[test]
    fn sites() {
        assert!(in_site(".naver.com", "naver.com"));
        assert!(in_site("nid.naver.com", "naver.com"));
        assert!(in_site("NAVER.com", "naver.com"));
        assert!(!in_site("notnaver.com", "naver.com"));
        assert!(!in_site("naver.com.evil", "naver.com"));
        assert!(sent_to(".google.com", "www.google.com"));
        assert!(sent_to("mail.google.com", "google.com"));
        assert!(!sent_to("accounts.google.com", "www.google.com"));
        assert!(!sent_to(".google.com", "notgoogle.com"));
        assert_eq!(normalize_site("https://www.Naver.com/path?q"), "www.naver.com");
        assert_eq!(normalize_site(".daum.net"), "daum.net");
    }

    #[test]
    fn rules_come_before_the_guess() {
        let rules = Rules::parse("# mine\nHM_CU\nmy*cookie  # comment\n!theme_sess\n@daum.net\n!@ads.example\n").unwrap();
        assert!(rules.is_login("HM_CU", ".daum.net"));
        assert!(rules.is_login("hm_cu", "example.com"));
        assert!(rules.is_login("MyLoginCookie", "example.com"));
        assert!(!rules.is_login("theme_sess", "example.com"));
        assert!(rules.is_login("PROF", "www.daum.net"));
        assert!(!rules.is_login("PROF", "naver.com"));
        assert!(!rules.is_login("sessionid", "ads.example"));
        assert!(rules.is_login("sessionid", "example.com"));
        assert!(Rules::parse("two words").is_err());
        assert!(Rules::parse("!@").is_err());
    }

    fn cookies(version: i64, rows: &[(&str, &str, i64)]) -> Cookies {
        let cols = ["originAttributes", "name", "value", "host", "path", "expiry", "lastAccessed"];
        let rows = rows
            .iter()
            .map(|(name, host, expiry)| {
                vec![
                    Value::text(""),
                    Value::text(name),
                    Value::text("v"),
                    Value::text(host),
                    Value::text("/"),
                    Value::Int(*expiry),
                    Value::Int(1),
                ]
            })
            .collect();
        Cookies::new(version, cols.map(String::from).to_vec(), rows).unwrap()
    }

    fn names(c: &Cookies) -> Vec<String> {
        c.rows.iter().map(|r| format!("{}@{}", c.name(r), c.host(r))).collect()
    }

    const NOW: i64 = 1_800_000_000_000;
    const LATER: i64 = NOW + 86_400_000;

    #[test]
    fn keeps_login_cookies_of_the_browser() {
        let browser = cookies(16, &[("SID", ".google.com", LATER), ("NID", ".google.com", LATER), ("_ga", ".x.com", LATER)]);
        let out = merge(None, &browser, &Rules::default(), NOW);
        assert_eq!(names(&out), ["SID@.google.com"]);
    }

    #[test]
    fn keeps_saved_cookies_the_browser_deleted_on_exit() {
        let saved = cookies(16, &[("SID", ".google.com", LATER), ("user_session", "github.com", LATER)]);
        // Everything was deleted on exit, then a new visit to google.com.
        let browser = cookies(16, &[("NID", ".google.com", LATER)]);
        let out = merge(Some(&saved), &browser, &Rules::default(), NOW);
        // github.com has no cookie left: kept. google.com has others: the site removed SID.
        assert_eq!(names(&out), ["user_session@github.com"]);
    }

    #[test]
    fn browser_cookies_replace_saved_ones() {
        let saved = cookies(16, &[("SID", ".google.com", LATER)]);
        let browser = cookies(16, &[("SID", ".google.com", LATER + 1)]);
        let out = merge(Some(&saved), &browser, &Rules::default(), NOW);
        assert_eq!(out.rows.len(), 1);
        assert_eq!(out.expiry_ms(&out.rows[0]), Some(LATER + 1));
        assert!(!out.same_as(&saved));
    }

    #[test]
    fn drops_expired_and_other_versions() {
        let saved = cookies(16, &[("SID", ".google.com", NOW - 1), ("auth", "a.com", LATER)]);
        let browser = cookies(16, &[]);
        assert_eq!(names(&merge(Some(&saved), &browser, &Rules::default(), NOW)), ["auth@a.com"]);
        let browser = cookies(17, &[]);
        assert!(merge(Some(&saved), &browser, &Rules::default(), NOW).rows.is_empty());
    }

    #[test]
    fn expiry_in_seconds_or_milliseconds() {
        let saved = cookies(12, &[("SID", ".google.com", NOW / 1000 + 60), ("auth", "a.com", NOW / 1000 - 60)]);
        assert_eq!(names(&merge(Some(&saved), &cookies(12, &[]), &Rules::default(), NOW)), ["SID@.google.com"]);
    }

    #[test]
    fn last_access_is_not_a_change() {
        let a = cookies(16, &[("SID", ".google.com", LATER)]);
        let mut b = a.clone();
        b.rows[0][6] = Value::Int(2);
        assert!(a.same_as(&b));
        b.rows[0][2] = Value::text("new");
        assert!(!a.same_as(&b));
    }

    #[test]
    fn skips_partitioned_cookies() {
        let mut c = cookies(16, &[("SID", ".google.com", LATER)]);
        c.rows[0][0] = Value::text("^partitionKey=%28https%2Cexample.com%29");
        assert!(merge(None, &c, &Rules::default(), NOW).rows.is_empty());
    }

    #[test]
    fn dates() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(951_782_400_000), "2000-02-29");
        assert_eq!(date(1_800_000_000_000), "2027-01-15");
    }
}
