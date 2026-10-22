# Changelog

## 1.0.9
- **Downloading from Drive no longer blanks the pane.** Cross-origin downloads
  were handed to WebKit by loading them into the pane you were looking at, which
  saved the file but replaced Drive with an empty page. They now run in a hidden
  webview of their own that shares the same session, and is discarded once the
  file has been saved.
- Drive's cookie warm-up (`drive.usercontent.google.com/auth_warmup`), which it
  loads in a hidden frame before every download, is no longer mistaken for a
  download. Matching that whole host pulled the warm-up out of the page, so Drive
  never started the real download.
- **Finished downloads are revealed in Finder again, and no longer logged as
  failures.** wry's success callback never passes the saved file's path, and
  Masse only counted a download as successful when it had one, so every
  download that worked, Gmail included, was reported as failed and never shown.
  Masse now remembers the path from when the download started.

## 1.0.6
- **Adding an account works for accounts without a profile photo.** Since 1.0.2 a
  new account was only saved once its photo had been read, so an account with no
  photo was seen by the add flow and then dropped. The address alone now saves it.
- **The add screen no longer hijacks an avatar.** After pressing +, the add screen
  became the "current app", and clicking an avatar keeps the current app, so it
  reopened the add screen under that account showing whichever account Google had
  last landed on. Once an account is added, Masse switches to its Mail and closes
  the add screen, and the add screen is never carried over as an app.
- **If Google lands on an account you already have,** Masse shows Google's account
  chooser once so the new account can be picked.
- Google's sign-in pages no longer report an account. They show the account you
  are already signed into, which used to be mistaken for the one being added.

## 1.0.5
- **Drive's Download button works.** Masse handles some downloads by cancelling
  the navigation and fetching the file inside the page, because WebKit renders a
  displayable file instead of saving it. That only works same-origin. A Gmail
  attachment is on `mail.google.com`, the same origin as the pane, so the fetch is
  allowed. A Drive file is on `drive.usercontent.google.com` while the pane is
  `drive.google.com`, and that host sends no CORS headers, so the fetch was
  blocked and the download was simply lost: click Download, nothing happens.
  Masse now only takes a download over when the page could actually fetch it, and
  leaves every cross-origin one to WebKit.

## 1.0.4
- **A pane that goes blank now comes back.** When WebKit's content process dies,
  the pane it was drawing turns white and stays white: nothing reloads it, so the
  app looks broken until it is restarted. Masse now notices the process dying and
  reloads that pane where it should have been.
- **A large download no longer takes the pane down with it.** A cancelled download
  navigation is fetched inside the page and handed to the native side as base64,
  which is a third larger again, inside a JSON string. That is fine for a mail
  attachment and fatal for a big Drive file: the content process runs out of
  memory and the pane goes blank, which is exactly what a blank Drive looked like.
  Anything over 25 MB is now left to WebKit's own download machinery instead.

## 1.0.3
- **An account that is missing from Google's session can now actually be added.**
  1.0.1 correctly noticed that a pane was showing the wrong account and sent it to
  `ServiceLogin`, which was the wrong door: ServiceLogin means "sign in", and
  Google treats it as already satisfied when ANY account is signed in, so it
  forwarded straight back to the wrong one. The recovery now uses `AddSession`,
  which means "sign in to an additional account". That is the real situation:
  `?authuser=` can only pick an account the session already has, and no URL can
  conjure one that was never signed in.
- **Clicking an account retries.** A failed recovery used to stick for the rest of
  the session, so the pane silently kept showing the wrong account until a
  restart. Clicking it in the rail now clears that and tries again.

## 1.0.2
- **The blue ring in the rail was Google's loading spinner.** An account's photo is
  read off the page, and the code took the first image inside the account button.
  Google puts a blue loading ring there from `ssl.gstatic.com` while the real photo
  loads, so that spinner got saved as the avatar and drawn in the rail from then
  on. Only a photo served from googleusercontent.com is accepted now, on both
  sides, and anything already stored that is not one is discarded at launch, so an
  affected account heals itself.
- **Verifying which account a pane is showing no longer waits on a photo.** The
  address and the photo are reported separately. Previously both travelled in the
  same message, so an account with no photo was never checked at all, which
  undercut the 1.0.1 fix.
- **A dead photo URL falls back to initials.** Google's photo URLs expire. The
  image simply failed before, leaving a circle of the account colour with nothing
  in it.

