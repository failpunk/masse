use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const SERVICES: [&str; 3] = ["mail", "calendar", "drive"];

/// Not a real Google app: the pane that runs Google's add-account flow. Kept out
/// of SERVICES so it never appears as a tab.
pub const ADD: &str = "__add";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub email: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub color: String,
    /// Filled in automatically from the signed-in page; not meant to be hand-edited.
    #[serde(default)]
    pub avatar: Option<String>,
}

impl Account {
    /// A newly discovered account, coloured by position so the rail stays legible.
    pub fn discovered(email: &str, index: usize) -> Account {
        Account {
            email: email.trim().to_string(),
            label: String::new(),
            color: PALETTE[index % PALETTE.len()].into(),
            avatar: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub accounts: Vec<Account>,
    /// How many panes may stay loaded at once. This is the memory dial: each live
    /// pane is a WebKit content process holding a whole Google app. Everything
    /// beyond this is destroyed least-recently-used first and reloads on return.
    #[serde(default = "default_max_live")]
    pub max_live: usize,
    /// Minutes a pane may sit untouched before it is destroyed to give the memory
    /// back. The pane on screen is exempt. 0 turns the timeout off.
    #[serde(default = "default_idle_minutes")]
    pub idle_minutes: u64,
    /// Window geometry from last quit: width, height, x, y, in physical pixels.
    #[serde(default)]
    pub window: Option<[f64; 4]>,
    /// Which display the window was on, and where it sat within that display.
    /// Absolute coordinates alone break when displays are rearranged, because the
    /// same coordinate then belongs to a different screen.
    #[serde(default)]
    pub monitor: Option<MonitorSpot>,
    /// Where you were when you quit: email then service.
    #[serde(default)]
    pub last: Option<[String; 2]>,
    /// "split" puts accounts on the left and apps across the top. "stacked" folds
    /// everything into the left rail, so every account's three apps are one click
    /// away without changing account first.
    #[serde(default = "default_nav")]
    pub nav: String,
}

/// A remembered display, identified well enough to recognise it again after a
/// rearrangement, plus the window's offset inside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitorSpot {
    /// The display's name, as macOS reports it. Absent on some virtual displays.
    #[serde(default)]
    pub name: String,
    /// Its pixel size, used to tell two same-named displays apart.
    pub w: f64,
    pub h: f64,
    /// The window's position relative to this display's top left.
    pub dx: f64,
    pub dy: f64,
    /// Where this display sits in the desktop's coordinate space. Two identical
    /// external monitors report the SAME name and the SAME size, so name and size
    /// alone cannot tell them apart and the window kept coming back on the wrong
    /// one. The origin is what actually distinguishes them.
    #[serde(default)]
    pub ox: f64,
    #[serde(default)]
    pub oy: f64,
    /// The display's scale factor when the size below was recorded. A remembered
    /// physical size only means something alongside the scale it was taken at.
    #[serde(default)]
    pub scale: f64,
}

pub const NAV_SPLIT: &str = "split";
pub const NAV_STACKED: &str = "stacked";

fn default_nav() -> String {
    NAV_SPLIT.to_string()
}

fn default_max_live() -> usize {
    2
}

fn default_idle_minutes() -> u64 {
    15
}

/// Ten suggestions rather than a full colour picker: enough to tell several accounts
/// apart at a glance, few enough to pick from without deliberating. Ordered
/// neutral-first so the greys read as the quiet default.
pub const PALETTE: [&str; 10] = [
    "#9aa4b2", // grey
    "#64748b", // slate
    "#6366f1", // indigo
    "#06b6d4", // cyan
    "#10b981", // green
    "#facc15", // yellow
    "#f97316", // orange
    "#ec4899", // pink
    "#ef4444", // red
    "#a855f7", // violet
];

/// Whether a string is a colour we are willing to write to the config.
pub fn is_hex_colour(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].chars().all(|c| c.is_ascii_hexdigit())
}

impl Default for Config {
    fn default() -> Self {
        Config {
            accounts: vec![],
            max_live: default_max_live(),
            idle_minutes: default_idle_minutes(),
            window: None,
            monitor: None,
            last: None,
            nav: default_nav(),
        }
    }
}

impl Config {
    /// Deliberately ~/.config rather than dirs::config_dir(), which on macOS
    /// resolves to ~/Library/Application Support. This file is meant to be opened
    /// and edited by hand, so it lives where you would go looking for it.
    pub fn path() -> PathBuf {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        home.join(".config").join("masse").join("accounts.json")
    }

