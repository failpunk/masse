// Masse: one window, several Google accounts, a small resident footprint.
//
// The whole memory argument rests on one thing: panes you are not looking at get
// destroyed, not hidden. Dropping a wry WebView tears down its WKWebView content
// process and the memory goes back to the system. `Lru` decides what to drop.

mod config;
mod lru;
mod ui;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use tao::{
    dpi::LogicalSize,
    event::{Event, StartCause, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::{Window, WindowBuilder},
};
use muda::{Menu, PredefinedMenuItem, Submenu};
use wry::{
    dpi::{LogicalPosition, LogicalSize as WrySize},
    Rect, WebView, WebViewBuilder, WebViewBuilderExtDarwin,
};

use config::{expected_host, service_url, signin_url, Account, Config, ADD, SERVICES};
use lru::Lru;
use ui::{RAIL_W, TOPBAR_H};


/// How often to look for panes that have gone cold. Cheap: it wakes, compares a
/// few timestamps, and goes back to sleep.
const SWEEP: std::time::Duration = std::time::Duration::from_secs(30);

/// One fixed identifier, shared by every pane.
///
/// This is what makes logins survive a relaunch. The *default* WKWebsiteDataStore
/// in an app without a full signing identity persists LocalStorage and IndexedDB
/// but keeps cookies in memory only, so every launch was a fresh session and
/// Google demanded two-factor again. WKWebsiteDataStore(forIdentifier:) is
/// properly persistent. Sharing one identifier across panes also means Google's
/// multi-login sees a single session, which is what makes ?authuser= work.
///
/// Changing these bytes signs you out of everything. Do not.
const SESSION_STORE: [u8; 16] = [
    0x53, 0x68, 0x69, 0x6d, 0x2d, 0x53, 0x65, 0x73, 0x73, 0x69, 0x6f, 0x6e, 0x2d, 0x76, 0x31, 0x00,
];

const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
                  (KHTML, like Gecko) Version/18.5 Safari/605.1.15";

#[derive(Debug)]
enum Msg {
    Show { email: String, service: String },
    /// A pane finished loading somewhere. Used to notice the signed-out bounce.
    Landed { email: String, service: String, url: String },
    Avatar { email: String, src: String },
    /// Open Google's add-account flow.
    AddAccount,
    OpenSettings,
    CloseSettings,
    Remove { email: String },
    Dials { max_live: usize, idle_minutes: u64 },
    OpenConfig,
}

struct App {
    config: Config,
    panes: HashMap<String, WebView>,
    lru: Lru,
    active: String,
    /// The settings modal, alive only while it is open.
    settings: Option<WebView>,
    /// Panes already sent to the login page once. Without this, a login that
    /// keeps failing would bounce the pane round in circles.
    rescued: std::collections::HashSet<String>,
}

fn key_of(email: &str, service: &str) -> String {
    format!("{}\u{1}{}", email.to_lowercase(), service)
}

fn main() -> wry::Result<()> {
    let config = Config::load();

    // Dump a chrome surface to stdout so its look can be checked in a browser
    // without launching the app. Debug aid only.
    if let Some(which) = std::env::args().nth(1).filter(|a| a.starts_with("--dump-")) {
        let state = rail_state(&config, &key_of(&config.accounts[0].email, "mail"));
        print!(
            "{}",
            match which.trim_start_matches("--dump-") {
                "rail" => ui::rail_html(&state),
                "topbar" => ui::topbar_html(&state),
                _ => ui::settings_html(&state),
            }
        );
        return Ok(());
    }
    let first = config.accounts[0].clone();

    // Without a real Edit menu macOS never routes Cmd+C/V/X or Cmd+A into a
    // WKWebView, so you cannot even paste a password into Google's login form.
    install_menu();

    let event_loop = EventLoopBuilder::<Msg>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title("Masse")
        .with_inner_size(LogicalSize::new(1340.0, 900.0))
        .with_min_inner_size(LogicalSize::new(680.0, 480.0))
        .build(&event_loop)
        .expect("window");

    let boot = rail_state(&config, &key_of(&first.email, "mail"));

    let rail_proxy = proxy.clone();
    let rail = WebViewBuilder::new()
        .with_bounds(rect(0.0, 0.0, RAIL_W, 900.0))
        .with_html(ui::rail_html(&boot))
        .with_ipc_handler(move |req| handle_rail(&rail_proxy, req.body()))
        .build_as_child(&window)?;

    let topbar_proxy = proxy.clone();
    let topbar = WebViewBuilder::new()
        .with_bounds(rect(RAIL_W, 0.0, 1340.0 - RAIL_W, TOPBAR_H))
        .with_html(ui::topbar_html(&boot))
        .with_ipc_handler(move |req| handle_rail(&topbar_proxy, req.body()))
        .build_as_child(&window)?;

    let chrome = ui::Chrome { rail, topbar };

    // `--probe` walks every account across every app and prints what each page
    // turned out to be. It is how the authuser addressing gets verified without
    // anyone clicking through nine panes by hand.
    if std::env::args().any(|a| a == "--probe") {
        let probe = event_loop.create_proxy();
        let plan: Vec<(String, String)> = config
            .accounts
            .iter()
            .flat_map(|a| SERVICES.iter().map(move |s| (a.email.clone(), s.to_string())))
            .collect();
        std::thread::spawn(move || {
            for (email, service) in plan {
                std::thread::sleep(std::time::Duration::from_secs(9));
                println!("[probe] --> {email} {service}");
                let _ = probe.send_event(Msg::Show { email, service });
            }
            std::thread::sleep(std::time::Duration::from_secs(9));
            println!("[probe] done");
            std::process::exit(0);
        });
    }

    let app = Rc::new(RefCell::new(App {
        lru: Lru::new(config.max_live),
        active: key_of(&first.email, "mail"),
        config,
        panes: HashMap::new(),
        settings: None,
        rescued: std::collections::HashSet::new(),
    }));

    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::WaitUntil(std::time::Instant::now() + SWEEP);
        reclaim_idle(&app);

        match event {
            Event::NewEvents(StartCause::Init) => {
                show(&app, &window, &proxy, &chrome, &first.email.clone(), "mail");
            }

            Event::UserEvent(Msg::Show { email, service }) => {
                show(&app, &window, &proxy, &chrome, &email, &service);
            }

            Event::UserEvent(Msg::Landed { email, service, url }) => {
                let host_ok = service == ADD
                    || url.contains(expected_host(&service))
                    || url.contains("accounts.google.com");
                if !host_ok {
                    let key = key_of(&email, &service);
                    let mut state = app.borrow_mut();
                    if state.rescued.insert(key.clone()) {
                        println!("[masse] {email} {service} bounced to a signed-out page, going to login");
                        if let Some(view) = state.panes.get(&key) {
                            let _ = view.load_url(&signin_url(&email, &service));
                        }
                    }
                }
            }

            Event::UserEvent(Msg::Avatar { email, src }) => {
                let mut app = app.borrow_mut();
                // Google serves the header thumbnail at 32px; ask for a retina one.
                let src = upscale(&src);
                let wanted = email.trim().to_lowercase();
                if app.config.find(&wanted).is_none() {
                    let index = app.config.accounts.len();
                    app.config.accounts.push(Account::discovered(&wanted, index));
                    app.config.save();
                    println!("[masse] added account {wanted}");
                }
                let changed = app
                    .config
                    .accounts
                    .iter_mut()
                    .find(|a| a.email.trim().to_lowercase() == wanted)
                    .filter(|a| a.avatar.as_deref() != Some(src.as_str()))
                    .map(|a| a.avatar = Some(src.clone()))
                    .is_some();
                if changed {
                    app.config.save();
                    chrome.push(&rail_state(&app.config, &app.active));
                }
            }

            Event::UserEvent(Msg::AddAccount) => {
                // The account is whoever ends up signed in, read off the page
                // afterwards, so nothing has to be typed and a listed account is
                // always one that genuinely works.
                let email = app.borrow().config.accounts[0].email.clone();
                show(&app, &window, &proxy, &chrome, &email, ADD);
            }

            Event::UserEvent(Msg::OpenSettings) => {
                let mut state = app.borrow_mut();
                if state.settings.is_none() {
                    // Hide the pane underneath so the modal is unambiguously on top
                    // whatever the subview order happens to be.
                    if let Some(view) = state.panes.get(&state.active) {
                        let _ = view.set_visible(false);
                    }
                    let size = window.inner_size().to_logical::<f64>(window.scale_factor());
                    let payload = rail_state(&state.config, &state.active);
                    let modal_proxy = proxy.clone();
                    match WebViewBuilder::new()
                        .with_bounds(rect(0.0, 0.0, size.width, size.height))
                        .with_transparent(true)
                        .with_html(ui::settings_html(&payload))
                        .with_ipc_handler(move |req| handle_rail(&modal_proxy, req.body()))
                        .build_as_child(&window)
                    {
                        Ok(view) => state.settings = Some(view),
                        Err(err) => eprintln!("[masse] could not open settings: {err}"),
                    }
                }
            }

            Event::UserEvent(Msg::CloseSettings) => {
                let mut state = app.borrow_mut();
                state.settings = None; // dropping it removes the subview
                if let Some(view) = state.panes.get(&state.active) {
                    let _ = view.set_visible(true);
                    let _ = view.focus();
                }
            }

            Event::UserEvent(Msg::Remove { email }) => {
                let wanted = email.trim().to_lowercase();
                let mut state = app.borrow_mut();
                if state.config.accounts.len() <= 1 {
                    eprintln!("[masse] refusing to remove the last account");
                } else {
                    state
                        .config
                        .accounts
                        .retain(|a| a.email.trim().to_lowercase() != wanted);
                    state.config.save();
                    // Tear down that account's panes rather than leaving orphans.
                    let dead: Vec<String> = state
                        .panes
                        .keys()
                        .filter(|k| k.starts_with(&format!("{wanted}\u{1}")))
                        .cloned()
                        .collect();
                    for key in dead {
                        state.panes.remove(&key);
                    }
                    println!("[masse] removed account {wanted}");
                }
                let payload = rail_state(&state.config, &state.active);
                chrome.push(&payload);
                if let Some(view) = &state.settings {
                    let _ = view.evaluate_script(&format!("window.shim.render({payload})"));
                }
                // If the account we were looking at is gone, go somewhere that exists.
                let orphaned = !state.active.starts_with(&format!("{wanted}\u{1}"));
                let fallback = state.config.accounts[0].email.clone();
                drop(state);
                if !orphaned {
                    show(&app, &window, &proxy, &chrome, &fallback, "mail");
                }
            }

            Event::UserEvent(Msg::Dials { max_live, idle_minutes }) => {
                let mut state = app.borrow_mut();
                state.config.max_live = max_live.max(1);
                state.config.idle_minutes = idle_minutes;
                state.config.save();
                state.lru = Lru::new(state.config.max_live);
                let active = state.active.clone();
                // Re-seat the visible pane so it is the freshest entry in the new budget.
                for evicted in state.lru.touch(&active) {
                    state.panes.remove(&evicted);
                }
                let extra: Vec<String> = state
                    .panes
                    .keys()
                    .filter(|k| **k != active)
                    .cloned()
                    .collect();
                for key in extra.into_iter().take(usize::MAX) {
                    if state.panes.len() > state.config.max_live {
                        state.panes.remove(&key);
                    }
                }
                println!("[masse] max_live={max_live} idle_minutes={idle_minutes}");
            }

            Event::UserEvent(Msg::OpenConfig) => {
                // -t forces the default *text editor* rather than whatever owns the
                // .json extension. Without it this opens Xcode on machines where
                // Xcode has claimed JSON, which is most machines with Xcode.
                let path = Config::path();
                let opened = std::process::Command::new("open")
                    .arg("-t")
                    .arg(&path)
                    .spawn()
                    .is_ok();
                if !opened {
                    let _ = std::process::Command::new("open").arg("-R").arg(&path).spawn();
                }
            }

            Event::WindowEvent {
                event: WindowEvent::Resized(_),
                ..
            } => {
                let size = window.inner_size().to_logical::<f64>(window.scale_factor());
                let _ = chrome.rail.set_bounds(rect(0.0, 0.0, RAIL_W, size.height));
                let _ = chrome
                    .topbar
                    .set_bounds(rect(RAIL_W, 0.0, (size.width - RAIL_W).max(1.0), TOPBAR_H));
                let app = app.borrow();
                if let Some(view) = app.panes.get(&app.active) {
                    let _ = view.set_bounds(content_rect(&window));
                }
                if let Some(view) = &app.settings {
                    let _ = view.set_bounds(rect(0.0, 0.0, size.width, size.height));
                }
            }

            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,

            _ => {}
        }
    });
}

