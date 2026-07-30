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
}

fn default_max_live() -> usize {
    2
}

fn default_idle_minutes() -> u64 {
    15
}

pub const PALETTE: [&str; 8] = [
    "#6366f1", "#ec4899", "#f59e0b", "#10b981", "#06b6d4", "#a855f7", "#ef4444", "#84cc16",
];

impl Default for Config {
    fn default() -> Self {
        Config {
            accounts: vec![],
            max_live: default_max_live(),
            idle_minutes: default_idle_minutes(),
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
        assert_eq!(Account::discovered("a@b.com", 8).color, PALETTE[0]);
    }

    #[test]
    fn unknown_service_falls_back_to_mail() {
        assert!(service_url("x@y.com", "nonsense").contains("mail.google.com"));
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
        };
        assert!(config.find(" one@gmail.COM ").is_some());
        assert!(config.find("other@gmail.com").is_none());
    }
}
