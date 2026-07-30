# Changelog

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