/// Destroy panes nobody has looked at for a while. The visible one is exempt, so
/// leaving the app open on Gmail all afternoon costs one content process, not three.
fn reclaim_idle(app: &Rc<RefCell<App>>) {
    let mut state = app.borrow_mut();
    if state.config.idle_minutes == 0 {
        return;
    }
    let idle = std::time::Duration::from_secs(state.config.idle_minutes * 60);
    let active = state.active.clone();
    for key in state.lru.stale(idle, &active) {
        if state.panes.remove(&key).is_some() {
            println!(
                "[masse] reclaimed {} after {} idle minutes",
                key.replace('\u{1}', " "),
                state.config.idle_minutes
            );
        }
    }
}

/// Bring a pane forward, building it on first use and evicting whatever falls off
/// the end of the LRU. Eviction is the point: `panes.remove` drops the WebView,
/// which kills its content process.
fn show(
    app: &Rc<RefCell<App>>,
    window: &Window,
    proxy: &EventLoopProxy<Msg>,
    chrome: &ui::Chrome,
    email: &str,
    service: &str,
) {
    let key = key_of(email, service);
    let bounds = content_rect(window);
    let mut state = app.borrow_mut();

    if !state.panes.contains_key(&key) {
        let probe_proxy = proxy.clone();
        let (owner, app_name) = (email.to_string(), service.to_string());
        let built = WebViewBuilder::new()
            .with_bounds(bounds)
            .with_data_store_identifier(SESSION_STORE)
            .with_user_agent(UA)
            .with_url(service_url(email, service))
            .with_initialization_script(PROBE)
            .with_ipc_handler(move |req| handle_pane(&probe_proxy, &owner, &app_name, req.body()))
            .build_as_child(window);
        match built {
            Ok(view) => {
                state.panes.insert(key.clone(), view);
            }
            Err(err) => {
                eprintln!("[masse] could not build pane {key}: {err}");
                return;
            }
        }
    }

    for evicted in state.lru.touch(&key) {
        state.panes.remove(&evicted);
        println!("[masse] evicted {}", evicted.replace('\u{1}', " "));
    }

    for (other, view) in state.panes.iter() {
        let _ = view.set_visible(*other == key);
    }
    if let Some(view) = state.panes.get(&key) {
        let _ = view.set_bounds(bounds);
        let _ = view.focus();
    }
    state.active = key;

    chrome.push(&rail_state(&state.config, &state.active));
}

