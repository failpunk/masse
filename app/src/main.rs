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
use muda::{accelerator::Accelerator, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use wry::{
    dpi::{LogicalPosition, LogicalSize as WrySize},
    Rect, WebView, WebViewBuilder, WebViewBuilderExtDarwin,
};

use config::{
    expected_host, route, route_link, service_url, signin_url, Account, Config, Route, ADD,
    NAV_STACKED, SERVICES,
};
use lru::Lru;
use ui::{RAIL_W, TOPBAR_H};


/// How often to look for panes that have gone cold. Cheap: it wakes, compares a
/// few timestamps, and goes back to sleep.
/// Set by --test-links so the external-link path can be exercised without
/// actually throwing browser tabs at whoever is running it.
/// Counts panes actually shown, so --fire-menu can tell "nothing crashed" apart
/// from "nothing happened". The first version of that test passed while every
/// menu activation was silently doing nothing.
static SHOWN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

static DRY_RUN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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
    /// Reload whatever pane is on screen.
    Reload,
    /// Hand a URL to the real browser instead of showing it in a pane.
    External(String),
    /// Test hook: make the visible pane attempt a navigation.
    Drive(String),
    /// A menu item fired. Carries muda's item id.
    Menu(String),
    /// Switch between the split layout and everything-in-the-rail.
    Nav(String),
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
    let menu_keepalive = install_menu(&config.accounts);

    let event_loop = EventLoopBuilder::<Msg>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let saved = config.window;
    let mut builder = WindowBuilder::new()
        .with_title("Masse")
        .with_min_inner_size(LogicalSize::new(680.0, 480.0));
    builder = match saved {
        Some([w, h, ..]) if w >= 680.0 && h >= 480.0 => {
            builder.with_inner_size(LogicalSize::new(w, h))
        }
        _ => builder.with_inner_size(LogicalSize::new(1340.0, 900.0)),
    };
    let window = builder.build(&event_loop).expect("window");
    if let Some([_, _, x, y]) = saved {
        window.set_outer_position(tao::dpi::LogicalPosition::new(x, y));
    }

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
    if config.nav == NAV_STACKED {
        let _ = chrome.topbar.set_visible(false);
    }

    // `--fire-menu` performs every custom menu item through AppKit, which is the
    // only path that reproduces the dangling-MenuChild crash: a keypress goes
    // NSMenu -> sendAction: -> muda's fire_menu_item_click, and that is where the
    // raw pointer gets dereferenced. Nothing else exercises it.
    if std::env::args().any(|a| a == "--fire-menu") {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(10));
            let plan: [(&str, &[usize]); 3] = [
                // Accounts, then the three apps. Index 2 is a separator.
                ("Go", &[0, 1, 3, 4, 5]),
                ("View", &[0]),
                ("Masse", &[0]),
            ];
            for (menu_title, indices) in plan {
                for index in indices {
                    println!("[fire] {menu_title} item {index}");
                    fire_menu_item(menu_title, *index);
                    std::thread::sleep(std::time::Duration::from_millis(700));
                }
            }
            let shown = SHOWN.load(std::sync::atomic::Ordering::Relaxed);
            // Four Go items were fired, so at least that many switches must have
            // landed. Anything less means activations are being dropped.
            // One show at startup plus the five Go activations.
            if shown < 6 {
                eprintln!("[fire] FAILED: only {shown} panes shown; menu actions are not landing");
                std::process::exit(1);
            }
            println!("[fire] survived every menu activation, {shown} panes shown");
            std::process::exit(0);
        });
    }

    // `--stress` fires switches faster than panes can finish building, which is
    // how the RefCell double-borrow crash reproduces without a keyboard.
    if std::env::args().any(|a| a == "--stress") {
        let proxy = event_loop.create_proxy();
        let plan: Vec<(String, String)> = config
            .accounts
            .iter()
            .flat_map(|a| SERVICES.iter().map(move |s| (a.email.clone(), s.to_string())))
            .collect();
        std::thread::spawn(move || {
            for round in 0..14 {
                for (email, service) in &plan {
                    let _ = proxy.send_event(Msg::Show {
                        email: email.clone(),
                        service: service.clone(),
                    });
                    std::thread::sleep(std::time::Duration::from_millis(90));
                }
                if round % 3 == 0 {
                    let _ = proxy.send_event(Msg::OpenSettings);
                    std::thread::sleep(std::time::Duration::from_millis(120));
                    let _ = proxy.send_event(Msg::CloseSettings);
                }
                let _ = proxy.send_event(Msg::Reload);
            }
            println!("[stress] survived");
            std::process::exit(0);
        });
    }

    // `--test-links` drives a pane at an outbound URL and reports where it went,
    // which is the only way to know the navigation handler is really wired.
    if std::env::args().any(|a| a == "--test-links") {
        DRY_RUN.store(true, std::sync::atomic::Ordering::Relaxed);
        let proxy = event_loop.create_proxy();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(12));
            for url in [
                "https://example.com/outbound",
                "https://docs.google.com/document/d/x/edit",
                "https://mail.google.com/mail/u/0/#settings",
            ] {
                println!("[test] driving pane at {url}");
                let _ = proxy.send_event(Msg::Drive(url.to_string()));
                std::thread::sleep(std::time::Duration::from_secs(4));
            }
            println!("[test] done");
            std::process::exit(0);
        });
    }

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

    let menu_proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        // Runs on the main thread inside the AppKit action. Enqueuing wakes the
        // event loop immediately, which polling never did.
        let _ = menu_proxy.send_event(Msg::Menu(event.id().0.clone()));
    }));

    let app = Rc::new(RefCell::new(App {
        lru: Lru::new(config.max_live),
        active: key_of(&first.email, "mail"),
        config,
        panes: HashMap::new(),
        settings: None,
        rescued: std::collections::HashSet::new(),
    }));

    event_loop.run(move |event, _target, control_flow| {
        // Captured so the menu's allocations live as long as the process does.
        // AppKit holds raw pointers into them. Do not remove.
        let _keep = &menu_keepalive;

        *control_flow = ControlFlow::WaitUntil(std::time::Instant::now() + SWEEP);
        reclaim_idle(&app);

        match event {
            Event::NewEvents(StartCause::Init) => {
                let (email, service) = {
                    let Ok(state) = app.try_borrow() else { return };
                    match &state.config.last {
                        // Only honour it if that account still exists.
                        Some([email, service]) if state.config.find(email).is_some() => {
                            (email.clone(), service.clone())
                        }
                        _ => (first.email.clone(), "mail".to_string()),
                    }
                };
                show(&app, &window, &proxy, &chrome, &email, &service);
            }

            Event::UserEvent(Msg::Show { email, service }) => {
                show(&app, &window, &proxy, &chrome, &email, &service);
            }

            Event::UserEvent(Msg::Menu(id)) => {
                let target = {
                    let Ok(state) = app.try_borrow() else { return };
                    let (email, service) = state
                        .active
                        .split_once('\u{1}')
                        .map(|(e, s)| (e.to_string(), s.to_string()))
                        .unwrap_or_else(|| (state.config.accounts[0].email.clone(), "mail".into()));
                    match id.split_once(':') {
                        // Switching account keeps the app, and vice versa.
                        Some(("acct", n)) => n
                            .parse::<usize>()
                            .ok()
                            .and_then(|i| state.config.accounts.get(i))
                            .map(|a| (a.email.clone(), service)),
                        Some(("app", app_name)) => Some((email, app_name.to_string())),
                        _ => None,
                    }
                };
                match (target, id.as_str()) {
                    (Some((email, service)), _) => {
                        show(&app, &window, &proxy, &chrome, &email, &service)
                    }
                    (None, "reload") => {
                        let Ok(state) = app.try_borrow() else { return };
                        if let Some(view) = state.panes.get(&state.active) {
                            let _ = view.reload();
                        }
                    }
                    (None, "settings") => {
                        let _ = proxy.send_event(Msg::OpenSettings);
                    }
                    _ => {}
                }
            }

            Event::UserEvent(Msg::Landed { email, service, url }) => {
                let host_ok = service == ADD
                    || url.contains(expected_host(&service))
                    || url.contains("accounts.google.com");
                if !host_ok {
                    let key = key_of(&email, &service);
                    let Ok(mut state) = app.try_borrow_mut() else { return };
                    if state.rescued.insert(key.clone()) {
                        println!("[masse] {email} {service} bounced to a signed-out page, going to login");
                        if let Some(view) = state.panes.get(&key) {
                            let _ = view.load_url(&signin_url(&email, &service));
                        }
                    }
                }
            }

            Event::UserEvent(Msg::Avatar { email, src }) => {
                let Ok(mut app) = app.try_borrow_mut() else { return };
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
                let Ok(state) = app.try_borrow() else { return };
                let email = state.config.accounts[0].email.clone();
                drop(state);
                show(&app, &window, &proxy, &chrome, &email, ADD);
            }

            Event::UserEvent(Msg::OpenSettings) => {
                let Ok(state) = app.try_borrow_mut() else { return };
                if state.settings.is_none() {
                    // Hide the pane underneath so the modal is unambiguously on top
                    // whatever the subview order happens to be.
                    if let Some(view) = state.panes.get(&state.active) {
                        let _ = view.set_visible(false);
                    }
                    let size = window.inner_size().to_logical::<f64>(window.scale_factor());
                    let payload = rail_state(&state.config, &state.active);
                    // Build with no borrow held: constructing a WebView re-enters
                    // AppKit, which can land back in this event loop.
                    drop(state);
                    let modal_proxy = proxy.clone();
                    let built = WebViewBuilder::new()
                        .with_bounds(rect(0.0, 0.0, size.width, size.height))
                        .with_transparent(true)
                        .with_html(ui::settings_html(&payload))
                        .with_ipc_handler(move |req| handle_rail(&modal_proxy, req.body()))
                        .build_as_child(&window);
                    match built {
                        Ok(view) => {
                            if let Ok(mut state) = app.try_borrow_mut() {
                                state.settings = Some(view);
                            }
                        }
                        Err(err) => eprintln!("[masse] could not open settings: {err}"),
                    }
                }
            }

            Event::UserEvent(Msg::CloseSettings) => {
                let Ok(mut state) = app.try_borrow_mut() else { return };
                let modal = state.settings.take();
                if let Some(view) = state.panes.get(&state.active) {
                    let _ = view.set_visible(true);
                    let _ = view.focus();
                }
                drop(state);
                drop(modal); // removes the subview, re-enters AppKit

            }

            Event::UserEvent(Msg::Remove { email }) => {
                let wanted = email.trim().to_lowercase();
                let Ok(mut state) = app.try_borrow_mut() else { return };
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
                let Ok(mut state) = app.try_borrow_mut() else { return };
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

            Event::UserEvent(Msg::External(url)) => {
                println!("[masse] opening externally: {url}");
                if !DRY_RUN.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = std::process::Command::new("open").arg(&url).spawn();
                }
            }

            Event::UserEvent(Msg::Drive(url)) => {
                let Ok(state) = app.try_borrow() else { return };
                if let Some(view) = state.panes.get(&state.active) {
                    let _ = view.evaluate_script(&format!("location.href = {:?}", url));
                }
            }

            Event::UserEvent(Msg::Nav(nav)) => {
                let (payload, visible) = {
                    let Ok(mut state) = app.try_borrow_mut() else { return };
                    if state.config.nav == nav {
                        return;
                    }
                    state.config.nav = nav.clone();
                    state.config.save();
                    println!("[masse] layout -> {nav}");
                    (
                        rail_state(&state.config, &state.active),
                        state.config.nav != NAV_STACKED,
                    )
                };
                // The top bar has no place in stacked mode, and every pane has to be
                // re-seated because the content area moved.
                let (rail_r, top_r) = chrome_rects(&window, &nav);
                let _ = chrome.rail.set_bounds(rail_r);
                let _ = chrome.topbar.set_visible(visible);
                let _ = chrome.topbar.set_bounds(top_r);
                chrome.push(&payload);
                let bounds = content_rect(&window, &nav);
                if let Ok(state) = app.try_borrow() {
                    for view in state.panes.values() {
                        let _ = view.set_bounds(bounds);
                    }
                }
            }

            Event::UserEvent(Msg::Reload) => {
                let Ok(state) = app.try_borrow() else { return };
                if let Some(view) = state.panes.get(&state.active) {
                    let _ = view.reload();
                }
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
                let nav = match app.try_borrow() {
                    Ok(a) => a.config.nav.clone(),
                    Err(_) => return,
                };
                let (rail_r, top_r) = chrome_rects(&window, &nav);
                let _ = chrome.rail.set_bounds(rail_r);
                let _ = chrome.topbar.set_bounds(top_r);
                let Ok(app) = app.try_borrow() else { return };
                if let Some(view) = app.panes.get(&app.active) {
                    let _ = view.set_bounds(content_rect(&window, &nav));
                }
                if let Some(view) = &app.settings {
                    // The modal covers the whole window regardless of layout.
                    let full = window.inner_size().to_logical::<f64>(window.scale_factor());
                    let _ = view.set_bounds(rect(0.0, 0.0, full.width, full.height));
                }
            }

            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                let Ok(mut state) = app.try_borrow_mut() else { return };
                let size = window.inner_size().to_logical::<f64>(window.scale_factor());
                let pos = window
                    .outer_position()
                    .map(|p| p.to_logical::<f64>(window.scale_factor()))
                    .unwrap_or(tao::dpi::LogicalPosition::new(0.0, 0.0));
                state.config.window = Some([size.width, size.height, pos.x, pos.y]);
                let (email, service) = state
                    .active
                    .split_once('\u{1}')
                    .map(|(e, s)| (e.to_string(), s.to_string()))
                    .unwrap_or_default();
                if !email.is_empty() && service != ADD {
                    state.config.last = Some([email, service]);
                }
                state.config.save();
                *control_flow = ControlFlow::Exit;
            }

            _ => {}
        }
    });
}

