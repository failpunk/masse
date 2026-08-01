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
    is_download, MonitorSpot, NAV_STACKED, PALETTE, SERVICES,
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
    /// Load a URL in the visible pane. Used for downloads, which have to stay in the
    /// app: WebKit only turns a response into a download if it makes the request.
    LoadHere(String),
    /// Test hook: make the visible pane attempt a navigation.
    Drive(String),
    /// A menu item fired. Carries muda's item id.
    Menu(String),
    /// Switch between the split layout and everything-in-the-rail.
    Nav(String),
    /// Set an account's highlight colour.
    Colour { email: String, colour: String },
    /// A page-side script grabbed a download itself (blob or fetch) and handed us
    /// the bytes over IPC, because the click that triggered it never reached any
    /// of wry's navigation hooks. See PROBE's click-intercept.
    SaveBlob { name: String, data: String },
    /// Ask the visible pane to fetch a URL itself and post the bytes back, for a
    /// download navigation we cancelled because WebKit would have rendered it
    /// rather than saving it.
    GrabBlob(String),
}

struct App {
    config: Config,
    panes: HashMap<String, WebView>,
    lru: Lru,
    active: String,
    /// The settings modal, alive only while it is open.
    settings: Option<WebView>,
    /// Geometry or last-location changed and has not been written yet. Writing on
    /// every pixel of a drag would hammer the disk.
    dirty: bool,
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

    // What the windowing layer actually sees, which is not always what Displays
    // shows: two identical monitors can report the same name and size, and the
    // scale factor is what makes a remembered physical size land right.
    if std::env::args().any(|a| a == "--monitors") {
        for m in event_loop.available_monitors() {
            let (p, s) = (m.position(), m.size());
            println!(
                "name={:?} pos=({}, {}) size={}x{} scale={}",
                m.name().unwrap_or_default(),
                p.x,
                p.y,
                s.width,
                s.height,
                m.scale_factor()
            );
        }
        println!("saved window : {:?}", config.window);
        println!("saved monitor: {:?}", config.monitor);
        return Ok(());
    }

