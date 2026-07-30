// Spike, not the product. It exists to answer two questions before any real
// code gets written:
//
//   1. Will Google let you sign in inside a WKWebView? If it shows the
//      "this browser or app may not be secure" wall, the whole native plan is
//      dead and Electron is the only path left.
//   2. Can one window host several webviews that are created and destroyed
//      independently? That is the mechanism the entire memory argument rests on.
//
// wry and tao are the webview and windowing crates Tauri is built on, so this
// exercises the identical WKWebView path with none of Tauri's config scaffolding.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use tao::{
    dpi::LogicalSize,
    event::{Event, StartCause, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
};
use muda::{Menu, PredefinedMenuItem, Submenu};
use wry::{
    dpi::{LogicalPosition, LogicalSize as WrySize},
    Rect, WebView, WebViewBuilder,
};

const RAIL_W: f64 = 68.0;

// WKWebView's default user agent omits the "Version/x Safari/y" tail, which is
// one of the things Google's sign-in checks sniff for.
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
                  (KHTML, like Gecko) Version/18.5 Safari/605.1.15";

#[derive(Debug)]
enum UserEvent {
    /// Show the pane for this key, creating it if it does not exist yet.
    Show(String),
    /// Destroy every pane except the active one, to watch the memory come back.
    EvictOthers,
}

fn url_for(key: &str) -> &'static str {
    match key {
        "calendar" => "https://calendar.google.com/calendar/r",
        "drive" => "https://drive.google.com/drive/my-drive",
        // Signed out, mail.google.com just redirects to marketing. Go at the
        // sign-in flow directly, which is where Google's embedded-webview
        // rejection would appear if it is going to appear at all.
        "signin" => "https://accounts.google.com/ServiceLogin?service=mail&continue=https://mail.google.com/mail/u/0/",
        _ => "https://mail.google.com/mail/u/0/",
    }
}

fn main() -> wry::Result<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Without a real Edit menu macOS never delivers Cmd+C/V/X or Cmd+A to a
    // WKWebView, so you cannot paste a password into it. The menu items have to
    // exist for the key equivalents to be routed at all.
    install_menu();

    let window = WindowBuilder::new()
        .with_title("Shim spike")
        .with_inner_size(LogicalSize::new(1340.0, 900.0))
        .build(&event_loop)
        .expect("window");

    let rail = WebViewBuilder::new()
        .with_bounds(rect(0.0, 0.0, RAIL_W, 900.0))
        .with_html(RAIL_HTML)
        .with_ipc_handler(move |req| {
            let body = req.body().to_string();
            let event = if body == "evict" {
                UserEvent::EvictOthers
            } else {
                UserEvent::Show(body)
            };
            let _ = proxy.send_event(event);
        })
        .build_as_child(&window)?;

    // `--bench` scripts the pane lifecycle so memory can be sampled from outside
    // without anyone having to click. Nothing else uses it.
    if std::env::args().any(|a| a == "--bench") {
        let proxy = event_loop.create_proxy();
        std::thread::spawn(move || {
            for (delay, step) in [(7, "signin"), (7, "calendar"), (7, "drive")] {
                std::thread::sleep(std::time::Duration::from_secs(delay));
                println!("[bench] MARK open {step}");
                let _ = proxy.send_event(UserEvent::Show(step.to_string()));
            }
            std::thread::sleep(std::time::Duration::from_secs(8));
            println!("[bench] MARK evict");
            let _ = proxy.send_event(UserEvent::EvictOthers);
        });
    }

    // The pane pool. Panes are built on first request and dropped on eviction;
    // dropping a WKWebView tears down its content process, which is what
    // actually returns the memory.
    let panes: Rc<RefCell<HashMap<String, WebView>>> = Rc::new(RefCell::new(HashMap::new()));
    let active = Rc::new(RefCell::new(String::from("mail")));

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;

        let content_rect = || {
            let size = window.inner_size().to_logical::<f64>(window.scale_factor());
            rect(RAIL_W, 0.0, (size.width - RAIL_W).max(0.0), size.height)
        };

        match event {
            Event::NewEvents(StartCause::Init) => {
                let _ = proxy_show(&panes, &active, "signin", &window, content_rect());
            }

            Event::UserEvent(UserEvent::Show(key)) => {
                let _ = proxy_show(&panes, &active, &key, &window, content_rect());
                report(&panes.borrow());
            }

            Event::UserEvent(UserEvent::EvictOthers) => {
                let keep = active.borrow().clone();
                panes.borrow_mut().retain(|k, _| *k == keep);
                report(&panes.borrow());
            }

            Event::WindowEvent {
                event: WindowEvent::Resized(_),
                ..
            } => {
                let size = window.inner_size().to_logical::<f64>(window.scale_factor());
                let _ = rail.set_bounds(rect(0.0, 0.0, RAIL_W, size.height));
                if let Some(view) = panes.borrow().get(&*active.borrow()) {
                    let _ = view.set_bounds(content_rect());
                }
            }

            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                let _ = target;
                *control_flow = ControlFlow::Exit;
            }

            _ => {}
        }
    });
}