/// Destroy panes nobody has looked at for a while. The visible one is exempt, so
/// leaving the app open on Gmail all afternoon costs one content process, not three.
fn reclaim_idle(app: &Rc<RefCell<App>>) {
    // AppKit re-enters this loop from inside menu actions and WebView calls. If a
    // borrow is already live, skip: the next sweep is 30 seconds away.
    let Ok(mut state) = app.try_borrow_mut() else {
        return;
    };
    if state.config.idle_minutes == 0 {
        return;
    }
    let idle = std::time::Duration::from_secs(state.config.idle_minutes * 60);
    let active = state.active.clone();
    let minutes = state.config.idle_minutes;
    let mut doomed = Vec::new();
    for key in state.lru.stale(idle, &active) {
        if let Some(view) = state.panes.remove(&key) {
            doomed.push(view);
            println!(
                "[masse] reclaimed {} after {minutes} idle minutes",
                key.replace('\u{1}', " ")
            );
        }
    }
    drop(state);
    drop(doomed);
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
    SHOWN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let key = key_of(email, service);
    let bounds = {
        let nav = app.borrow().config.nav.clone();
        content_rect(window, &nav)
    };
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
            // Target=_blank links: never adopt them into this pane.
            // Gmail marks outbound links target=_blank, so this is the main path.
            .with_new_window_req_handler({
                let proxy = proxy.clone();
                let service = service.to_string();
                move |url, _features| {
                    if route_link(&service, &url) != Route::Drop {
                        let _ = proxy.send_event(Msg::External(url));
                    }
                    wry::NewWindowResponse::Deny
                }
            })
            // Same-pane navigations to anywhere that is not this app.
            .with_navigation_handler({
                let proxy = proxy.clone();
                let service = service.to_string();
                move |url| match route(&service, &url) {
                    Route::Stay => true,
                    Route::Drop => false,
                    Route::External => {
                        let _ = proxy.send_event(Msg::External(url));
                        false
                    }
                }
            })
            .with_download_started_handler(|url, path| {
                // Straight to ~/Downloads under the name the server gave it.
                if let Some(home) = std::env::var_os("HOME") {
                    let name = path
                        .file_name()
                        .map(|n| n.to_owned())
                        .unwrap_or_else(|| std::ffi::OsString::from("download"));
                    let mut target = std::path::PathBuf::from(home);
                    target.push("Downloads");
                    target.push(name);
                    *path = target;
                }
                println!("[masse] downloading {url} -> {}", path.display());
                true
            })
            .with_download_completed_handler(|_url, path, success| match (success, path) {
                (true, Some(path)) => {
                    println!("[masse] saved {}", path.display());
                    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
                }
                _ => eprintln!("[masse] download failed"),
            })
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

    let mut doomed = Vec::new();
    for evicted in state.lru.touch(&key) {
        if let Some(view) = state.panes.remove(&evicted) {
            doomed.push(view);
        }
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

    // Released last, and explicitly outside the borrow above, because tearing a
    // WebView down re-enters AppKit.
    drop(state);
    drop(doomed);
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
        Some("nav") => Msg::Nav(value["nav"].as_str().unwrap_or_default().to_string()),
        Some("dials") => Msg::Dials {
            max_live: value["max_live"].as_u64().unwrap_or(2) as usize,
            idle_minutes: value["idle_minutes"].as_u64().unwrap_or(15),
        },
        Some("config") => Msg::OpenConfig,
        Some("link") => Msg::External(value["url"].as_str().unwrap_or_default().to_string()),
        _ => return,
    };
    let _ = proxy.send_event(msg);
}