    let saved = config.window;
    // Deliberately NOT sized from `saved` here. A physical size given at build time
    // is resolved against the display the window is born on, not the one it is
    // about to be moved to, which silently halved the window on every launch.
    // restore_position applies the remembered size once the target display, and so
    // the right scale factor, is known.
    let window = WindowBuilder::new()
        .with_title("Masse")
        .with_min_inner_size(LogicalSize::new(680.0, 480.0))
        .with_inner_size(LogicalSize::new(1340.0, 900.0))
        .build(&event_loop)
        .expect("window");
    restore_position(&window, saved, config.monitor.as_ref());

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
        dirty: false,
        rescued: std::collections::HashSet::new(),
    }));

    event_loop.run(move |event, _target, control_flow| {
        // Captured so the menu's allocations live as long as the process does.
        // AppKit holds raw pointers into them. Do not remove.
        let _keep = &menu_keepalive;

        *control_flow = ControlFlow::WaitUntil(std::time::Instant::now() + SWEEP);
        reclaim_idle(&app);
        flush(&app);

        match event {
            Event::NewEvents(StartCause::Init) => {
                // The window is on its final display by now, so its scale factor is
                // the one the remembered physical size was recorded against.
                restore_size(&window, saved);

                // Size the chrome from the real window, not the constants it was
                // built with.
                let nav = app.borrow().config.nav.clone();
                let (rail_r, top_r) = chrome_rects(&window, &nav);
                let _ = chrome.rail.set_bounds(rail_r);
                let _ = chrome.topbar.set_bounds(top_r);

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

            Event::UserEvent(Msg::Colour { email, colour }) => {
                // Never write junk into the config, whatever the UI sends.
                if !config::is_hex_colour(&colour) {
                    eprintln!("[masse] ignoring colour {colour:?}");
                    return;
                }
                let payload = {
                    let Ok(mut state) = app.try_borrow_mut() else { return };
                    let wanted = email.trim().to_lowercase();
                    match state
                        .config
                        .accounts
                        .iter_mut()
                        .find(|a| a.email.trim().to_lowercase() == wanted)
                    {
                        Some(account) => account.color = colour.clone(),
                        None => return,
                    }
                    state.config.save();
                    println!("[masse] {email} highlight -> {colour}");
                    rail_state(&state.config, &state.active)
                };
                chrome.push(&payload);
                if let Ok(state) = app.try_borrow() {
                    if let Some(view) = &state.settings {
                        let _ = view.evaluate_script(&format!("window.shim.render({payload})"));
                    }
                }
            }

            Event::UserEvent(Msg::LoadHere(url)) => {
                let Ok(state) = app.try_borrow() else { return };
                if let Some(view) = state.panes.get(&state.active) {
                    let _ = view.load_url(&url);
                }
            }

            Event::UserEvent(Msg::SaveBlob { name, data }) => {
                let Some(bytes) = b64_decode(&data) else {
                    eprintln!("[masse] blob save: could not decode {name}");
                    return;
                };
                let Some(path) = unique_download_path(&name) else {
                    eprintln!("[masse] blob save: no HOME, dropping {name}");
                    return;
                };
                match std::fs::write(&path, &bytes) {
                    Ok(()) => {
                        println!("[masse] saved {}", path.display());
                        let _ = std::process::Command::new("open").arg("-R").arg(&path).spawn();
                    }
                    Err(err) => eprintln!("[masse] blob save failed: {err}"),
                }
            }

            Event::UserEvent(Msg::GrabBlob(url)) => {
                let Ok(state) = app.try_borrow() else { return };
                if let Some(view) = state.panes.get(&state.active) {
                    let arg = serde_json::to_string(&url).unwrap_or_else(|_| "\"\"".into());
                    let _ = view.evaluate_script(&format!("window.__masseGrab && window.__masseGrab({arg})"));
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
                event: WindowEvent::Moved(_),
                ..
            } => remember_geometry(&app, &window),

            Event::WindowEvent {
                event: WindowEvent::Resized(_),
                ..
            } => {
                remember_geometry(&app, &window);
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
                remember_geometry(&app, &window);
                if let Ok(mut state) = app.try_borrow_mut() {
                    remember_location(&mut state);
                }
                flush(&app);
                *control_flow = ControlFlow::Exit;
            }

            // Cmd+Q and a quit AppleEvent both terminate without ever sending
            // CloseRequested, which is why geometry was never once saved.
            Event::LoopDestroyed => {
                remember_geometry(&app, &window);
                if let Ok(mut state) = app.try_borrow_mut() {
                    remember_location(&mut state);
                }
                flush(&app);
            }

            _ => {}
        }
    });
}

/// Put the window back where it was, preferring the display it was on.
///
/// Absolute coordinates alone are not enough: unplug a monitor or drag it to the
/// other side in Displays and the same coordinate belongs to a different screen, or
/// to no screen at all. So the remembered display is matched first, by name and
/// size, and the window is placed at its old offset within that display.
fn restore_position(window: &Window, saved: Option<[f64; 4]>, spot: Option<&MonitorSpot>) {
    let monitors: Vec<_> = window.available_monitors().collect();
    let contains = |x: f64, y: f64| {
        monitors.iter().any(|m| {
            let (o, sz) = (m.position(), m.size());
            let (l, t) = (o.x as f64, o.y as f64);
            x >= l - 8.0 && y >= t - 8.0 && x < l + sz.width as f64 && y < t + sz.height as f64
        })
    };
    let shapes: Vec<Screen> = monitors
        .iter()
        .map(|m| {
            let (o, sz) = (m.position(), m.size());
            Screen {
                name: m.name().unwrap_or_default(),
                w: sz.width as f64,
                h: sz.height as f64,
                ox: o.x as f64,
                oy: o.y as f64,
            }
        })
        .collect();

    if let Some(spot) = spot {
        let match_ = pick_monitor(&shapes, spot).map(|i| &monitors[i]);
        if let Some(m) = match_ {
            let o = m.position();
            let x = o.x as f64 + spot.dx;
            let y = o.y as f64 + spot.dy;
            let label = if spot.name.is_empty() { "unnamed display" } else { &spot.name };
            println!(
                "[masse] restoring onto {label} at ({}, {}) +{}, +{}",
                o.x, o.y, spot.dx, spot.dy
            );
            window.set_outer_position(tao::dpi::PhysicalPosition::new(x, y));
            return;
        }
        println!("[masse] the display it was last on is gone, falling back");
    }

    match saved {
        Some([_, _, x, y]) if contains(x, y) => {
            window.set_outer_position(tao::dpi::PhysicalPosition::new(x, y));
        }
        Some(_) => println!("[masse] saved position is off every display, ignoring it"),
        None => {}
    }
}

/// The identifying facts about one display, lifted out of tao so the matching
/// below can be tested without a window server.
#[derive(Debug, Clone, PartialEq)]
struct Screen {
    name: String,
    w: f64,
    h: f64,
    ox: f64,
    oy: f64,
}

/// Which display a remembered spot refers to.
///
/// Origin is checked first and name/size only as a fallback, because two identical
/// external monitors report the SAME name and the SAME size. Matching on those
/// alone always returned the first of the pair, which is why the window kept
/// reopening on the wrong screen. The fallback still matters: a display that has
/// been moved in Displays keeps its identity but changes origin.
fn pick_monitor(screens: &[Screen], spot: &MonitorSpot) -> Option<usize> {
    let same_shape = |s: &Screen| s.name == spot.name && s.w == spot.w && s.h == spot.h;
    screens
        .iter()
        .position(|s| same_shape(s) && s.ox == spot.ox && s.oy == spot.oy)
        .or_else(|| screens.iter().position(same_shape))
}

/// Apply the remembered size, which must happen AFTER the window has actually
/// landed on its target display.
///
/// Every size tao accepts is resolved against the window's current scale factor,
/// so asking for one before the move has registered uses the OLD display's scale.
/// Both failure modes were real here: sizing at build time on the 2x built-in
/// halved the window on a 1x monitor every launch, and sizing immediately after
/// `set_outer_position` doubled it instead, because the move had not landed yet.
/// Called from the first loop iteration, by which point the scale factor is true.
fn restore_size(window: &Window, saved: Option<[f64; 4]>) {
    let Some([w, h, ..]) = saved else { return };
    if w < 400.0 || h < 300.0 {
        return;
    }
    println!(
        "[masse] restoring size {w}x{h} physical (window scale now {})",
        window.scale_factor()
    );
    window.set_inner_size(tao::dpi::PhysicalSize::new(w, h));
}

/// Record where the window is, in physical pixels so it survives moving between
/// monitors with different scale factors.
fn remember_geometry(app: &Rc<RefCell<App>>, window: &Window) {
    let Ok(mut state) = app.try_borrow_mut() else { return };
    let size = window.inner_size();
    let Ok(pos) = window.outer_position() else { return };
    let next = [size.width as f64, size.height as f64, pos.x as f64, pos.y as f64];

    // Also note the display and the offset within it, so rearranging monitors does
    // not send the window to whichever screen now owns that absolute coordinate.
    let spot = window.current_monitor().map(|m| {
        let (o, sz) = (m.position(), m.size());
        MonitorSpot {
            name: m.name().unwrap_or_default(),
            w: sz.width as f64,
            h: sz.height as f64,
            dx: pos.x as f64 - o.x as f64,
            dy: pos.y as f64 - o.y as f64,
            ox: o.x as f64,
            oy: o.y as f64,
            scale: m.scale_factor(),
        }
    });

    if state.config.window != Some(next) || state.config.monitor != spot {
        state.config.window = Some(next);
        state.config.monitor = spot;
        state.dirty = true;
    }
}

/// Note which account and app is on screen, for the next launch.
fn remember_location(state: &mut App) {
    let (email, service) = state
        .active
        .split_once('\u{1}')
        .map(|(e, s)| (e.to_string(), s.to_string()))
        .unwrap_or_default();
    if email.is_empty() || service == ADD {
        return;
    }
    let next = Some([email, service]);
    if state.config.last != next {
        state.config.last = next;
        state.dirty = true;
    }
}

/// Write pending changes. Called from the periodic sweep and on the way out, so
/// nothing depends on one particular exit path firing.
fn flush(app: &Rc<RefCell<App>>) {
    let Ok(mut state) = app.try_borrow_mut() else { return };
    if !state.dirty {
        return;
    }
    state.config.save();
    state.dirty = false;
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
                    // An attachment opens as a new window too. Denying it means the
                    // request is never made and nothing downloads; sending it to the
                    // browser means signing in again there. So it goes to the pane,
                    // where WebKit sees the attachment response and downloads it.
                    if is_download(&url) {
                        println!("[masse] download link: {}", &url[..url.len().min(120)]);
                        let _ = proxy.send_event(Msg::LoadHere(url));
                    } else if route_link(&service, &url) != Route::Drop {
                        let _ = proxy.send_event(Msg::External(url));
                    } else {
                        println!("[masse] dropped link: {}", &url[..url.len().min(120)]);
                    }
                    wry::NewWindowResponse::Deny
                }
            })
            // Same-pane navigations to anywhere that is not this app.
            .with_navigation_handler({
                let proxy = proxy.clone();
                let service = service.to_string();
                move |url| match route(&service, &url) {
                    Route::Stay => {
                        // Gmail's download button navigates (in a hidden frame) to
                        // the attachment URL and expects the browser to save the
                        // response. WebKit only turns a response into a download
                        // when it cannot display the MIME type, so a JPEG just
                        // renders into nowhere and the click appears to do nothing.
                        // Cancel it and have the page fetch the bytes instead.
                        if is_download(&url) {
                            println!("[masse] download nav caught: {}", &url[..url.len().min(140)]);
                            let _ = proxy.send_event(Msg::GrabBlob(url));
                            return false;
                        }
                        true
                    }
                    Route::Drop => {
                        if !url.contains("doubleclick") && !url.contains("analytics") {
                            println!("[masse] dropped nav: {}", &url[..url.len().min(120)]);
                        }
                        false
                    }
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
                println!(
                    "[masse] download started: {} -> {}",
                    &url[..url.len().min(100)],
                    path.display()
                );
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
    remember_location(&mut state);

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
        Some("color") => Msg::Colour {
            email: value["email"].as_str().unwrap_or_default().to_string(),
            colour: value["color"].as_str().unwrap_or_default().to_string(),
        },
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
    if value["type"] == "jslog" {
        println!("[js] {}", value["msg"].as_str().unwrap_or_default());
        return;
    }
    if value["type"] == "downloadUrl" {
        let Some(url) = value["url"].as_str() else { return };
        println!("[masse] page-caught download url: {}", &url[..url.len().min(120)]);
        let _ = proxy.send_event(Msg::LoadHere(url.to_string()));
        return;
    }
    if value["type"] == "save" {
        let (Some(name), Some(data)) = (value["name"].as_str(), value["data"].as_str()) else {
            return;
        };
        println!("[masse] blob save: {name} ({} bytes b64)", data.len());
        let _ = proxy.send_event(Msg::SaveBlob {
            name: name.to_string(),
            data: data.to_string(),
        });
    }
}

/// Decodes a standard (or data-URL) base64 payload. Hand-rolled rather than a
/// crate: it is twenty lines, and pulling in a dependency for one call site
/// works against why this app is written in Rust over Electron in the first
/// place.
fn b64_decode(input: &str) -> Option<Vec<u8>> {
    let body = input.split(',').next_back().unwrap_or(input);
    let mut val = 0u32;
    let mut bits = 0u32;
    let mut out = Vec::with_capacity(body.len() / 4 * 3);
    for c in body.bytes() {
        let digit = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\n' | b'\r' => continue,
            _ => return None,
        } as u32;
        val = (val << 6) | digit;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((val >> bits) as u8);
        }
    }
    Some(out)
}