    pub fn load() -> Config {
        let path = Self::path();
        let mut config: Config = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| match serde_json::from_str(&raw) {
                Ok(parsed) => Some(parsed),
                Err(err) => {
                    // Never silently discard someone's account list.
                    eprintln!("[masse] {} is not valid JSON: {err}", path.display());
                    eprintln!("[masse] refusing to overwrite it; fix or delete the file");
                    std::process::exit(1);
                }
            })
            .unwrap_or_default();

        if config.accounts.is_empty() {
            config.accounts = vec![Account {
                email: "you@example.com".into(),
                label: "Edit accounts.json".into(),
                color: PALETTE[0].into(),
                avatar: None,
            }];
            config.save();
            eprintln!("[masse] wrote a starter config to {}", path.display());
        }

        for (i, account) in config.accounts.iter_mut().enumerate() {
            if account.color.is_empty() {
                account.color = PALETTE[i % PALETTE.len()].into();
            }
        }
        config.max_live = config.max_live.max(1);
        // A hand-edited file should not be able to put the window in a layout that
        // does not exist.
        if config.nav != NAV_SPLIT && config.nav != NAV_STACKED {
            eprintln!("[masse] unknown nav \"{}\", falling back to {NAV_SPLIT}", config.nav);
            config.nav = default_nav();
        }
        config
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(err) = fs::write(&path, json + "\n") {
                    eprintln!("[masse] could not write {}: {err}", path.display());
                }
            }
            Err(err) => eprintln!("[masse] could not serialise config: {err}"),
        }
    }

    pub fn find(&self, email: &str) -> Option<&Account> {
        let wanted = email.trim().to_lowercase();
        self.accounts
            .iter()
            .find(|a| a.email.trim().to_lowercase() == wanted)
    }
}

/// Addressed by email rather than by the /u/0, /u/1 index. Those indices are
/// assigned per Google session and shift when a login is added or removed, so a
/// hardcoded one quietly starts opening a different inbox.
pub fn service_url(email: &str, service: &str) -> String {
    let user = encode(email);
    match service {
        // Google's own add-account flow. Whichever account gets signed in here is
        // detected from the resulting page, so no email has to be typed anywhere.
        ADD => "https://accounts.google.com/AddSession?continue=https%3A%2F%2Fmail.google.com%2Fmail%2Fu%2F0%2F".to_string(),
        "calendar" => format!("https://calendar.google.com/calendar/r?authuser={user}"),
        "drive" => format!("https://drive.google.com/drive/my-drive?authuser={user}"),
        _ => format!("https://mail.google.com/mail/u/?authuser={user}"),
    }
}

/// Signed out, mail.google.com and calendar.google.com redirect to marketing
/// pages that contain no login form, so a pane that lands there is a dead end.
/// This is the way back in: Google's own login, told where to go afterwards and
/// which address to preselect.
pub fn signin_url(email: &str, service: &str) -> String {
    format!(
        "https://accounts.google.com/ServiceLogin?continue={}&Email={}",
        encode(&service_url(email, service)),
        encode(email)
    )
}

/// The host a pane is supposed to end up on. Anywhere else (bar the login flow
/// itself) means we got bounced.
pub fn expected_host(service: &str) -> &'static str {
    match service {
        ADD => "accounts.google.com",
        "calendar" => "calendar.google.com",
        "drive" => "drive.google.com",
        _ => "mail.google.com",
    }
}

fn encode(raw: &str) -> String {
    raw.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// What to do with a navigation a pane is attempting.
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    /// Belongs to this pane's app. Let it happen.
    Stay,
    /// A link. Hand it to the real browser.
    External,
    /// Block it and tell nobody. Analytics and ad beacons arrive as navigations
    /// wry cannot distinguish from top-level ones, and throwing those at the
    /// browser opens junk tabs.
    Drop,
}

/// Hosts that only ever appear as tracking beacons.
const TRACKERS: [&str; 6] = [
    "doubleclick.net",
    "google-analytics.com",
    "googletagmanager.com",
    "googlesyndication.com",
    "googleadservices.com",
    "adservice.google.com",
];