fn handle_rail(proxy: &EventLoopProxy<Msg>, body: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return;
    };
    let msg = match value["type"].as_str() {
        Some("show") => Msg::Show {
            email: value["email"].as_str().unwrap_or_default().to_string(),
            service: value["service"].as_str().unwrap_or("mail").to_string(),
        },
        Some("add") => Msg::AddAccount,
        Some("settings") => Msg::OpenSettings,
        Some("close") => Msg::CloseSettings,
        Some("remove") => Msg::Remove {
            email: value["email"].as_str().unwrap_or_default().to_string(),
        },
        Some("dials") => Msg::Dials {
            max_live: value["max_live"].as_u64().unwrap_or(2) as usize,
            idle_minutes: value["idle_minutes"].as_u64().unwrap_or(15),
        },
        Some("config") => Msg::OpenConfig,
        _ => return,
    };
    let _ = proxy.send_event(msg);
}

fn handle_pane(proxy: &EventLoopProxy<Msg>, email: &str, service: &str, body: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return;
    };
    if value["type"] == "page" {
        println!("[page] {body}");
        let _ = proxy.send_event(Msg::Landed {
            email: email.to_string(),
            service: service.to_string(),
            url: value["url"].as_str().unwrap_or_default().to_string(),
        });
        return;
    }
    if value["type"] == "avatar" {
        let (Some(email), Some(src)) = (value["email"].as_str(), value["src"].as_str()) else {
            return;
        };
        let _ = proxy.send_event(Msg::Avatar {
            email: email.to_string(),
            src: src.to_string(),
        });
    }
}

