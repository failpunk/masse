//! The two chrome webviews. Accounts run down the left, apps across the top:
//! account and app are independent choices, so they get independent controls.
//! Both receive the same `window.shim.render(state)` payload.

/// The two chrome webviews, so callers cannot update one and forget the other.
pub struct Chrome {
    pub rail: wry::WebView,
    pub topbar: wry::WebView,
}

impl Chrome {
    pub fn push(&self, state: &str) {
        let script = format!("window.shim.render({state})");
        let _ = self.rail.evaluate_script(&script);
        let _ = self.topbar.evaluate_script(&script);
    }
}

pub const RAIL_W: f64 = 72.0;

pub const TOPBAR_H: f64 = 46.0;

const SHARED_CSS: &str = r#"
  :root { color-scheme: dark; }
  * { box-sizing: border-box; }
  body {
    margin: 0; background: #11131a; color: #fff; overflow: hidden;
    font: 500 12px/1.2 -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
    -webkit-user-select: none; cursor: default;
  }
  button { all: unset; cursor: pointer; }
"#;

const SHARED_JS: &str = r#"
  // 24px viewBox so the geometry has room to be correct, then scaled down. The
  // previous set was drawn at 20px and read as mush: a chevron for mail, a bare
  // triangle for Drive.
  const ICONS = {
    mail: '<rect x="2.5" y="5" width="19" height="14" rx="2.6"/>'
        + '<path d="M3.6 7.4l7.5 5.2a1.9 1.9 0 0 0 1.8 0l7.5-5.2"/>',
    calendar: '<rect x="3" y="5.5" width="18" height="15" rx="2.6"/>'
        + '<path d="M3 10.4h18"/><path d="M8 3.4v4M16 3.4v4"/>'
        + '<circle cx="12" cy="15" r="1.5" fill="currentColor" stroke="none"/>',
    drive: '<path d="M3 8a2 2 0 0 1 2-2h3.7a2 2 0 0 1 1.5.7l1.3 1.4H19a2 2 0 0 1 2 2v7'
        + 'a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  };
  const LABELS = { mail: 'Mail', calendar: 'Calendar', drive: 'Drive' };
  const send = (m) => window.ipc.postMessage(JSON.stringify(m));
  // A highlight can be yellow or indigo, so the glyph sitting on it cannot be a
  // fixed colour. Relative luminance decides light or dark ink.
  const readable = (hex) => {
    const chan = (i) => {
      const c = parseInt(hex.slice(1 + i * 2, 3 + i * 2), 16) / 255;
      return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * chan(0) + 0.7152 * chan(1) + 0.0722 * chan(2) > 0.36
      ? '#11131a' : '#f1f2f4';
  };
  const TICK = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" '
    + 'stroke-width="3.2" stroke-linecap="round" stroke-linejoin="round">'
    + '<path d="M5 12.8l4.6 4.4L19 6.6"/></svg>';
  const svg = (d) => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor"
      stroke-width="1.7" stroke-linejoin="round" stroke-linecap="round">${d}</svg>`;
"#;

pub fn rail_html(state: &str) -> String {
    format!(
        r#"<!doctype html>
<meta charset="utf-8">
<style>
  {SHARED_CSS}
  body {{
    height: 100vh; padding: 14px 0 12px; display: flex; flex-direction: column;
    align-items: center; gap: 14px;
  }}
  .ava {{
    width: 46px; height: 46px; border-radius: 50%; overflow: hidden; position: relative;
    display: grid; place-items: center; color: #fff; font: 600 15px/1 system-ui;
    box-shadow: 0 0 0 2px transparent;
    /* Inactive accounts are held back so the active one reads instantly. */
    opacity: .45; filter: saturate(.65);
    transition: box-shadow .14s ease, opacity .14s ease, filter .14s ease, transform .14s ease;
  }}
  .ava img {{ width: 100%; height: 100%; object-fit: cover; display: block; }}
  .ava:hover {{ opacity: .8; filter: none; box-shadow: 0 0 0 2px rgba(255,255,255,.45); }}
  .ava.on {{
    opacity: 1; filter: none;
    box-shadow: 0 0 0 3px #11131a, 0 0 0 5px var(--hl, #fff);
    transform: scale(1.04);
  }}
  /* A bar on the window edge marks the current account even at a glance. */
  .slot {{ position: relative; display: grid; place-items: center; width: 100%; height: 46px; }}
  .slot {{ border-radius: 0 12px 12px 0; }}
  .slot.on {{ background: rgba(255,255,255,.08); }}
  .slot.on::before {{
    content: ''; position: absolute; left: 0; top: 3px; bottom: 3px; width: 5px;
    border-radius: 0 4px 4px 0; background: var(--hl, #fff);
  }}
  /* Stacked mode: the rail is the whole navigation, so it needs room to breathe
     and to scroll once there are several accounts. */
  body.stacked {{ padding-top: 12px; gap: 6px; overflow-y: auto; }}
  /* .slot is a fixed 46px tall in split mode, where it holds only the avatar. In
     stacked mode it also holds a column of apps, so it must grow: with a fixed
     height the apps overflowed and landed on top of the next account. */
  body.stacked .slot {{
    height: auto; display: flex; flex-direction: column; align-items: center;
    padding: 9px 0 13px; border-radius: 0 14px 14px 0;
  }}
  body.stacked .slot.on {{ background: rgba(255,255,255,.07); }}
  /* The account circle gives up a little size so the three apps can sit under it
     at a legible size instead of being crammed against it. */
  body.stacked .ava {{ width: 38px; height: 38px; font-size: 13px; }}
  body.stacked .slot.on::before {{ top: 6px; bottom: 6px; }}
  /* One column, not a row: three icons side by side in a 60-odd pixel rail sat on
     top of each other. Each app is its own small circle under the account. */
  .apps {{
    display: flex; flex-direction: column; align-items: center;
    gap: 5px; margin-top: 9px;
  }}
  .app {{
    width: 25px; height: 25px; border-radius: 50%; display: grid; place-items: center;
    color: rgba(255,255,255,.45); background: rgba(255,255,255,.06);
    transition: background .12s ease, color .12s ease;
  }}
  .app svg {{ width: 14px; height: 14px; }}
  .app:hover {{ background: rgba(255,255,255,.17); color: #fff; }}
  .app.on {{ background: var(--hl, #fff); color: var(--hl-ink, #11131a); }}

  .gear {{
    margin-top: auto; width: 44px; height: 44px; border-radius: 13px; flex: none;
    display: grid; place-items: center; color: rgba(255,255,255,.55);
    background: rgba(255,255,255,.06);
    transition: background .12s ease, color .12s ease;
  }}
  .gear svg {{ width: 22px; height: 22px; }}
  .add {{
    width: 44px; height: 44px; border-radius: 50%; flex: none;
    display: grid; place-items: center; color: rgba(255,255,255,.5);
    border: 1.5px dashed rgba(255,255,255,.28);
    transition: border-color .12s ease, color .12s ease, background .12s ease;
  }}
  .add svg {{ width: 22px; height: 22px; }}
  .add:hover {{
    border-color: rgba(255,255,255,.6); color: #fff; background: rgba(255,255,255,.08);
  }}
  .gear:hover {{ background: rgba(255,255,255,.16); color: #fff; }}
  .gear:active {{ background: rgba(255,255,255,.24); }}
</style>
<div id="rail" style="display:flex;flex-direction:column;align-items:center;gap:14px;width:100%"></div>
<button class="add" id="add">
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
    <path d="M12 6v12M6 12h12"/>
  </svg>
</button>
<button class="gear" id="gear">
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"
       stroke-linecap="round" stroke-linejoin="round">
    <!-- A toothed cog. The previous version was a ring with long thin rays,
         which reads as a brightness control, not a gear. -->
    <circle cx="12" cy="12" r="3.2"/>
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
  </svg>
</button>
<script>
  {SHARED_JS}
  window.shim = {{
    render(state) {{
      const stacked = state.nav === 'stacked';
      document.body.classList.toggle('stacked', stacked);
      const rail = document.getElementById('rail');
      rail.textContent = '';

      for (const a of state.accounts) {{
        const here = a.email.toLowerCase() === (state.active.email || '').toLowerCase();
        const slot = document.createElement('div');
        slot.className = 'slot' + (here ? ' on' : '');
        // Every highlight in this slot derives from the account's own colour, so the
        // active account is identifiable without reading the avatar.
        slot.style.setProperty('--hl', a.color);
        slot.style.setProperty('--hl-ink', readable(a.color));

        const ava = document.createElement('button');
        ava.className = 'ava' + (here ? ' on' : '');
        ava.style.background = a.color;
        if (a.avatar) {{
          const img = new Image();
          img.src = a.avatar;
          img.referrerPolicy = 'no-referrer';
          ava.appendChild(img);
        }} else ava.textContent = a.initials;
        // Switching account keeps whichever app you are already looking at.
        ava.onclick = () => send({{ type: 'show', email: a.email, service: state.active.service }});
        slot.appendChild(ava);

        // Stacked mode puts every app under its own account, so any of the nine
        // destinations is one click away without changing account first.
        if (stacked) {{
          const apps = document.createElement('div');
          apps.className = 'apps';
          for (const svc of state.services) {{
            const b = document.createElement('button');
            b.className = 'app' + (here && svc === state.active.service ? ' on' : '');
            b.innerHTML = svg(ICONS[svc]);
            b.onclick = () => send({{ type: 'show', email: a.email, service: svc }});
            apps.appendChild(b);
          }}
          slot.appendChild(apps);
        }}
        rail.appendChild(slot);
      }}
    }},
  }};
  document.getElementById('gear').onclick = () => send({{ type: 'settings' }});
  document.getElementById('add').onclick = () => send({{ type: 'add' }});
  window.shim.render({state});
</script>"#
    )
}

pub fn topbar_html(state: &str) -> String {
    format!(
        r#"<!doctype html>
<meta charset="utf-8">
<style>
  {SHARED_CSS}
  body {{
    height: 100vh; display: flex; align-items: center; gap: 4px; padding: 0 14px;
    border-bottom: 1px solid rgba(255,255,255,.09);
  }}
  .tab {{
    display: flex; align-items: center; gap: 7px; height: 30px; padding: 0 13px;
    border-radius: 9px; color: rgba(255,255,255,.6); font-weight: 500;
    transition: background .12s ease, color .12s ease;
  }}
  .tab svg {{ width: 15px; height: 15px; }}
  .tab:hover {{ background: rgba(255,255,255,.09); color: #fff; }}
  .tab.on {{ background: #fff; color: #11131a; font-weight: 600; }}
  .who {{
    margin-left: auto; display: flex; align-items: center; gap: 8px;
    color: rgba(255,255,255,.42); font-size: 11.5px; white-space: nowrap;
  }}
  .dot {{ width: 8px; height: 8px; border-radius: 50%; }}
</style>
<div id="tabs" style="display:flex;gap:4px"></div>
<div class="who" id="who"></div>
<script>
  {SHARED_JS}
  window.shim = {{
    render(state) {{
      const tabs = document.getElementById('tabs');
      tabs.textContent = '';
      for (const s of state.services) {{
        const b = document.createElement('button');
        b.className = 'tab' + (s === state.active.service ? ' on' : '');
        b.innerHTML = svg(ICONS[s]) + `<span>${{LABELS[s]}}</span>`;
        b.onclick = () => send({{ type: 'show', email: state.active.email, service: s }});
        tabs.appendChild(b);
      }}
      // Which account you are in, so the apps across the top are never ambiguous.
      const who = document.getElementById('who');
      who.textContent = '';
      const current = state.accounts.find(
        (a) => a.email.toLowerCase() === (state.active.email || '').toLowerCase()
      );
      if (current) {{
        const dot = document.createElement('span');
        dot.className = 'dot';
        dot.style.background = current.color;
        who.appendChild(dot);
        who.appendChild(document.createTextNode(current.email));
      }}
    }},
  }};
  window.shim.render({state});
</script>"#
    )
}

/// The settings modal. Built fresh each time it opens, which is deliberate: wry
/// child webviews stack in creation order, so creating it on demand is what puts
/// it above the panes. It is destroyed on close, costing nothing when shut.
pub fn settings_html(state: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        r##"<!doctype html>
<meta charset="utf-8">
<style>
  {SHARED_CSS}
  body {{
    height: 100vh; display: grid; place-items: center; padding: 28px 28px 80px;
    background: rgba(8, 9, 13, .78); backdrop-filter: blur(14px);
  }}
  .card {{
    width: 100%; max-width: 520px; max-height: 100%; overflow: auto;
    background: #16181f; border-radius: 18px; padding: 24px 26px 20px;
    box-shadow: 0 24px 70px -20px rgba(0,0,0,.75), 0 0 0 1px rgba(255,255,255,.08);
  }}
  header {{ display: flex; align-items: center; gap: 12px; margin-bottom: 4px; }}
  .mark {{ width: 34px; height: 34px; flex: none; }}
  h1 {{ font-size: 19px; margin: 0; letter-spacing: -.2px; font-weight: 650; }}
  .ver {{
    margin-left: auto; font: 500 11.5px/1 ui-monospace, SFMono-Regular, monospace;
    color: rgba(255,255,255,.45); background: rgba(255,255,255,.07);
    padding: 5px 8px; border-radius: 6px;
  }}
  .sub {{ color: rgba(255,255,255,.45); font-size: 12.5px; margin: 0 0 22px 46px; }}
  h2 {{
    font-size: 10.5px; text-transform: uppercase; letter-spacing: .09em;
    color: rgba(255,255,255,.38); margin: 20px 0 9px;
  }}
  .row {{
    display: flex; align-items: center; gap: 11px; padding: 8px 10px;
    border-radius: 11px; background: rgba(255,255,255,.04); margin-bottom: 5px;
  }}
  .ava {{
    width: 30px; height: 30px; border-radius: 50%; overflow: hidden; flex: none;
    display: grid; place-items: center; font: 600 11px/1 system-ui; color: #fff;
  }}
  .ava img {{ width: 100%; height: 100%; object-fit: cover; }}
  .mail {{ flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
  /* The row shows only the current colour. The ten suggestions live in a popover,
     so the modal is not a wall of swatches when several accounts are configured. */
  .chipwrap {{ position: relative; flex: none; display: flex; }}
  .current {{
    width: 22px; height: 22px; border-radius: 50%; cursor: pointer;
    box-shadow: inset 0 0 0 1px rgba(0,0,0,.3), 0 0 0 1px rgba(255,255,255,.14);
    transition: transform .12s cubic-bezier(.22,1,.36,1);
  }}
  .current:hover {{ transform: scale(1.12); }}
  .current.open {{ box-shadow: inset 0 0 0 1px rgba(0,0,0,.3), 0 0 0 2px #fff; }}
  .pop {{
    position: absolute; right: 0; top: calc(100% + 7px); z-index: 5;
    display: flex; gap: 6px; padding: 9px 10px; border-radius: 12px;
    background: #22262e;
    box-shadow: 0 12px 32px -8px rgba(0,0,0,.75), 0 0 0 1px rgba(255,255,255,.14);
  }}
  .pop.up {{ top: auto; bottom: calc(100% + 7px); }}
  .chip {{
    width: 20px; height: 20px; border-radius: 50%; cursor: pointer; flex: none;
    display: grid; place-items: center;
    box-shadow: inset 0 0 0 1px rgba(0,0,0,.28);
    transition: transform .12s cubic-bezier(.22,1,.36,1);
  }}
  .chip:hover {{ transform: scale(1.16); }}
  .chip svg {{ width: 12px; height: 12px; opacity: 0; }}
  .chip.on svg {{ opacity: 1; }}
  .kill {{
    font-size: 11.5px; padding: 5px 10px; border-radius: 7px;
    color: rgba(255,255,255,.5); background: rgba(255,255,255,.07);
  }}
  .kill:hover {{ background: rgba(255,90,90,.22); color: #ffb4b4; }}
  .kill.arm {{ background: #e5484d; color: #fff; }}
  .dial {{
    display: flex; align-items: center; gap: 11px; padding: 9px 10px;
    border-radius: 11px; background: rgba(255,255,255,.04); margin-bottom: 5px;
  }}
  .dial > label:not(.switch) {{ flex: 1; font-size: 12.5px; }}
  /* A switch rather than two buttons: with a segmented control it was not obvious
     which side was active. */
  .switch {{ flex: none; margin-left: auto; cursor: pointer; display: inline-flex; }}
  .switch input {{ position: absolute; opacity: 0; pointer-events: none; }}
  .switch span {{
    width: 44px; height: 26px; border-radius: 999px; position: relative;
    background: rgba(255,255,255,.13); box-shadow: inset 0 0 0 1px rgba(255,255,255,.10);
    transition: background .16s ease;
  }}
  .switch span::after {{
    content: ''; position: absolute; top: 3px; left: 3px; width: 20px; height: 20px;
    border-radius: 50%; background: #fff;
    transition: transform .16s cubic-bezier(.22,1,.36,1);
  }}
  .switch input:checked + span {{ background: #2b5bff; }}
  .switch input:checked + span::after {{ transform: translateX(18px); }}
  .switch input:focus-visible + span {{ box-shadow: 0 0 0 2px rgba(255,255,255,.5); }}
  .dial small {{ display: block; color: rgba(255,255,255,.38); font-size: 11px; margin-top: 2px; }}
  input[type="number"], input[type="text"] {{
    margin-left: auto; flex: none;
  }}
  input {{
    width: 56px; font: inherit; text-align: center; padding: 5px; border-radius: 7px;
    border: 1px solid rgba(255,255,255,.14); background: rgba(0,0,0,.3); color: #fff;
  }}
  .withunit {{
    display: flex; align-items: center; gap: 7px; white-space: nowrap;
    color: rgba(255,255,255,.55); font-size: 12.5px;
  }}
  footer {{ display: flex; align-items: center; gap: 10px; margin-top: 20px; }}
  /* A floating pill, centred over the bottom of the overlay. Detached from the
     edges so it reads as an object sitting on top rather than a docked bar. */
  .promo {{
    position: fixed; left: 50%; bottom: 22px; transform: translateX(-50%);
    display: inline-flex; align-items: center; gap: 8px; white-space: nowrap;
    padding: 11px 20px; border-radius: 999px; position: fixed;
    /* Sheen: a cool tint under a top-down highlight, so it catches light along its
       upper edge the way a physical pill would. The inset white line is the
       highlight; the inset dark line at the bottom is the shaded underside. */
    background:
      linear-gradient(180deg, rgba(120,150,255,.20), rgba(70,90,190,.10) 46%, rgba(24,27,34,.72)),
      rgba(30,34,42,.92);
    box-shadow:
      0 8px 28px -6px rgba(0,0,0,.6),
      0 2px 22px -4px rgba(43,91,255,.42),
      inset 0 1px 0 rgba(255,255,255,.26),
      inset 0 -1px 0 rgba(0,0,0,.34),
      0 0 0 1px rgba(140,165,255,.24);
    backdrop-filter: blur(10px);
    font-size: 12.5px; color: rgba(255,255,255,.72); cursor: pointer;
    transition: transform .16s cubic-bezier(.22,1,.36,1), background .12s ease,
                color .12s ease, box-shadow .16s ease;
  }}
  .promo:hover {{
    color: #fff;
    background:
      linear-gradient(180deg, rgba(140,170,255,.30), rgba(80,105,215,.14) 46%, rgba(28,32,40,.74)),
      rgba(34,39,48,.94);
    transform: translateX(-50%) translateY(-2px);
    box-shadow:
      0 12px 34px -6px rgba(0,0,0,.68),
      0 3px 30px -4px rgba(43,91,255,.60),
      inset 0 1px 0 rgba(255,255,255,.34),
      inset 0 -1px 0 rgba(0,0,0,.34),
      0 0 0 1px rgba(160,185,255,.38);
  }}
  .promo:active {{ transform: translateX(-50%) translateY(0); }}
  .promo svg {{ width: 13px; height: 13px; opacity: .75; }}
  .link {{ font-size: 11.5px; color: rgba(255,255,255,.35); text-decoration: underline; }}
  .link:hover {{ color: #fff; }}
  .done {{
    margin-left: auto; padding: 8px 18px; border-radius: 9px;
    background: #fff; color: #11131a; font-weight: 600; font-size: 13px;
  }}
</style>
<button class="promo" id="promo">
  Learn more about our AI research
  <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.8"
       stroke-linecap="round" stroke-linejoin="round">
    <path d="M7.5 4.5h8v8"/><path d="M15.5 4.5L5 15"/>
  </svg>
</button>
<div class="card">
  <header>
    <svg class="mark" viewBox="0 0 48 48" aria-hidden="true"><rect width="48" height="48" rx="11" fill="#0b0d0f"/><path d="M24.0 24.0 L24.98 10.03 A14.0 14.0 0 0 1 36.58 30.14 Z" fill="#6366f1"/><path d="M24.0 24.0 L35.61 31.83 A14.0 14.0 0 0 1 12.39 31.83 Z" fill="#ec4899"/><path d="M24.0 24.0 L11.42 30.14 A14.0 14.0 0 0 1 23.02 10.03 Z" fill="#f59e0b"/></svg>
    <h1>Masse</h1>
    <span class="ver">v{version}</span>
  </header>
  <p class="sub">Several Google accounts, one window.</p>

  <h2>Accounts</h2>
  <div id="accounts"></div>

  <h2>Navigation</h2>
  <div class="dial">
    <label for="navtoggle">Put every app in the left rail
      <small>Show each account's Mail, Calendar and Drive in the rail, so you can go
      straight to any of them. The rail gets more crowded.</small></label>
    <label class="switch"><input type="checkbox" id="navtoggle" /><span></span></label>
  </div>

  <h2>Links</h2>
  <div class="dial">
    <label for="linktoggle">Fix Google account links
      <small>Let Masse rewrite Google meeting links so they open in the appropriate
      account. Very useful when you're signed into more than one Google account.</small></label>
    <label class="switch"><input type="checkbox" id="linktoggle" /><span></span></label>
  </div>

  <h2>Memory</h2>
  <div class="dial">
    <label>Pages kept open
      <small>Fewer pages use less memory. Closed pages reload when you go back to
      them.</small></label>
    <input id="maxLive" type="number" min="1" max="9">
  </div>
  <div class="dial">
    <label>Close a page you have not used for
      <small>Set this to 0 to keep every page open.</small></label>
    <span class="withunit"><input id="idle" type="number" min="0" max="600"> minutes</span>
  </div>

  <footer>
    <button class="link" id="json">Edit accounts.json</button>
    <button class="done" id="close">Close</button>
  </footer>
</div>
<script>
  {SHARED_JS}
  let armed = null;
  let picking = null;

  let LAST = null;
  window.shim = {{
    render(state) {{
      LAST = state;
      const list = document.getElementById('accounts');
      list.textContent = '';
      for (const a of state.accounts) {{
        const row = document.createElement('div');
        row.className = 'row';

        const ava = document.createElement('span');
        ava.className = 'ava';
        ava.style.background = a.color;
        if (a.avatar) {{
          const img = new Image();
          img.src = a.avatar;
          img.referrerPolicy = 'no-referrer';
          ava.appendChild(img);
        }} else ava.textContent = a.initials;
        row.appendChild(ava);

        const mail = document.createElement('span');
        mail.className = 'mail';
        mail.textContent = a.email;
        row.appendChild(mail);

        // Current colour only; the ten suggestions appear when this is clicked.
        const wrap = document.createElement('div');
        wrap.className = 'chipwrap';
        const current = document.createElement('button');
        current.className = 'current' + (picking === a.email ? ' open' : '');
        current.style.background = a.color;
        current.onclick = (e) => {{
          e.stopPropagation();
          picking = picking === a.email ? null : a.email;
          window.shim.render(state);
        }};
        wrap.appendChild(current);

        if (picking === a.email) {{
          const pop = document.createElement('div');
          pop.className = 'pop';
          for (const c of state.palette) {{
            const chip = document.createElement('button');
            chip.className = 'chip' + (c.toLowerCase() === (a.color || '').toLowerCase() ? ' on' : '');
            chip.style.background = c;
            chip.style.color = readable(c);
            chip.innerHTML = TICK;
            chip.onclick = (e) => {{
              e.stopPropagation();
              picking = null;
              send({{ type: 'color', email: a.email, color: c }});
            }};
            pop.appendChild(chip);
          }}
          wrap.appendChild(pop);
          // Flip above when there is no room below, since the card scrolls and would
          // otherwise clip it for the last account.
          requestAnimationFrame(() => {{
            const box = pop.getBoundingClientRect();
            if (box.bottom > window.innerHeight - 8) pop.classList.add('up');
          }});
        }}
        row.appendChild(wrap);

        const kill = document.createElement('button');
        kill.className = 'kill' + (armed === a.email ? ' arm' : '');
        kill.textContent = armed === a.email ? 'Really remove?' : 'Remove';
        // Two-step, because a stray click should not silently drop an account.
        kill.onclick = () => {{
          if (armed === a.email) {{
            armed = null;
            send({{ type: 'remove', email: a.email }});
          }} else {{
            armed = a.email;
            window.shim.render(state);
          }}
        }};
        row.appendChild(kill);
        list.appendChild(row);
      }}
      const navToggle = document.getElementById('navtoggle');
      navToggle.checked = state.nav === 'stacked';
      navToggle.onchange = () =>
        send({{ type: 'nav', nav: navToggle.checked ? 'stacked' : 'split' }});
      const linkToggle = document.getElementById('linktoggle');
      linkToggle.checked = state.rewrite_links !== false;
      linkToggle.onchange = () =>
        send({{ type: 'rewriteLinks', on: linkToggle.checked }});
      document.getElementById('maxLive').value = state.max_live;
      document.getElementById('idle').value = state.idle_minutes;
    }},
  }};

  const push = () => send({{
    type: 'dials',
    max_live: Number(document.getElementById('maxLive').value) || 1,
    idle_minutes: Number(document.getElementById('idle').value) || 0,
  }});
  document.getElementById('maxLive').onchange = push;
  document.getElementById('idle').onchange = push;
  document.getElementById('json').onclick = () => send({{ type: 'config' }});
  document.getElementById('close').onclick = () => send({{ type: 'close' }});
  // Routed through the host so it lands in the real browser, not in a pane.
  document.getElementById('promo').onclick = () =>
    send({{ type: 'link', url: 'https://ae.studio/alignment' }});
  document.addEventListener('click', () => {{
    if (picking !== null) {{ picking = null; window.shim.render(LAST); }}
  }});
  document.addEventListener('keydown', (e) => {{
    if (e.key !== 'Escape') return;
    // Escape closes the popover first, and only then the modal.
    if (picking !== null) {{ picking = null; window.shim.render(LAST); return; }}
    send({{ type: 'close' }});
  }});
  window.shim.render({state});
</script>"##
    )
}