## 1.0.1
- **A pane can no longer show the wrong account.** Masse asks Google for an
  account with `?authuser=<address>`, but only ever checked which *host* came
  back. When Google signs one account out it silently serves whichever account is
  still signed in, on the same host, so a pane kept its label and its colour while
  showing somebody else's mail. The page already reports which account it belongs
  to; that is now compared against the account the pane was asked for, and a
  mismatch sends the pane to the login page for the account it claims to be. Once
  per pane, so a login that keeps failing cannot loop.
- **An unknown account is no longer added just because a page mentioned it.**
  Discovery is now limited to the add-account flow, which is the only place an
  unfamiliar address is expected. Previously a mix-up could quietly write the
  wrong account into the config.
- **The navigation cannot disappear.** The rail and the top bar were emptied and
  then rebuilt, so anything that threw part way through left them blank for good,
  which looks exactly like the navigation vanishing. Both now build into a
  fragment and swap it in at the end: a failed render keeps what was already
  there. The specific fault found by reproducing it: an account with no colour
  reached the contrast helper, which called `.slice()` on `undefined` and threw.
  That helper is now guarded, and errors in either chrome webview are reported
  instead of being swallowed by a console nobody reads.

## 1.0.0
- **Masse is signed and notarized, so it just opens.** Every release until now was
  ad-hoc signed, which meant macOS refused to launch it and every download came
  with instructions for talking your way past Gatekeeper. The app is now signed
  with a Developer ID certificate under the hardened runtime, submitted to Apple
  for notarization, and the resulting ticket is stapled into the bundle, so it
  opens on a machine that has never seen it and with no network. Verified by
  extracting the shipped zip to a clean location and asking Gatekeeper directly:
  `accepted, source=Notarized Developer ID`.
- `bundle.sh` does the signing and notarizing on release builds only, driven by
  `MASSE_SIGN_ID` and `MASSE_NOTARY_PROFILE`. Without them a build is ad-hoc and
  instant, which is what you want while iterating. It stapled the ticket before
  building the distributable zip, because the zip submitted to Apple does not
  contain the ticket, and it fails the build outright if Gatekeeper still rejects
  the result.
- The site no longer explains how to get past the warning, because there is no
  longer a warning.

## 0.16.0
- **Link rewriting is now a setting, on by default.** Settings has a Links section
  with one switch: "Fix Google account links". Turn it off and every link is handed
  to the browser exactly as the page wrote it. The switch takes effect on the next
  click, not the next launch, and is stored as `rewrite_links` in `accounts.json`.
  An existing config without the field keeps rewriting on, so upgrading changes
  nothing.
- **Links to other sites no longer go through Google.** Gmail and Chat do not store
  the link you were given; they store `www.google.com/url?...&url=<the real one>`.
  Opening that means a click on a link to somebody else's site travels through
  Google first. The destination is right there in the query, so Masse now unwraps it
  and opens the real address directly. Rejects anything that is not http or https,
  so a crafted `url=javascript:...` cannot be handed to `open`.
- Rewrote the settings copy. It described the implementation rather than saying what
  the switch does, and one line ("Off hands every link over exactly as written") was
  simply unreadable.

## 0.15.3
- **Only meeting links get addressed to an account.** 0.15.2 rewrote every
  Google-owned link, and Gmail and Chat wrap *every* outbound link in a
  `www.google.com/url?...` redirector. That host is Google's, so a link to Loom
  was being sent through Google's account chooser on its way out: pointless, and
  it handed the destination to a sign-in flow that had no business seeing it. The
  rule is now the one host where the answer actually matters, `meet.google.com`.
  Docs, Drive, Gmail and search links open wherever the browser opens them.

## 0.15.2
- **Meeting links really do open as the right account now.** 0.15.0 claimed this
  and was wrong, twice over, so here is what actually goes wrong and what fixes
  it. Google puts `authuser=1` on its own Calendar links. That is a session
  *index*, assigned per session, so the browser resolves `1` against its own
  ordering of accounts and opens whichever one happens to sit there: the exact
  symptom. Replacing the index with the email address is necessary but still not
  enough, because Meet ignores the address form of `authuser`. What works is
  Google's account chooser, which resolves the address server-side and redirects
  with the session already correct, so outbound Google links are now wrapped in
  it. Verified by clicking a real Meet invite, not by reasoning about it.
- Non-Google links (Zoom, Teams) are untouched, and a chooser URL is never
  wrapped in a second chooser.
- Still true, and not fixable from here: this only chooses between accounts the
  browser is already signed into. It cannot create a session, so an account the
  browser has never seen lands on a sign-in page.

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
