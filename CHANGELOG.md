# Changelog

## 0.15.0
- **Meeting links open as the right account.** A Meet link carries no account, so
  the browser resolves it against whichever Google account it happens to have
  first, which for anyone running meetings from several accounts is usually the
  wrong one. Google's own calendar behaves this way too. Masse knows which
  account's calendar the click came from, so it now stamps `authuser=` onto
  outbound Google links on the way out. Non-Google links are untouched, because
  `authuser` is Google's parameter and means nothing to Zoom. The sign-in host is
  excluded so this cannot fight a login in progress. This only helps if the
  browser is already signed into that account; otherwise `authuser` lands on a
  sign-in page rather than guessing.
- **One download path instead of three.** 0.14.2 caught downloads three ways: the
  cancelled navigation, a click interceptor reading a `download_url` attribute off
  Gmail's attachment card, and loading the URL into the visible pane. Only the
  first ever fired. The click interceptor never matched current Gmail markup, and
  loading the URL into the pane is what took over the whole window and left a pane
  with no way back to the inbox. Both are gone. What remains is the cancelled
  navigation, which covers every surface (the attachment chip, the preview
  overlay, Drive) precisely because it hooks the navigation rather than any
  particular button.
- Verified end to end rather than by inspection: a downloaded attachment's SHA-256
  matches the same file fetched by a real browser, and the page-side
  fetch-and-encode step now has its own test, since it lives in a string literal
  the Rust compiler never checks.

## 0.14.2
- **Attachment downloads work.** Clicking Gmail's download icon did nothing at
  all. The click does reach WebKit as a navigation to the attachment URL, but wry
  only turns a response into a download when WebKit cannot display the MIME type,
  so a JPEG rendered invisibly into a hidden frame and no file was ever written.
  That navigation is now cancelled and the bytes are fetched in the page and
  handed to the native side, which writes them to `~/Downloads` without
  overwriting an existing file. Gmail's download control is a `<button>` carrying
  the URL in a `download_url` attribute on the surrounding card, not a link, so
  the button is caught directly too; the cancelled-navigation path stays as the
  backstop for the other attachment surfaces.
- **The window reopens where and how it was left.** Two faults, both only visible
  on a multi-display setup. Two identical external monitors report the SAME name
  and the SAME size, so matching on those alone always returned the first of the
  pair and the window came back on the wrong screen; the display's origin is now
  what identifies it. Separately, every size tao accepts is resolved against the
  window's current scale factor, so a remembered physical size applied at build
  time used the 2x built-in's scale and halved the window on a 1x monitor, every
  launch, until it hit the minimum. The size is now applied on the first loop
  iteration, once the window is actually on its target display.
- `--monitors` dumps what the windowing layer sees (name, origin, size, scale),
  which is not what Displays shows and is what made the above diagnosable.

## 0.8.1
- Fixed a crash on Cmd+1. A RefCell borrow was held across calls into AppKit and
  WebKit, which pump the run loop and re-enter the event handler, so the second
  borrow aborted the process. Borrows reachable from a re-entrant call now yield
  instead, and WebViews are built and destroyed outside any borrow.
- Dropped `panic = "abort"`, which turned a diagnosable panic into a bare SIGABRT.

## 0.8.3
- **Fixed the Cmd+1 crash properly.** muda stores a raw pointer to each menu
  item's `MenuChild` inside the NSMenuItem and dereferences it on every
  activation. `install_menu` dropped its `Menu`, `Submenu` and `MenuItem` handles
  on return, leaving that pointer dangling, so the first activation read a String
  out of freed memory. The menu is now kept alive for the process lifetime.
  `--fire-menu` performs every custom item through AppKit and is a real regression
  test: it aborts without the fix and exits clean with it.
- 0.8.1's borrow-guard work was aimed at the wrong cause. It is kept because
  holding a borrow across an AppKit call is a genuine hazard, but it fixed nothing.

## 0.8.2
- Gmail's own iframes (apps launcher, feedback widget, Calendar side panel) no
  longer open as Chrome tabs on every load. Google-owned hosts stay in the pane;
  real link clicks, which arrive as target=_blank, still all leave.

## 0.8.0
- **Links never open in a pane.** Outbound links, and Docs/Sheets opened from
  Drive, go to your default browser. Google's own login plumbing and the
  signed-out bounce deliberately stay in the pane; tracking beacons are dropped
  silently rather than becoming browser tabs.
- Keyboard shortcuts: Cmd+1..9 for accounts, Cmd+Shift+1/2/3 for Mail, Calendar
  and Drive, Cmd+R to reload, Cmd+, for settings. All in a Go and View menu.
- Window size, position, and which account and app you were on are remembered.
- Downloads land in ~/Downloads and reveal in Finder.

## 0.7.0
- Renamed to Masse throughout. Session store migrated, so accounts carry over.
- Settings modal behind the gear: logo, version, remove an account, memory dials.
- Rebuilds now quit the app cleanly instead of killing it, which was discarding
  the cookie jar and forcing a fresh login every time.

## 0.6.0
- Logins survive a restart. Switched to `WKWebsiteDataStore(forIdentifier:)`; the
  default store was keeping cookies in memory only, so every launch demanded
  two-factor again.

## 0.5.0
- Add accounts with the `+` in the rail, which runs Google's own add-account flow
  and detects whichever account signs in. No typing addresses, no editing JSON.
- A real cog for settings. The previous icon was a ring with rays and read as a
  brightness control.

## 0.4.1
- Settings button enlarged to 44px and pinned to the bottom of the rail.

## 0.4.0
- Accounts on the left, apps across the top, as originally intended. Previously
  three tiny icons were crammed under each avatar.

## 0.3.0
- First real version: pane pool, LRU eviction, idle timeout, avatar rail,
  `?authuser=` addressing, config file, macOS menu, app bundle.