/// Whatever the page suggested for a filename, made safe to put in a path:
/// no separators, no leading dot, never empty.
fn safe_filename(name: &str) -> String {
    let cleaned: String = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .chars()
        .map(|c| if c.is_control() { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.');
    if cleaned.is_empty() {
        "download".to_string()
    } else {
        cleaned.to_string()
    }
}

/// `~/Downloads/name`, or `~/Downloads/name (2)` etc. if that name is taken.
/// Never overwrites an existing file the way a raw write would.
fn unique_download_path(name: &str) -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut dir = std::path::PathBuf::from(home);
    dir.push("Downloads");
    let name = safe_filename(name);
    let (stem, ext) = match name.rfind('.') {
        Some(0) | None => (name.as_str(), ""),
        Some(at) => (&name[..at], &name[at..]),
    };
    let mut candidate = dir.join(&name);
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    Some(candidate)
}

#[cfg(test)]
mod monitor_tests {
    use super::{pick_monitor, Screen};
    use crate::config::MonitorSpot;

    fn screen(name: &str, ox: f64, oy: f64) -> Screen {
        Screen { name: name.into(), w: 2560.0, h: 1440.0, ox, oy }
    }

    fn spot(name: &str, ox: f64, oy: f64) -> MonitorSpot {
        MonitorSpot {
            name: name.into(),
            w: 2560.0,
            h: 1440.0,
            dx: 0.0,
            dy: 30.0,
            ox,
            oy,
            scale: 1.0,
        }
    }

    #[test]
    fn two_identical_monitors_are_told_apart_by_origin() {
        // The real setup that caused this: both externals report the same name and
        // the same size, and differ only in where they sit.
        let screens = vec![
            screen("Monitor #41040", 0.0, 0.0),
            screen("Monitor #12857", 1728.0, -516.0),
            screen("Monitor #12857", -2560.0, -540.0),
        ];
        assert_eq!(pick_monitor(&screens, &spot("Monitor #12857", -2560.0, -540.0)), Some(2));
        assert_eq!(pick_monitor(&screens, &spot("Monitor #12857", 1728.0, -516.0)), Some(1));
    }

    #[test]
    fn a_display_that_moved_is_still_recognised_by_name_and_size() {
        let screens = vec![screen("Monitor #12857", 4000.0, 0.0)];
        assert_eq!(pick_monitor(&screens, &spot("Monitor #12857", 1728.0, -516.0)), Some(0));
    }

    #[test]
    fn a_display_that_is_gone_matches_nothing() {
        let screens = vec![screen("Monitor #41040", 0.0, 0.0)];
        assert_eq!(pick_monitor(&screens, &spot("Monitor #12857", 1728.0, -516.0)), None);
    }

    #[test]
    fn an_old_config_without_an_origin_still_finds_its_display() {
        // Upgrades: ox/oy default to 0, so only the name-and-size fallback can hit.
        let screens = vec![
            screen("Monitor #41040", 0.0, 0.0),
            screen("Monitor #12857", 1728.0, -516.0),
        ];
        assert_eq!(pick_monitor(&screens, &spot("Monitor #12857", 0.0, 0.0)), Some(1));
    }
}

#[cfg(test)]
mod download_tests {
    use super::{b64_decode, safe_filename};

    #[test]
    fn decodes_plain_base64() {
        assert_eq!(b64_decode("aGVsbG8=").unwrap(), b"hello");
    }

    #[test]
    fn decodes_a_data_url_by_taking_the_part_after_the_comma() {
        assert_eq!(
            b64_decode("data:image/jpeg;base64,aGVsbG8=").unwrap(),
            b"hello"
        );
    }

    #[test]
    fn decodes_input_with_no_padding() {
        assert_eq!(b64_decode("aGVsbG8").unwrap(), b"hello");
    }

    #[test]
    fn rejects_bytes_outside_the_base64_alphabet() {
        assert!(b64_decode("not valid!!").is_none());
    }

    #[test]
    fn filename_strips_any_path_and_leading_dots() {
        assert_eq!(safe_filename("../../etc/passwd"), "passwd");
        assert_eq!(safe_filename("...secret"), "secret");
        assert_eq!(safe_filename("Attachment0.jpeg"), "Attachment0.jpeg");
    }

    #[test]
    fn filename_never_comes_back_empty() {
        assert_eq!(safe_filename(""), "download");
        assert_eq!(safe_filename("."), "download");
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
        // The apps stack vertically under each account, so the rail only has to be
        // one circle wide.
        66.0
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
        "palette": PALETTE,
        "max_live": config.max_live,
        "idle_minutes": config.idle_minutes,
    })
    .to_string()
}


