# Masse

A small macOS app for working across several Google accounts: Mail, Calendar and
Drive for each, in one window, without a second browser.

Accounts run down the left, apps across the top. The two are independent, so
switching account keeps the app you are in and switching app keeps the account.

## Why it is small

It uses the system WebKit through [wry](https://github.com/tauri-apps/wry) rather
than bundling Chromium, so the shell is under 1 MB and about 90 MB resident. But
the shell was never the problem: Gmail costs 300-500 MB wherever it runs, and
Shift's footprint comes from keeping nine of them loaded at once.

So the real mechanism is that panes you are not looking at are **destroyed**, not
hidden. Dropping a WebView tears down its WebKit content process and the memory
returns to the system. Two pressures decide what goes, both in
`~/.config/shim/accounts.json`:

| setting | effect |
| --- | --- |
| `max_live` | how many panes may be loaded at once (default 2) |
| `idle_minutes` | destroy a pane untouched for this long (default 15, 0 disables) |

Measured on one machine, `max_live: 2` with Gmail and Calendar both live sat around
1.3 GB, against Shift's 2.7 GB and climbing. `max_live: 1` lands near 500 MB at the
cost of a reload on every switch.

## Build and run

```bash
cd app && ./tools/bundle.sh && open target/Shim.app
```

Requires Rust and Xcode command line tools. Nothing else, no npm, no bundler.

## Layout

| | |
| --- | --- |
| [app/src/main.rs](app/src/main.rs) | window, pane pool, the event loop |
| [app/src/ui.rs](app/src/ui.rs) | the account rail and app tabs |
| [app/src/lru.rs](app/src/lru.rs) | which panes get destroyed, and when |
| [app/src/config.rs](app/src/config.rs) | accounts file, URL building |
| [app/tools/bundle.sh](app/tools/bundle.sh) | packages Shim.app |
| [spike/](spike/) | the throwaway that proved the approach was viable |

## Keys

| | |
| --- | --- |
| Cmd+1 .. Cmd+9 | switch account, keeping the current app |
| Cmd+Shift+1/2/3 | Mail, Calendar, Drive, keeping the current account |
| Cmd+R | reload the visible pane |
| Cmd+, | settings |
| Cmd+Q | quit. Use this, see below |

## Things worth knowing before changing anything

- **The bundle identifier and the session store identifier are load bearing.**
  `com.failpunk.shim` in `bundle.sh` and `SESSION_STORE` in `main.rs` together
  decide where cookies live. Change either and every account is signed out.
- **Never hold a `RefCell` borrow across an AppKit or WebKit call.** Creating,
  destroying, focusing or scripting a WebView pumps the macOS run loop, which
  re-enters the tao event handler. A borrow still live at that moment panics and,
  with `panic = "abort"`, takes the process down with no message. This crashed the
  app on Cmd+1 in 0.8.0. Borrows reachable from a re-entrant path use
  `try_borrow`, and views are built and dropped unborrowed. `--stress` exercises it.
- **Never `kill` this app.** WebKit flushes its cookie jar on clean shutdown and
  discards it on a hard kill, so `pkill masse` signs you out of every account.
  Quit with Cmd+Q, or `osascript -e 'tell application "Masse" to quit'`.
  `bundle.sh` does this for you before replacing the bundle.
- **Cookies need an explicit data store.** The default `WKWebsiteDataStore` in an
  app without a full signing identity persists LocalStorage and IndexedDB but keeps
  cookies in memory, so every launch was a fresh login. `with_data_store_identifier`
  fixes it.
- **The Edit menu is not decoration.** Without it macOS never routes Cmd+V into a
  WebView, so you cannot paste a password into Google's login form.
- **Link routing is an allowlist, not a blocklist.** A pane may navigate to its
  own app, the `accounts.*` login flow, and its own preview hosts. Everything else
  goes to the browser, and known beacon hosts are dropped so they cannot become
  browser tabs. wry cannot tell a subframe navigation from a top-level one, which
  is why the drop list exists at all. See `route()` in `config.rs`.
- **Accounts are addressed by email**, via `?authuser=`, never by the `/u/0` index.
  Those indices shift when a login is added or removed.
- **Passkeys cannot work.** WebAuthn in a WebView needs an Apple entitlement granted
  only to apps that qualify as browsers. Use a password and a non-passkey factor.