/// Bring `key` to the front, building its webview the first time it is asked for.
fn proxy_show(
    panes: &Rc<RefCell<HashMap<String, WebView>>>,
    active: &Rc<RefCell<String>>,
    key: &str,
    window: &tao::window::Window,
    bounds: Rect,
) -> wry::Result<()> {
    let mut map = panes.borrow_mut();

    if !map.contains_key(key) {
        let label = key.to_string();
        let view = WebViewBuilder::new()
            .with_bounds(bounds)
            .with_user_agent(UA)
            .with_url(url_for(key))
            // Report what Google actually served. Its embedded-webview rejection
            // is recognisable by URL (disallowed_useragent / deniedsigninrejected)
            // and by the heading text, so this answers question 1 without a
            // screenshot and without anyone typing a password.
            .with_initialization_script(PROBE)
            .with_ipc_handler(move |req| println!("[{label}] {}", req.body()))
            .build_as_child(window)?;
        map.insert(key.to_string(), view);
        println!("[spike] built pane '{key}'");
    }

    // Hide every other pane rather than destroying it, so switching back is instant.
    for (other, view) in map.iter() {
        let _ = view.set_visible(other == key);
    }
    if let Some(view) = map.get(key) {
        let _ = view.set_bounds(bounds);
        let _ = view.focus();
    }
    *active.borrow_mut() = key.to_string();
    Ok(())
}

fn report(map: &HashMap<String, WebView>) {
    let mut keys: Vec<_> = map.keys().cloned().collect();
    keys.sort();
    println!("[spike] live panes: {} {:?}", keys.len(), keys);
}

fn install_menu() {
    let menu = Menu::new();
    let app = Submenu::new("Shim", true);
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

const PROBE: &str = r#"
window.addEventListener('load', () => {
  const heading = document.querySelector('h1, [role=heading]');
  window.ipc.postMessage(JSON.stringify({
    url: location.href.slice(0, 160),
    title: document.title,
    heading: heading ? heading.textContent.trim().slice(0, 120) : null,
    blocked: /disallowed_useragent|deniedsigninrejected|browser_not_secure/i.test(location.href)
             || /not secure|couldn.t sign you in/i.test(document.body.innerText.slice(0, 4000)),
  }));
});
"#;

const RAIL_HTML: &str = r#"<!doctype html>
<meta charset="utf-8">
<style>
  :root { color-scheme: dark; }
  body {
    margin: 0; height: 100vh; background: #11131a; color: #fff;
    display: flex; flex-direction: column; align-items: center; gap: 6px;
    padding-top: 14px; box-sizing: border-box;
    font: 500 10px/1.2 -apple-system, system-ui, sans-serif;
    -webkit-user-select: none;
  }
  button {
    all: unset; cursor: pointer; width: 46px; height: 46px; border-radius: 13px;
    display: grid; place-items: center; color: rgba(255,255,255,.6);
    background: rgba(255,255,255,.05); font-size: 9px; text-align: center;
  }
  button:hover { background: rgba(255,255,255,.14); color: #fff; }
  button.on { background: #fff; color: #11131a; }
  .evict { margin-top: auto; margin-bottom: 14px; background: rgba(255,80,80,.16); color: #ff9b9b; }
</style>
<button data-k="signin" class="on">Sign in</button>
<button data-k="mail">Mail</button>
<button data-k="calendar">Cal</button>
<button data-k="drive">Drive</button>
<button class="evict" data-k="evict">Evict</button>
<script>
  for (const b of document.querySelectorAll('button')) {
    b.onclick = () => {
      const k = b.dataset.k;
      if (k !== 'evict') {
        for (const o of document.querySelectorAll('button')) o.classList.toggle('on', o === b);
      }
      window.ipc.postMessage(k);
    };
  }
</script>"#;
