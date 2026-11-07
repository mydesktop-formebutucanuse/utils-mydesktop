# logincookies

Keeps the login cookies of LibreWolf (or Firefox) when the browser deletes
its cookies on exit, and only those.

LibreWolf deletes every cookie when it closes ("Delete cookies and site data
when LibreWolf is closed", `privacy.sanitize.sanitizeOnShutdown`). That also
logs you out of every site. Exceptions keep all the cookies of a site,
trackers included. logincookies keeps just the cookies a login needs:

- It starts the browser. While the browser runs, the cookies whose names
  look like the ones of a login are saved apart, every time the cookie
  database changes (it is checked every 2 seconds).
- The next time, before the browser starts, the saved cookies are put back
  into its database, so the sites find you logged in. Every other cookie
  stays deleted.
- A login cookie that a site removes while it has other cookies there (you
  logged out) is no longer saved. Expired cookies go too.

Nothing changes in the browser's settings: it keeps deleting its cookies on
exit.

No crate dependencies: only libsqlite3 is linked.

## Login cookies

A cookie is taken for a login cookie by its name, case ignored, when it

- contains `sess`, `auth`, `token`, `login`, `logged`, `remember`, `jwt`,
  `acct` or `account` (`PHPSESSID`, `user_session`, `auth_token`,
  `LOGIN_INFO`, `XSRF-TOKEN`),
- has the word `sid` or `user` with no letter touching it (`connect.sid`,
  `c_user`, `dotcom_user`, but not `sidebar` or `username`),
- has `SID` in capitals (Google: `SID`, `HSID`, `SAPISID`, `__Secure-3PSID`),
- starts with `wordpress_`,
- or is one of the names of big sites the above misses: Naver (`NID_AUT`,
  `NID_SES`, `NID_JKL`), Kakao (`_kawlt`, `_kawltea`, `_karmt`, `_karmtea`),
  Facebook (`xs`), X (`ct0`, `twid`, `kdt`), LinkedIn (`li_at`), Apple
  (`myacinfo`), Spotify (`sp_dc`, `sp_key`), Netflix, Amazon (`at-main`,
  `x-main`, `ubid-main`) and Google's `__Host-GAPS`.

Analytics cookies whose names look like these are not (`_ga...`, `_hj...`
of Hotjar, `__stripe_...`, `ajs_...`, `AMP_TOKEN`, `optimizely...`,
`intercom-...`, `s_sess`). Neither are third-party cookies kept apart for
one site (partitioned): a login is with the site itself.

A guess from the name misses some sites and takes a few cookies too many.
`$XDG_CONFIG_HOME/logincookies/rules` (`~/.config/logincookies/rules`)
comes first:

```
NAME     a cookie named NAME is a login cookie (* matches anything)
!NAME    a cookie named NAME never is
@SITE    every cookie of SITE and of its subdomains is one
!@SITE   no cookie of SITE is one
```

See [rules.example](rules.example). To find what a site needs, log in and
run `logincookies list -a` while the browser runs: it lists the cookies the
browser has and marks the login cookies with `*`.

## Build and install

On Gentoo the package installs it (see the [top README](../README.md)).
By hand, with Rust 1.85 or later and SQLite 3.35 or later, from the top of
the repository:

```sh
cargo build --release
install -m 755 target/release/logincookies ~/.local/bin/
```

## Usage

```
logincookies [-v] [-b BROWSER] [-p PROFILE] [--] [ARGS...]
logincookies [-b BROWSER] [-p PROFILE] list [-a]
logincookies [-b BROWSER] [-p PROFILE] forget SITE...
```

Start the browser with `logincookies` instead of `librewolf`: in Window
Maker, put `logincookies` as the command of the LibreWolf dock icon or menu
entry. `ARGS` go to the browser (`logincookies --private-window`), and
`logincookies` ends when the browser does. `-v` prints the cookies it saves
and puts back. Started while the browser already runs, it only passes
`ARGS` on (a new window), and it saves the cookies if nothing else does.

Links opened from other applications start LibreWolf through its desktop
file. To have them go through logincookies too:

```sh
mkdir -p ~/.local/share/applications
sed 's/^Exec=librewolf/Exec=logincookies/' /usr/share/applications/librewolf.desktop \
	>~/.local/share/applications/librewolf.desktop
```

`-b firefox` does the same for Firefox. The profile is the browser's
default one, from `profiles.ini` in `~/.librewolf` (`~/.mozilla/firefox`);
`-p` gives another profile folder, and it is passed on to the browser.

`logincookies list` prints the saved cookies: site, expiry date and name
(never the values). `logincookies forget SITE` deletes the saved cookies of
a site (`naver.com`, `www.naver.com` or a URL): the ones the site gets and
the ones of its subdomains. Log out of the site first if the browser runs,
or they are saved again.

## Limits

- Only cookies with an expiry date are kept: a browser keeps the others
  (session cookies) in memory only, and they end with it anyway. Tick "keep
  me logged in" (로그인 상태 유지) where a site offers it.
- Sites that keep their login in their local storage instead of a cookie
  (Discord, for example) are logged out all the same: logincookies only
  handles cookies.
- The cookies are saved within about 2 seconds of the browser writing them.
  Quitting right after logging in can lose that login.
- A browser started without logincookies gets no cookies back, and its
  cookies are saved only once logincookies is started while it runs.
- Private windows never write their cookies, so nothing of them is saved.
- When the browser's cookie database changes format (`PRAGMA user_version`)
  outside logincookies, the cookies saved before are not put back: log in
  again once. An upgrade the browser makes after logincookies has put them
  back keeps them.

The saved cookies are in `$XDG_DATA_HOME/logincookies/PROFILE.sqlite`
(`~/.local/share/logincookies`), readable by you only. Like the browser's
own cookie database, they log in whoever reads them: keep them out of
backups others can read.

## License

GPL-3.0-or-later. See [LICENSE](../LICENSE).