fn handle_pane(proxy: &EventLoopProxy<Msg>, email: &str, service: &str, body: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return;
    };
    if value["type"] == "caps" {
        println!("[caps] {body}");
        return;
    }
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

/// In stacked mode the rail is the entire navigation, so it gets wider and the top
/// bar is gone, which means the content pane starts at the very top.
fn rail_width(nav: &str) -> f64 {
    if nav == NAV_STACKED {
        // Three 20px app buttons plus their gaps, with margin either side.
        80.0
    } else {
        RAIL_W
    }
}

fn content_rect(window: &Window, nav: &str) -> Rect {
    let size = window.inner_size().to_logical::<f64>(window.scale_factor());
    let left = rail_width(nav);
    let top = if nav == NAV_STACKED { 0.0 } else { TOPBAR_H };
    rect(
        left,
        top,
        (size.width - left).max(1.0),
        (size.height - top).max(1.0),
    )
}

/// Bounds for the rail and the top bar, given the layout.
fn chrome_rects(window: &Window, nav: &str) -> (Rect, Rect) {
    let size = window.inner_size().to_logical::<f64>(window.scale_factor());
    let left = rail_width(nav);
    (
        rect(0.0, 0.0, left, size.height),
        rect(left, 0.0, (size.width - left).max(1.0), TOPBAR_H),
    )
}

