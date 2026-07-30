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
  document.getElementById('gear').onclick = () => send({{ type: 'config' }});
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
