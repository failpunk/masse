# Changelog

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