/// Everything the menu bar is made of, which must outlive the process.
///
/// muda stores a raw pointer to each item's `MenuChild` inside the NSMenuItem and
/// dereferences it on every activation. Dropping these handles leaves that pointer
/// dangling, so the first Cmd+1 read a String out of freed memory. Predefined
/// items (Quit, Copy, Paste) are wired straight to AppKit selectors, which is why
/// only the custom items crashed.
struct MenuKeepAlive {
    _menu: Menu,
    _submenus: Vec<Submenu>,
    _items: Vec<MenuItem>,
}

/// Ask AppKit to perform a menu item, exactly as a key equivalent would.
fn fire_menu_item(menu_title: &str, index: usize) {
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;

    // AppKit is main-thread only, so hop there and wait for it.
    let (title, done) = (menu_title.to_string(), std::sync::Arc::new(
        std::sync::atomic::AtomicBool::new(false),
    ));
    let flag = done.clone();
    dispatch_on_main(move || {
        autoreleasepool(|_| {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let app = NSApplication::sharedApplication(mtm);
            let Some(main_menu) = app.mainMenu() else { return };
            for i in 0..unsafe { main_menu.numberOfItems() } {
                let item = unsafe { main_menu.itemAtIndex(i) };
                let Some(item) = item else { continue };
                if unsafe { item.title() }.to_string() != title {
                    continue;
                }
                if let Some(submenu) = unsafe { item.submenu() } {
                    if index < unsafe { submenu.numberOfItems() } as usize {
                        unsafe { submenu.performActionForItemAtIndex(index as isize) };
                    }
                }
            }
        });
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    for _ in 0..200 {
        if done.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn dispatch_on_main<F: FnOnce() + Send + 'static>(work: F) {
    // Minimal main-queue hop without pulling in a dispatch crate.
    let boxed: Box<Box<dyn FnOnce() + Send>> = Box::new(Box::new(work));
    extern "C" fn trampoline(ctx: *mut std::ffi::c_void) {
        let work: Box<Box<dyn FnOnce() + Send>> = unsafe { Box::from_raw(ctx as *mut _) };
        work();
    }
    extern "C" {
        fn dispatch_async_f(
            queue: *mut std::ffi::c_void,
            context: *mut std::ffi::c_void,
            work: extern "C" fn(*mut std::ffi::c_void),
        );
        static _dispatch_main_q: std::ffi::c_void;
    }
    unsafe {
        dispatch_async_f(
            &_dispatch_main_q as *const _ as *mut _,
            Box::into_raw(boxed) as *mut _,
            trampoline,
        );
    }
}

#[must_use = "dropping this dangles the pointers AppKit holds into the menu"]
fn install_menu(accounts: &[Account]) -> MenuKeepAlive {
    let menu = Menu::new();
    let app = Submenu::new("Masse", true);
    let edit = Submenu::new("Edit", true);
    let view = Submenu::new("View", true);
    let go = Submenu::new("Go", true);

    let key = |spec: &str| spec.parse::<Accelerator>().ok();

    let mut items = Vec::new();
    let settings = MenuItem::with_id("settings", "Settings...", true, key("CmdOrCtrl+Comma"));
    items.push(settings.clone());
    let _ = app.append_items(&[
        &PredefinedMenuItem::about(None, None),
        &PredefinedMenuItem::separator(),
        &settings,
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::quit(None),
    ]);

    let reload = MenuItem::with_id("reload", "Reload", true, key("CmdOrCtrl+R"));
    let _ = view.append(&reload);
    items.push(reload);

    // Cmd+1..9 picks an account, Cmd+Shift+1..3 picks an app.
    for (i, account) in accounts.iter().take(9).enumerate() {
        let label = if account.label.is_empty() {
            account.email.clone()
        } else {
            format!("{} ({})", account.label, account.email)
        };
        let item = MenuItem::with_id(
            format!("acct:{i}"),
            label,
            true,
            key(&format!("CmdOrCtrl+{}", i + 1)),
        );
        let _ = go.append(&item);
        items.push(item);
    }
    let _ = go.append(&PredefinedMenuItem::separator());
    for (i, service) in SERVICES.iter().enumerate() {
        let mut label = service.to_string();
        label.get_mut(0..1).map(|c| c.make_ascii_uppercase());
        let item = MenuItem::with_id(
            format!("app:{service}"),
            label,
            true,
            key(&format!("CmdOrCtrl+Shift+{}", i + 1)),
        );
        let _ = go.append(&item);
        items.push(item);
    }
    let _ = edit.append_items(&[
        &PredefinedMenuItem::undo(None),
        &PredefinedMenuItem::redo(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::cut(None),
        &PredefinedMenuItem::copy(None),
        &PredefinedMenuItem::paste(None),
        &PredefinedMenuItem::select_all(None),
    ]);
    let _ = menu.append_items(&[&app, &edit, &view, &go]);
    menu.init_for_nsapp();

    MenuKeepAlive {
        _menu: menu,
        _submenus: vec![app, edit, view, go],
        _items: items,
    }
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
        "nav": config.nav,
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
    type: 'caps',
    notification: typeof Notification,
    permission: (typeof Notification !== 'undefined' && Notification.permission) || null,
    serviceWorker: 'serviceWorker' in navigator,
  }));
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