/// Google's own infrastructure. Gmail is built out of iframes pointing at these:
/// the apps launcher (ogs), the feedback widget (clients6), the Calendar side
/// panel. wry reports subframe loads through the same handler as top-level
/// navigations, so treating these as outbound opened a browser tab per widget
/// every time Gmail loaded.
fn is_google_owned(host: &str) -> bool {
    const OWNED: [&str; 5] = [
        "google.com",
        "googleapis.com",
        "gstatic.com",
        "googleusercontent.com",
        "googlemail.com",
    ];
    OWNED
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

fn is_tracker(host: &str) -> bool {
    TRACKERS
        .iter()
        .any(|t| host == *t || host.ends_with(&format!(".{t}")))
}

/// For navigations a pane makes on its own, including subframes. Conservative on
/// purpose: only a genuinely third-party page is worth ejecting, because anything
/// Google-owned is probably a piece of the app rather than a link.
pub fn route(service: &str, url: &str) -> Route {
    if stays_in_pane(service, url) {
        return Route::Stay;
    }
    match host_of(url) {
        None => Route::Stay,
        Some(host) if is_tracker(host) => Route::Drop,
        Some(host) if is_google_owned(host) => Route::Stay,
        _ if !url.starts_with("http://") && !url.starts_with("https://") => Route::Drop,
        _ => Route::External,
    }
}

/// For target=_blank, which is how Gmail, Calendar and Drive mark a real link the
/// user clicked. Everything leaves, including Google's own apps, because a link is
/// a link. Nothing is silently swallowed except beacons.
/// Whether this looks like a file coming down rather than a page to visit.
///
/// Gmail and Drive start a download by opening a new window at one of these, and a
/// blob: URL is content the page built itself, which is only ever a download or a
/// preview. Either way it has to stay in the app: handing it to the browser means a
/// second sign-in, and denying it means nothing happens at all.
pub fn is_download(url: &str) -> bool {
    if url.starts_with("blob:") {
        return true;
    }
    let Some(host) = host_of(url) else { return false };
    matches!(
        host,
        "mail-attachment.googleusercontent.com" | "drive.usercontent.google.com"
    ) || (host.ends_with(".googleusercontent.com") && url.contains("/download"))
        || url.contains("view=att")
        || url.contains("export=download")
}

/// Whether `?authuser=` means anything to this host.
///
/// Google honours it across its apps, so a Meet link opened from account B's
/// calendar can say which account it belongs to. `accounts.google.com` is
/// excluded: that is the sign-in flow itself, and it carries its own account
/// selection which this must not fight with.
fn honours_authuser(host: &str) -> bool {
    is_google_owned(host) && host != "accounts.google.com"
}

/// Stamp the owning account onto an outbound Google URL.
///
/// Without this, handing a Meet link to the browser lands on whichever account
/// the browser happens to have first, which for anyone signed into several is
/// usually the wrong one. Non-Google links are returned untouched: `authuser` is
/// Google's parameter and appending it to somebody else's URL is noise at best.
pub fn with_account(url: &str, email: &str) -> String {
    let Some(host) = host_of(url) else {
        return url.to_string();
    };
    if !honours_authuser(host) {
        return url.to_string();
    }
    // Already addressed, by us or by Google. Leave it alone.
    if url.contains("authuser=") {
        return url.to_string();
    }
    // The parameter has to go in the query, which ends where the fragment
    // begins. Appending blindly would bury it inside Gmail's #inbox/... instead.
    let (before, fragment) = match url.split_once('#') {
        Some((before, fragment)) => (before, Some(fragment)),
        None => (url, None),
    };
    let separator = if before.contains('?') { '&' } else { '?' };
    let mut out = format!("{before}{separator}authuser={}", encode(email));
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    out
}

pub fn route_link(_service: &str, url: &str) -> Route {
    match host_of(url) {
        Some(host) if is_tracker(host) => Route::Drop,
        _ if !url.starts_with("http://") && !url.starts_with("https://") => Route::Drop,
        _ => Route::External,
    }
}

/// Whether a navigation belongs inside its pane, or is an outbound link that
/// should go to the real browser.
///
/// Deliberately a short allowlist rather than a blocklist: a pane is for exactly
/// one Google app, plus the login flow it may be bounced through. Anything else,
/// including Docs and Sheets opened from Drive, is somebody else's page and opens
/// in the browser where history, extensions and password manager live.
pub fn stays_in_pane(service: &str, url: &str) -> bool {
    // In-page navigations carry no host and must never be treated as outbound.
    for scheme in ["about:", "blob:", "data:", "javascript:"] {
        if url.starts_with(scheme) {
            return true;
        }
    }
    match host_of(url) {
        None => true,
        Some(host) => {
            host == expected_host(service)
                // The sign-in flow, which a pane legitimately gets redirected into.
                // Google checks login state across its properties during sign-in
                // (accounts.youtube.com/CheckConnection and friends), so the whole
                // accounts.* family stays in the pane. Letting those out puts stray
                // tabs in the browser mid-login.
                || host.split('.').next() == Some("accounts")
                // Signed-out marketing pages: reached only as a bounce, and handled
                // by sending the pane to the login page instead.
                || is_signed_out_bounce(url)
                // Attachment and image previews served for the pane itself.
                || host.ends_with(".googleusercontent.com")
                || host == "drive.usercontent.google.com"
        }
    }
}

/// The marketing page Google redirects to when you ask for an app while signed
/// out. Not a destination anyone wants: the pane gets sent to the login instead.
pub fn is_signed_out_bounce(url: &str) -> bool {
    matches!(host_of(url), Some("workspace.google.com"))
}

fn host_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    // Drop any userinfo and port so comparisons are on the bare host.
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host.split_once(':').map_or(host, |(h, _)| h);
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

pub fn initials(account: &Account) -> String {
    let source = if account.label.trim().is_empty() {
        account.email.trim()
    } else {
        account.label.trim()
    };
    let words: Vec<&str> = source
        .split(|c: char| c.is_whitespace() || "._@-".contains(c))
        .filter(|w| !w.is_empty())
        .collect();
    match words.as_slice() {
        [] => "?".into(),
        [one] => one.chars().next().unwrap().to_uppercase().to_string(),
        [first, second, ..] => format!(
            "{}{}",
            first.chars().next().unwrap().to_uppercase(),
            second.chars().next().unwrap().to_uppercase()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(label: &str, email: &str) -> Account {
        Account {
            email: email.into(),
            label: label.into(),
            color: String::new(),
            avatar: None,
        }
    }

    #[test]
    fn urls_carry_the_encoded_address() {
        assert_eq!(
            service_url("a.b@work.com", "mail"),
            "https://mail.google.com/mail/u/?authuser=a.b%40work.com"
        );
        assert!(service_url("x@y.com", "calendar").starts_with("https://calendar.google.com/"));
        assert!(service_url("x@y.com", "drive").starts_with("https://drive.google.com/"));
    }

    #[test]
    fn signin_url_carries_the_destination_and_the_address() {
        let url = signin_url("a@b.com", "calendar");
        assert!(url.starts_with("https://accounts.google.com/ServiceLogin?continue="));
        assert!(url.contains("calendar.google.com"), "must come back to Calendar");
        assert!(url.contains("Email=a%40b.com"));
    }

    #[test]
    fn expected_hosts_match_the_service_urls() {
        for service in SERVICES {
            assert!(
                service_url("x@y.com", service).contains(expected_host(service)),
                "{service} url and expected host disagree"
            );
        }
    }

    #[test]
    fn the_add_flow_is_not_a_tab_and_lands_on_accounts() {
        assert!(!SERVICES.contains(&ADD));
        assert!(service_url("x@y.com", ADD).contains("AddSession"));
        assert_eq!(expected_host(ADD), "accounts.google.com");
    }

    #[test]
    fn discovered_accounts_cycle_the_palette() {
        assert_eq!(Account::discovered(" a@b.com ", 0).email, "a@b.com");
        assert_eq!(Account::discovered("a@b.com", 0).color, PALETTE[0]);
        // Length-agnostic: adding a colour should not break this.
        assert_eq!(Account::discovered("a@b.com", PALETTE.len()).color, PALETTE[0]);
        assert_eq!(Account::discovered("a@b.com", PALETTE.len() - 1).color, PALETTE[PALETTE.len() - 1]);
    }

    #[test]
    fn unknown_service_falls_back_to_mail() {
        assert!(service_url("x@y.com", "nonsense").contains("mail.google.com"));
    }

    #[test]
    fn outbound_links_leave_the_pane() {
        // The pane's own app stays put.
        assert!(stays_in_pane("mail", "https://mail.google.com/mail/u/0/#inbox"));
        assert!(stays_in_pane("calendar", "https://calendar.google.com/calendar/r/day"));
        // Login is part of a pane's legitimate life.
        assert!(stays_in_pane("mail", "https://accounts.google.com/v3/signin/identifier"));
        // Previews the pane itself serves.
        assert!(stays_in_pane("mail", "https://lh3.googleusercontent.com/a/x=s96-c"));

        // Everything else is a link, and links belong in the browser.
        assert!(!stays_in_pane("mail", "https://example.com/article"));
        assert!(!stays_in_pane("mail", "https://www.google.com/url?q=https://x.com"));
        assert!(!stays_in_pane("mail", "https://calendar.google.com/calendar/r"));
        assert!(!stays_in_pane("drive", "https://docs.google.com/document/d/abc/edit"));
    }

    #[test]
    fn a_lookalike_domain_is_not_treated_as_google() {
        assert_eq!(route("mail", "https://google.com.evil.test/x"), Route::External);
        assert_eq!(route("mail", "https://notgoogle.com/x"), Route::External);
    }

    #[test]
    fn the_login_flow_is_never_pushed_to_the_browser() {
        // Google's cross-property login check during sign-in.
        assert!(stays_in_pane(
            "mail",
            "https://accounts.youtube.com/accounts/CheckConnection?pmpo=x"
        ));
        assert!(stays_in_pane("mail", "https://accounts.google.com/v3/signin/identifier"));
        // But youtube proper is still just a link.
        assert!(!stays_in_pane("mail", "https://www.youtube.com/watch?v=x"));
    }

    #[test]
    fn the_signed_out_bounce_stays_in_the_pane_to_be_rescued() {
        let bounce = "https://workspace.google.com/intl/en-US/gmail/";
        assert!(is_signed_out_bounce(bounce));
        assert!(stays_in_pane("mail", bounce), "must not escape to the browser");
        assert!(!is_signed_out_bounce("https://mail.google.com/mail/u/0/"));
    }

    #[test]
    fn gmails_own_widgets_never_become_browser_tabs() {
        // Every one of these opened a Chrome tab in 0.8.1 on each Gmail load.
        for url in [
            "https://ogs.google.com/u/0/widget/app?origin=https%3A%2F%2Fmail.google.com",
            "https://feedback-pa.clients6.google.com/static/proxy.html?usegapi=1",
            "https://calendar.google.com/calendar/u/0/companion?origin=x",
            "https://www.gstatic.com/og/_/js/k=og.qtm.en_US.x",
            "https://content.googleapis.com/static/proxy.html",
        ] {
            assert_eq!(route("mail", url), Route::Stay, "{url}");
        }
        // A third-party frame still leaves.
        assert_eq!(route("mail", "https://example.com/tracking-frame"), Route::External);
    }

    #[test]
    fn a_meet_link_carries_the_account_it_was_opened_from() {
        assert_eq!(
            with_account("https://meet.google.com/abc-defg-hij", "b@work.com"),
            "https://meet.google.com/abc-defg-hij?authuser=b%40work.com"
        );
    }

    #[test]
    fn an_existing_query_gets_the_account_appended_not_replaced() {
        assert_eq!(
            with_account("https://meet.google.com/abc?hs=1", "b@work.com"),
            "https://meet.google.com/abc?hs=1&authuser=b%40work.com"
        );
    }

    #[test]
    fn the_account_goes_in_the_query_not_inside_the_fragment() {
        // Appending blindly would produce "#inbox?authuser=...", which Gmail
        // reads as part of the fragment and ignores.
        assert_eq!(
            with_account("https://mail.google.com/mail/u/0/#inbox", "b@work.com"),
            "https://mail.google.com/mail/u/0/?authuser=b%40work.com#inbox"
        );
    }

    #[test]
    fn a_link_that_already_names_an_account_is_left_alone() {
        let already = "https://meet.google.com/abc?authuser=a%40home.com";
        assert_eq!(with_account(already, "b@work.com"), already);
    }

    #[test]
    fn a_non_google_meeting_link_is_never_rewritten() {
        for url in [
            "https://us02web.zoom.us/j/1234567890",
            "https://teams.microsoft.com/l/meetup-join/x",
            "https://example.com/standup",
        ] {
            assert_eq!(with_account(url, "b@work.com"), url);
        }
    }

    #[test]
    fn the_sign_in_flow_is_never_given_an_account_param() {
        // accounts.google.com does its own account selection; a stray authuser
        // here fights the login it is in the middle of.
        let signin = "https://accounts.google.com/ServiceLogin?service=mail";
        assert_eq!(with_account(signin, "b@work.com"), signin);
    }

    #[test]
    fn attachments_are_recognised_as_downloads() {
        assert!(is_download(
            "https://mail-attachment.googleusercontent.com/attachment/u/0/?view=att&th=1"
        ));
        assert!(is_download("https://drive.usercontent.google.com/download?id=abc"));
        assert!(is_download("https://mail.google.com/mail/u/0/?ui=2&view=att&disp=safe"));
        assert!(is_download("blob:https://mail.google.com/9f2c-1"));
        assert!(is_download("https://docs.google.com/spreadsheets/d/x/export?export=download"));
        // Ordinary pages are not downloads.
        assert!(!is_download("https://example.com/article"));
        assert!(!is_download("https://mail.google.com/mail/u/0/#inbox"));
        assert!(!is_download("https://docs.google.com/document/d/abc/edit"));
    }

    #[test]
    fn a_clicked_link_always_leaves_even_when_google_owns_it() {
        assert_eq!(route_link("mail", "https://docs.google.com/document/d/x/edit"), Route::External);
        assert_eq!(route_link("mail", "https://example.com/post"), Route::External);
        assert_eq!(route_link("mail", "https://www.google-analytics.com/x"), Route::Drop);
        assert_eq!(route_link("mail", "mailto:someone@example.com"), Route::Drop);
    }

    #[test]
    fn beacons_are_dropped_rather_than_opened_in_the_browser() {
        assert_eq!(
            route("mail", "https://2507573.fls.doubleclick.net/activityi;src=x"),
            Route::Drop
        );
        assert_eq!(route("mail", "https://www.google-analytics.com/collect"), Route::Drop);
        // A real link still goes out.
        assert_eq!(route("mail", "https://example.com/article"), Route::External);
        // The pane's own app is untouched.
        assert_eq!(route("mail", "https://mail.google.com/mail/u/0/"), Route::Stay);
        // Odd schemes are dropped, not shelled out to `open`.
        assert_eq!(route("mail", "itms-apps://apps.apple.com/x"), Route::Drop);
        // A tracker lookalike is not a tracker.
        assert_eq!(route("mail", "https://notdoubleclick.net/x"), Route::External);
    }

    #[test]
    fn host_parsing_is_not_fooled_by_lookalikes() {
        // A host that merely contains the allowed name must not pass.
        assert!(!stays_in_pane("mail", "https://mail.google.com.evil.test/x"));
        assert!(!stays_in_pane("mail", "https://evil.test/mail.google.com"));
        // Userinfo must not be mistaken for the host.
        assert!(!stays_in_pane("mail", "https://mail.google.com@evil.test/x"));
        // A port is not part of the host.
        assert!(stays_in_pane("mail", "https://mail.google.com:443/mail/u/0/"));
        // In-page schemes are never outbound.
        assert!(stays_in_pane("mail", "about:blank"));
        assert!(stays_in_pane("mail", "blob:https://mail.google.com/abc"));
    }

    #[test]
    fn only_real_hex_colours_are_accepted() {
        assert!(is_hex_colour("#6366f1"));
        assert!(is_hex_colour("#ABCDEF"));
        assert!(!is_hex_colour("6366f1"), "needs the hash");
        assert!(!is_hex_colour("#63f"), "shorthand is not accepted");
        assert!(!is_hex_colour("#zzzzzz"));
        assert!(!is_hex_colour("red"));
        assert!(!is_hex_colour(""));
        // Every palette entry must survive its own validator.
        for c in PALETTE {
            assert!(is_hex_colour(c), "{c}");
        }
    }

    #[test]
    fn nav_defaults_to_split() {
        assert_eq!(default_nav(), NAV_SPLIT);
        assert_ne!(NAV_SPLIT, NAV_STACKED);
    }

    #[test]
    fn initials_prefer_the_label() {
        assert_eq!(initials(&account("AE Studio", "j@ae.studio")), "AS");
        assert_eq!(initials(&account("Personal", "j@gmail.com")), "P");
        assert_eq!(initials(&account("", "justin.vencel@ae.studio")), "JV");
    }

    #[test]
    fn lookup_ignores_case_and_padding() {
        let config = Config {
            accounts: vec![account("One", "One@Gmail.com")],
            max_live: 2,
            idle_minutes: 15,
            window: None,
            monitor: None,
            last: None,
            nav: default_nav(),
        };
        assert!(config.find(" one@gmail.COM ").is_some());
        assert!(config.find("other@gmail.com").is_none());
    }
}