fn upscale(src: &str) -> String {
    // ".../photo.jpg=s32-c" -> ".../photo.jpg=s96-c", left alone if there is no suffix.
    match src.rfind("=s") {
        Some(at) if src[at + 2..].chars().next().is_some_and(|c| c.is_ascii_digit()) => {
            format!("{}=s96-c", &src[..at])
        }
        _ => src.to_string(),
    }
}

fn content_rect(window: &Window) -> Rect {
    let size = window.inner_size().to_logical::<f64>(window.scale_factor());
    rect(
        RAIL_W,
        TOPBAR_H,
        (size.width - RAIL_W).max(1.0),
        (size.height - TOPBAR_H).max(1.0),
    )
}

fn install_menu() {
    let menu = Menu::new();
    let app = Submenu::new("Masse", true);
    let edit = Submenu::new("Edit", true);
    let _ = app.append_items(&[
        &PredefinedMenuItem::about(None, None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::quit(None),
    ]);
    let _ = edit.append_items(&[
        &PredefinedMenuItem::undo(None),
        &PredefinedMenuItem::redo(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::cut(None),
        &PredefinedMenuItem::copy(None),
        &PredefinedMenuItem::paste(None),
        &PredefinedMenuItem::select_all(None),
    ]);
    let _ = menu.append_items(&[&app, &edit]);
    menu.init_for_nsapp();
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect {
        position: LogicalPosition::new(x, y).into(),
        size: WrySize::new(w.max(1.0), h.max(1.0)).into(),
    }
}

fn rail_state(config: &Config, active: &str) -> String {
    let accounts: Vec<serde_json::Value> = config
        .accounts
        .iter()
        .map(|a: &Account| {
            serde_json::json!({
                "email": a.email,
                "label": if a.label.is_empty() { a.email.split('@').next().unwrap_or("") } else { &a.label },
                "color": a.color,
                "avatar": a.avatar,
                "initials": config::initials(a),
            })
        })
        .collect();
    let (email, service) = active.split_once('\u{1}').unwrap_or(("", "mail"));
    serde_json::json!({
        "accounts": accounts,
        "services": SERVICES,
        "active": { "email": email, "service": service },
        "max_live": config.max_live,
        "idle_minutes": config.idle_minutes,
    })
    .to_string()
}


const PROBE: &str = r#"
window.addEventListener('load', () => {
  const find = () => {
    for (const el of document.querySelectorAll('[aria-label*="Google Account"]')) {
      const email = (el.getAttribute('aria-label') || '').match(/[\w.+-]+@[\w-]+\.[\w.]+/);
      const img = el.querySelector('img');
      if (email && img && /^https?:/.test(img.src)) {
        window.ipc.postMessage(JSON.stringify({ type: 'avatar', email: email[0], src: img.src }));
        return true;
      }
    }
    return false;
  };
  const heading = document.querySelector('h1, [role=heading]');
  window.ipc.postMessage(JSON.stringify({
    type: 'page', url: location.href.slice(0, 120), title: document.title,
    heading: heading ? heading.textContent.trim().slice(0, 60) : null,
  }));
  if (find()) return;
  const observer = new MutationObserver(() => { if (find()) observer.disconnect(); });
  observer.observe(document.documentElement, { childList: true, subtree: true });
  setTimeout(() => observer.disconnect(), 30000);
});
"#;