const PROBE: &str = r#"
(() => {
  // Gmail/Drive's download icons never reach wry's navigation hooks: the click
  // is handled entirely in page JS (fetch + blob + a synthetic <a download>),
  // both for the attachment-chip icon and the preview/"print window" overlay's
  // icon. So downloads are caught here, at the click, instead of relying on
  // WebKit to notice a response and turn it into a native download.
  function isDownloadHref(href) {
    if (!href) return false;
    if (href.startsWith('blob:')) return true;
    if (/[?&](view=att|export=download)(&|$)/.test(href)) return true;
    try {
      const host = new URL(href, location.href).hostname;
      if (host === 'mail-attachment.googleusercontent.com' || host === 'drive.usercontent.google.com') return true;
      if (host.endsWith('.googleusercontent.com') && href.includes('/download')) return true;
    } catch (e) {}
    return false;
  }
  function nameFor(anchor, url, disposition) {
    if (anchor && anchor.hasAttribute('download') && anchor.getAttribute('download')) {
      return anchor.getAttribute('download');
    }
    if (disposition) {
      const m = /filename\*?=(?:UTF-8'')?"?([^";]+)"?/i.exec(disposition);
      if (m) { try { return decodeURIComponent(m[1]); } catch (e) { return m[1]; } }
    }
    // Gmail's attachment URLs carry no filename in the path, so fall back to the
    // name it puts in its own download button before giving up.
    const button = document.querySelector('[aria-label^="Download attachment "]');
    if (button) {
      const named = button.getAttribute('aria-label').replace(/^Download attachment\s+/, '').trim();
      if (named) return named;
    }
    try {
      const last = new URL(url, location.href).pathname.split('/').filter(Boolean).pop();
      if (last && last.includes('.')) return decodeURIComponent(last);
    } catch (e) {}
    return 'download';
  }
  function grabBlob(url, anchor, forcedName) {
    fetch(url, { credentials: 'include' })
      .then((r) => r.blob().then((blob) => ({ blob, disposition: r.headers.get('content-disposition') })))
      .then(({ blob, disposition }) => {
        const reader = new FileReader();
        reader.onload = () => {
          window.ipc.postMessage(JSON.stringify({
            type: 'save', name: forcedName || nameFor(anchor, url, disposition), data: reader.result,
          }));
        };
        reader.readAsDataURL(blob);
      })
      .catch((e) => window.ipc.postMessage(JSON.stringify({ type: 'jslog', msg: 'download fetch failed: ' + e })));
  }
  // Called from the Rust side when a download navigation was cancelled.
  window.__masseGrab = (url) => grabBlob(url, null, null);
  // Gmail's download control is a <button aria-label="Download attachment X">,
  // not a link, and the URL lives on the surrounding attachment card in a
  // `download_url` attribute shaped "mime:name:url". Nothing about this click
  // ever becomes a navigation wry can see, so it has to be caught here.
  function attachmentDownload(el) {
    let node = el;
    for (let i = 0; node && i < 12; i++) {
      const label = node.getAttribute && node.getAttribute('aria-label');
      if (label && /^Download\b/i.test(label)) {
        // Found the button. The card holding the URL is somewhere above it.
        let card = node;
        for (let j = 0; card && j < 12; j++) {
          const holder = card.querySelector ? card.querySelector('[download_url]') : null;
          const own = card.getAttribute && card.getAttribute('download_url');
          const raw = own || (holder && holder.getAttribute('download_url'));
          if (raw) {
            // "image/jpeg:Attachment0.jpeg:https://mail.google.com/..."
            const first = raw.indexOf(':');
            const second = raw.indexOf(':', first + 1);
            if (second > -1) {
              return { url: raw.slice(second + 1), name: raw.slice(first + 1, second) };
            }
          }
          card = card.parentElement;
        }
        // The button is there but the card is not shaped as expected. Let the
        // click through: the navigation it triggers is caught on the Rust side.
        return null;
      }
      node = node.parentElement;
    }
    return null;
  }
  document.addEventListener('click', (event) => {
    const attachment = attachmentDownload(event.target);
    if (attachment) {
      event.preventDefault();
      event.stopPropagation();
      grabBlob(attachment.url, null, attachment.name);
      return;
    }
    let el = event.target;
    while (el && el !== document.body) {
      if (el.tagName === 'A' && el.href) {
        if (el.href.startsWith('blob:')) {
          event.preventDefault();
          event.stopPropagation();
          grabBlob(el.href, el);
        } else if (el.hasAttribute('download') || isDownloadHref(el.href)) {
          event.preventDefault();
          event.stopPropagation();
          // Loading this URL in the pane (the old approach) never produced a
          // real file: WebKit's download delegate does not fire for a plain
          // load_url() navigation the way it does for a new-window request.
          // Fetching and saving the bytes ourselves does not depend on that
          // delegate at all, and this URL is same-origin with the page, so
          // there is no CORS obstacle to the fetch.
          grabBlob(el.href, el);
        }
        break;
      }
      el = el.parentElement;
    }
  }, true);
})();
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

