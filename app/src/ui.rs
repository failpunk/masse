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
  const ICONS = {
    mail: '<rect x="2.5" y="4.5" width="15" height="11" rx="1.5"/><path d="M3 5.5l7 5 7-5"/>',
    calendar: '<rect x="3" y="5" width="14" height="12" rx="1.5"/><path d="M3 8.5h14M7 3.5v3M13 3.5v3"/>',
    drive: '<path d="M10 3.5L17 16H3z"/><path d="M6.5 10.5h7"/>',
  };
  const LABELS = { mail: 'Mail', calendar: 'Calendar', drive: 'Drive' };
  const send = (m) => window.ipc.postMessage(JSON.stringify(m));
  const svg = (d) => `<svg viewBox="0 0 20 20" fill="none" stroke="currentColor"
      stroke-width="1.7" stroke-linejoin="round" stroke-linecap="round">${d}</svg>`;
"#;

pub fn rail_html(state: &str) -> String {
    format!(
        r#"<!doctype html>
<meta charset="utf-8">
<style>
  {SHARED_CSS}
  body {{
    height: 100vh; padding: 10px 0 12px; display: flex; flex-direction: column;
    align-items: center; gap: 8px;
  }}
  .ava {{
    width: 46px; height: 46px; border-radius: 50%; overflow: hidden; position: relative;
    display: grid; place-items: center; color: #fff; font: 600 15px/1 system-ui;
    box-shadow: 0 0 0 2px transparent; transition: box-shadow .14s ease, transform .14s ease;
  }}
  .ava img {{ width: 100%; height: 100%; object-fit: cover; display: block; }}
  .ava:hover {{ box-shadow: 0 0 0 2px rgba(255,255,255,.45); }}
  .ava.on {{ box-shadow: 0 0 0 2.5px #fff; }}
  /* A bar on the window edge marks the current account even at a glance. */
  .slot {{ position: relative; display: grid; place-items: center; width: 100%; height: 46px; }}
  .slot.on::before {{
    content: ''; position: absolute; left: 0; top: 7px; bottom: 7px; width: 3px;
    border-radius: 0 3px 3px 0; background: #fff;
  }}
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
<div id="rail" style="display:flex;flex-direction:column;align-items:center;gap:8px;width:100%"></div>
<button class="add" id="add" title="Add a Google account">
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
    <path d="M12 6v12M6 12h12"/>
  </svg>
</button>
<button class="gear" id="gear" title="Edit accounts.json">
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
      const rail = document.getElementById('rail');
      rail.textContent = '';
      for (const a of state.accounts) {{
        const here = a.email.toLowerCase() === (state.active.email || '').toLowerCase();
        const slot = document.createElement('div');
        slot.className = 'slot' + (here ? ' on' : '');
        const ava = document.createElement('button');
        ava.className = 'ava' + (here ? ' on' : '');
        ava.style.background = a.color;
        ava.title = a.label ? `${{a.label}} (${{a.email}})` : a.email;
        if (a.avatar) {{
          const img = new Image();
          img.src = a.avatar;
          img.referrerPolicy = 'no-referrer';
          ava.appendChild(img);
        }} else ava.textContent = a.initials;
        // Switching account keeps whichever app you are already looking at.
        ava.onclick = () => send({{ type: 'show', email: a.email, service: state.active.service }});
        slot.appendChild(ava);
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
    height: 100vh; display: grid; place-items: center; padding: 28px;
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
  .dial label {{ flex: 1; font-size: 12.5px; }}
  .dial small {{ display: block; color: rgba(255,255,255,.38); font-size: 11px; margin-top: 2px; }}
  input {{
    width: 56px; font: inherit; text-align: center; padding: 5px; border-radius: 7px;
    border: 1px solid rgba(255,255,255,.14); background: rgba(0,0,0,.3); color: #fff;
  }}
  .withunit {{
    display: flex; align-items: center; gap: 7px; white-space: nowrap;
    color: rgba(255,255,255,.55); font-size: 12.5px;
  }}
  footer {{ display: flex; align-items: center; gap: 10px; margin-top: 20px; }}
  .link {{ font-size: 11.5px; color: rgba(255,255,255,.35); text-decoration: underline; }}
  .link:hover {{ color: #fff; }}
  .done {{
    margin-left: auto; padding: 8px 18px; border-radius: 9px;
    background: #fff; color: #11131a; font-weight: 600; font-size: 13px;
  }}
</style>
<div class="card">
  <header>
    <svg class="mark" viewBox="0 0 48 48" aria-hidden="true">
      <rect width="48" height="48" rx="11" fill="#11131a"/>
      <rect x="8" y="10" width="4" height="28" rx="2" fill="#6366f1"/>
      <circle cx="30" cy="15.5" r="4.2" fill="#fff"/>
      <circle cx="30" cy="24" r="4.2" fill="#fff"/>
      <circle cx="30" cy="32.5" r="4.2" fill="#fff"/>
    </svg>
    <h1>Masse</h1>
    <span class="ver">v{version}</span>
  </header>
  <p class="sub">Several Google accounts, one window.</p>

  <h2>Accounts</h2>
  <div id="accounts"></div>

  <h2>Memory</h2>
  <div class="dial">
    <label>Panes kept loaded
      <small>Fewer means less memory and a reload when you switch back.</small></label>
    <input id="maxLive" type="number" min="1" max="9">
  </div>
  <div class="dial">
    <label>Close unused panes after
      <small>0 keeps them loaded forever.</small></label>
    <span class="withunit"><input id="idle" type="number" min="0" max="600"> minutes</span>
  </div>

  <footer>
    <button class="link" id="json">Edit accounts.json</button>
    <button class="done" id="close">Done</button>
  </footer>
</div>
<script>
  {SHARED_JS}
  let armed = null;

  window.shim = {{
    render(state) {{
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
  document.addEventListener('keydown', (e) => {{ if (e.key === 'Escape') send({{ type: 'close' }}); }});
  window.shim.render({state});
</script>"##
    )
}
