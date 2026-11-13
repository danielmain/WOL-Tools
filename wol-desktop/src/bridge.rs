use crate::state::ThemeMode;

pub const DARKREADER_JS: &str = include_str!("../ui/lib/darkreader.js");
pub const TOKENIZE_JS: &str = include_str!("../../wol-highlights/lib/tokenize.js");
pub const LANGUAGES_JS: &str = include_str!("../../wol-highlights/lib/languages.js");
pub const HIGHLIGHT_JS: &str = include_str!("../../wol-highlights/lib/highlight.js");
pub const CONTENT_JS: &str = include_str!("../../wol-highlights/content.js");
pub const CONTENT_CSS: &str = include_str!("../../wol-highlights/content.css");

pub fn build_injected_script(
    initial_marks_json: Option<&str>,
    initial_cookies_json: Option<&str>,
) -> String {
    build_injected_script_with_theme(
        initial_marks_json,
        initial_cookies_json,
        ThemeMode::default(),
    )
}

pub fn build_injected_script_with_theme(
    initial_marks_json: Option<&str>,
    initial_cookies_json: Option<&str>,
    initial_theme: ThemeMode,
) -> String {
    let initial_marks = initial_marks_json.unwrap_or("null");
    let initial_cookies = initial_cookies_json.unwrap_or(
        r#"{"cookieConsent-STRICTLY_NECESSARY":"true","cookieConsent-FUNCTIONAL":"true","cookieConsent-DIAGNOSTIC":"true","cookieConsent-USAGE":"true"}"#,
    );
    let initial_theme_str = initial_theme.as_str();
    let css_json = serde_json::to_string(CONTENT_CSS).unwrap_or_else(|_| "\"\"".to_string());

    let shim = format!(
        r##"
(function() {{
  // --- Persistent Cookie Shim for Desktop / Custom Protocol ---
  if (!window.__wolCookiesShimInstalled) {{
    window.__wolCookiesShimInstalled = true;

    const defaultCookies = {initial_cookies};
    const cookieStore = {{}};

    function mergeCookieObj(rawObj) {{
      if (!rawObj || typeof rawObj !== 'object') return;
      for (const k of Object.keys(rawObj)) {{
        const v = rawObj[k];
        if (typeof v === 'string') {{
          cookieStore[k] = {{ value: v, expiresAt: null }};
        }} else if (v && typeof v === 'object' && typeof v.value === 'string') {{
          cookieStore[k] = {{ value: v.value, expiresAt: v.expiresAt || null }};
        }}
      }}
    }}

    // 1. Seed defaults
    mergeCookieObj(defaultCookies);

    // 2. Seed from localStorage if available
    try {{
      const saved = window.localStorage && window.localStorage.getItem("__wol_cookies");
      if (saved) {{
        mergeCookieObj(JSON.parse(saved));
      }}
    }} catch (e) {{}}

    // 3. Seed from window.__WOL_INITIAL_COOKIES if set
    if (window.__WOL_INITIAL_COOKIES) {{
      mergeCookieObj(window.__WOL_INITIAL_COOKIES);
    }}

    function persistCookies() {{
      try {{
        if (window.localStorage) {{
          window.localStorage.setItem("__wol_cookies", JSON.stringify(cookieStore));
        }}
      }} catch (e) {{}}
    }}

    // Intercept window.__WOL_INITIAL_COOKIES assignment
    let _initialCookies = window.__WOL_INITIAL_COOKIES || null;
    try {{
      Object.defineProperty(window, '__WOL_INITIAL_COOKIES', {{
        get: function() {{ return _initialCookies; }},
        set: function(val) {{
          _initialCookies = val;
          mergeCookieObj(val);
          persistCookies();
        }},
        configurable: true,
        enumerable: true
      }});
    }} catch (e) {{}}

    function getCookieString() {{
      if (window.__WOL_INITIAL_COOKIES) {{
        mergeCookieObj(window.__WOL_INITIAL_COOKIES);
      }}
      const now = Date.now();
      const pairs = [];
      for (const name of Object.keys(cookieStore)) {{
        const entry = cookieStore[name];
        if (!entry) continue;
        if (entry.expiresAt !== null && entry.expiresAt <= now) {{
          delete cookieStore[name];
          continue;
        }}
        pairs.push(name + "=" + entry.value);
      }}
      return pairs.join("; ");
    }}

    function setCookieString(rawStr) {{
      if (typeof rawStr !== 'string') return;
      const parts = rawStr.split(';').map(function(s) {{ return s.trim(); }});
      if (!parts[0]) return;
      const eqIdx = parts[0].indexOf('=');
      if (eqIdx === -1) return;
      const name = parts[0].substring(0, eqIdx).trim();
      const value = parts[0].substring(eqIdx + 1).trim();
      if (!name) return;

      let expiresAt = null;
      for (let i = 1; i < parts.length; i++) {{
        const directive = parts[i];
        const dEq = directive.indexOf('=');
        const dName = (dEq === -1 ? directive : directive.substring(0, dEq)).trim().toLowerCase();
        const dVal = dEq === -1 ? '' : directive.substring(dEq + 1).trim();

        if (dName === 'expires') {{
          const d = new Date(dVal);
          if (!isNaN(d.getTime())) {{
            expiresAt = d.getTime();
          }}
        }} else if (dName === 'max-age') {{
          const secs = parseInt(dVal, 10);
          if (!isNaN(secs)) {{
            expiresAt = Date.now() + secs * 1000;
          }}
        }}
      }}

      if (expiresAt !== null && expiresAt <= Date.now()) {{
        delete cookieStore[name];
        persistCookies();
        try {{
          if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {{
            window.__TAURI_INTERNALS__.invoke("set_cookie", {{ name: name, value: null }});
          }}
        }} catch (e) {{}}
      }} else {{
        cookieStore[name] = {{ value: value, expiresAt: expiresAt }};
        persistCookies();
        try {{
          if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {{
            window.__TAURI_INTERNALS__.invoke("set_cookie", {{ name: name, value: value }});
          }}
        }} catch (e) {{}}
      }}
    }}

    function installHook(target) {{
      if (!target) return;
      try {{
        Object.defineProperty(target, 'cookie', {{
          get: getCookieString,
          set: setCookieString,
          configurable: true,
          enumerable: true
        }});
      }} catch (e) {{}}
    }}

    if (typeof Document !== 'undefined' && Document.prototype) installHook(Document.prototype);
    if (typeof HTMLDocument !== 'undefined' && HTMLDocument.prototype) installHook(HTMLDocument.prototype);
    if (typeof document !== 'undefined') installHook(document);

    try {{
      if (typeof Navigator !== 'undefined' && Navigator.prototype) {{
        Object.defineProperty(Navigator.prototype, 'cookieEnabled', {{
          get: function() {{ return true; }},
          configurable: true
        }});
      }}
    }} catch (e) {{}}
  }}

  // --- Highlights Webview Bridge ---
  if (window.__wolBridgeInstalled) return;
  window.__wolBridgeInstalled = true;

  let currentMarks = {initial_marks};
  const storageListeners = [];
  const messageListeners = [];

  window.__wolDispatchMessage = function(message) {{
    for (const cb of messageListeners) {{
      try {{
        cb(message, {{}}, function() {{}});
      }} catch (e) {{
        console.warn("[__wolBridge] message listener error:", e);
      }}
    }}
  }};

  window.__wolBridge = {{
    storage: {{
      local: {{
        get: async function(keys) {{
          try {{
            if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {{
              const res = await window.__TAURI_INTERNALS__.invoke("get_marks");
              if (res !== undefined) currentMarks = res;
            }}
          }} catch (e) {{
            console.warn("[__wolBridge] invoke get_marks failed:", e);
          }}
          if (keys === "wolMarks" || keys === null || keys === undefined) {{
            return {{ wolMarks: currentMarks }};
          }}
          if (Array.isArray(keys)) {{
            const out = {{}};
            if (keys.includes("wolMarks")) out.wolMarks = currentMarks;
            return out;
          }}
          return {{ wolMarks: currentMarks }};
        }},
        set: async function(items) {{
          if (items && items.wolMarks !== undefined) {{
            currentMarks = items.wolMarks;
            try {{
              if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {{
                await window.__TAURI_INTERNALS__.invoke("set_marks", {{ marks: currentMarks }});
              }}
            }} catch (e) {{
              console.warn("[__wolBridge] invoke set_marks failed:", e);
            }}
          }}
        }},
        remove: async function(key) {{
          if (key === "wolMarks") {{
            currentMarks = null;
            try {{
              if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {{
                await window.__TAURI_INTERNALS__.invoke("set_marks", {{ marks: null }});
              }}
            }} catch (e) {{}}
          }}
        }}
      }},
      onChanged: {{
        addListener: function(cb) {{
          if (typeof cb === "function") storageListeners.push(cb);
        }}
      }}
    }},
    runtime: {{
      onMessage: {{
        addListener: function(cb) {{
          if (typeof cb === "function") messageListeners.push(cb);
        }}
      }}
    }}
  }};

  window.chrome = window.__wolBridge;
  window.browser = window.__wolBridge;

  function injectStyle() {{
    if (document.getElementById("wol-highlights-css")) return;
    const style = document.createElement("style");
    style.id = "wol-highlights-css";
    style.textContent = {css_json} + "\n.lnc-firstRunPopup {{ display: none !important; }}\n.article {{ max-width: none !important; padding: 1em 20px 5em 20px !important; }}\n";
    (document.head || document.documentElement).appendChild(style);
  }}

  function injectNavPill() {{
    if (window.location.protocol !== "wol:" || window.top !== window.self) return;
    if (document.getElementById("wol-desktop-nav-pill")) return;

    if (!document.getElementById("wol-desktop-nav-css")) {{
      const navStyle = document.createElement("style");
      navStyle.id = "wol-desktop-nav-css";
      navStyle.classList.add("darkreader");
      navStyle.textContent = `
        #wol-desktop-nav-pill {{
          position: fixed;
          top: 7px;
          left: 84px;
          z-index: 2147483000;
          display: inline-flex;
          align-items: center;
          padding: 3px;
          gap: 2px;
          border-radius: 9999px;
          background: rgba(255, 255, 255, 0.72);
          border: 1px solid rgba(0, 0, 0, 0.08);
          box-shadow: 0 4px 16px rgba(0, 0, 0, 0.10), 0 1px 3px rgba(0, 0, 0, 0.06);
          backdrop-filter: blur(14px) saturate(160%);
          -webkit-backdrop-filter: blur(14px) saturate(160%);
          opacity: 0.72;
          transition: opacity 0.18s ease, transform 0.18s ease, box-shadow 0.18s ease;
          user-select: none;
          -webkit-user-select: none;
        }}
        #wol-desktop-nav-pill:hover {{
          opacity: 1;
          box-shadow: 0 6px 20px rgba(0, 0, 0, 0.14), 0 1px 4px rgba(0, 0, 0, 0.08);
        }}
        #wol-desktop-nav-pill .wol-nav-sep {{
          width: 1px;
          height: 16px;
          background: rgba(0, 0, 0, 0.10);
          flex-shrink: 0;
        }}
        #wol-desktop-nav-pill button {{
          appearance: none;
          -webkit-appearance: none;
          border: none;
          background: transparent;
          color: #1f2937;
          width: 30px;
          height: 30px;
          padding: 0;
          margin: 0;
          border-radius: 9999px;
          display: inline-flex;
          align-items: center;
          justify-content: center;
          cursor: pointer;
          transition: background-color 0.15s ease, transform 0.12s ease, opacity 0.15s ease;
        }}
        #wol-desktop-nav-pill button:hover:not(:disabled) {{
          background: rgba(0, 0, 0, 0.07);
        }}
        #wol-desktop-nav-pill button:active:not(:disabled) {{
          transform: scale(0.90);
          background: rgba(0, 0, 0, 0.12);
        }}
        #wol-desktop-nav-pill button:disabled {{
          opacity: 0.30;
          cursor: default;
        }}
        #wol-desktop-nav-pill svg {{
          width: 16px;
          height: 16px;
          stroke: currentColor;
          fill: none;
          stroke-width: 2;
          stroke-linecap: round;
          stroke-linejoin: round;
        }}
        #wol-desktop-nav-pill button.wol-bible-btn {{
          width: auto;
          padding: 0 10px 0 8px;
          gap: 5px;
          font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
          font-size: 12px;
          font-weight: 500;
          white-space: nowrap;
        }}
        #wol-desktop-nav-pill button.wol-bible-btn.is-done {{
          color: #15803d;
        }}
        #wol-desktop-nav-pill button.wol-bible-btn.is-error {{
          color: #b91c1c;
        }}
        html[data-wol-theme="dark"] #wol-desktop-nav-pill {{
          background: rgba(30, 32, 36, 0.74);
          border-color: rgba(255, 255, 255, 0.12);
          box-shadow: 0 4px 16px rgba(0, 0, 0, 0.36);
        }}
        html[data-wol-theme="dark"] #wol-desktop-nav-pill .wol-nav-sep {{
          background: rgba(255, 255, 255, 0.14);
        }}
        html[data-wol-theme="dark"] #wol-desktop-nav-pill button {{
          color: #f3f4f6;
        }}
        html[data-wol-theme="dark"] #wol-desktop-nav-pill button:hover:not(:disabled) {{
          background: rgba(255, 255, 255, 0.10);
        }}
        html[data-wol-theme="dark"] #wol-desktop-nav-pill button:active:not(:disabled) {{
          background: rgba(255, 255, 255, 0.16);
        }}
        html[data-wol-theme="dark"] #wol-desktop-nav-pill button.wol-bible-btn.is-done {{
          color: #4ade80;
        }}
        html[data-wol-theme="dark"] #wol-desktop-nav-pill button.wol-bible-btn.is-error {{
          color: #f87171;
        }}
        #wol-desktop-nav-pill button.is-active {{
          background: rgba(37, 99, 235, 0.14);
          color: #2563eb;
        }}
        html[data-wol-theme="dark"] #wol-desktop-nav-pill button.is-active {{
          background: rgba(59, 130, 246, 0.24);
          color: #93c5fd;
        }}
        #wol-desktop-settings-popover {{
          position: fixed;
          top: 48px;
          left: 84px;
          z-index: 2147483001;
          width: 316px;
          padding: 14px;
          border-radius: 14px;
          background: rgba(255, 255, 255, 0.92);
          border: 1px solid rgba(15, 23, 42, 0.10);
          box-shadow: 0 18px 42px rgba(15, 23, 42, 0.18), 0 2px 8px rgba(15, 23, 42, 0.08);
          backdrop-filter: blur(20px) saturate(180%);
          -webkit-backdrop-filter: blur(20px) saturate(180%);
          color: #0f172a;
          font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
          font-size: 13px;
          line-height: 1.4;
          display: none;
          flex-direction: column;
          gap: 12px;
          box-sizing: border-box;
        }}
        #wol-desktop-settings-popover.is-open {{
          display: flex;
        }}
        #wol-desktop-settings-popover * {{
          box-sizing: border-box;
        }}
        .wol-pop-header {{
          display: flex;
          align-items: center;
          justify-content: space-between;
          gap: 8px;
        }}
        .wol-pop-title {{
          display: flex;
          align-items: center;
          gap: 8px;
          font-weight: 700;
          font-size: 14px;
          letter-spacing: -0.01em;
        }}
        .wol-pop-badge {{
          display: inline-flex;
          align-items: center;
          gap: 5px;
          padding: 2px 8px;
          border-radius: 9999px;
          font-size: 11px;
          font-weight: 600;
          background: rgba(34, 197, 94, 0.14);
          color: #15803d;
        }}
        .wol-pop-badge.is-offline {{
          background: rgba(239, 68, 68, 0.14);
          color: #b91c1c;
        }}
        .wol-pop-badge-dot {{
          width: 6px;
          height: 6px;
          border-radius: 50%;
          background: currentColor;
        }}
        .wol-pop-card {{
          background: rgba(248, 250, 252, 0.85);
          border: 1px solid rgba(15, 23, 42, 0.08);
          border-radius: 10px;
          padding: 11px 12px;
          display: flex;
          flex-direction: column;
          gap: 9px;
        }}
        .wol-pop-section-label {{
          font-size: 11px;
          font-weight: 700;
          text-transform: uppercase;
          letter-spacing: 0.05em;
          color: #64748b;
        }}
        .wol-pop-status {{
          font-size: 13px;
          font-weight: 600;
          color: #1e293b;
        }}
        .wol-pop-substatus {{
          font-size: 11.5px;
          color: #64748b;
          white-space: nowrap;
          overflow: hidden;
          text-overflow: ellipsis;
        }}
        .wol-pop-toggle-row {{
          display: flex;
          align-items: center;
          justify-content: space-between;
          font-size: 12.5px;
          font-weight: 500;
          color: #334155;
          cursor: pointer;
          user-select: none;
        }}
        .wol-pop-toggle-row input {{
          width: 15px;
          height: 15px;
          accent-color: #2563eb;
          cursor: pointer;
        }}
        .wol-pop-btn-row {{
          display: grid;
          grid-template-columns: 1fr 1fr;
          gap: 8px;
        }}
        .wol-pop-btn {{
          appearance: none;
          -webkit-appearance: none;
          border: 1px solid rgba(15, 23, 42, 0.12);
          background: #ffffff;
          color: #1e293b;
          border-radius: 8px;
          padding: 7px 10px;
          font-family: inherit;
          font-size: 12.5px;
          font-weight: 600;
          display: inline-flex;
          align-items: center;
          justify-content: center;
          gap: 6px;
          cursor: pointer;
          transition: background-color 0.14s ease, border-color 0.14s ease, transform 0.1s ease;
        }}
        .wol-pop-btn:hover:not(:disabled) {{
          background: #f1f5f9;
        }}
        .wol-pop-btn:active:not(:disabled) {{
          transform: scale(0.98);
        }}
        .wol-pop-btn:disabled {{
          opacity: 0.45;
          cursor: not-allowed;
        }}
        .wol-pop-btn.is-primary {{
          background: #2563eb;
          border-color: #2563eb;
          color: #ffffff;
        }}
        .wol-pop-btn.is-primary:hover:not(:disabled) {{
          background: #1d4ed8;
          border-color: #1d4ed8;
        }}
        .wol-pop-btn.is-danger {{
          color: #dc2626;
          border-color: rgba(220, 38, 38, 0.28);
        }}
        .wol-pop-btn.is-danger:hover:not(:disabled) {{
          background: rgba(220, 38, 38, 0.08);
          border-color: rgba(220, 38, 38, 0.45);
        }}
        .wol-pop-btn svg {{
          width: 14px;
          height: 14px;
          stroke: currentColor;
          fill: none;
          stroke-width: 2;
          stroke-linecap: round;
          stroke-linejoin: round;
          flex-shrink: 0;
        }}
        .wol-pop-segmented {{
          display: grid;
          grid-template-columns: repeat(3, 1fr);
          background: rgba(15, 23, 42, 0.06);
          padding: 3px;
          border-radius: 8px;
          gap: 2px;
        }}
        .wol-pop-seg-btn {{
          appearance: none;
          -webkit-appearance: none;
          border: none;
          background: transparent;
          color: #475569;
          border-radius: 6px;
          padding: 5px 6px;
          font-family: inherit;
          font-size: 12px;
          font-weight: 600;
          cursor: pointer;
          transition: background-color 0.14s ease, color 0.14s ease;
        }}
        .wol-pop-seg-btn.is-selected {{
          background: #ffffff;
          color: #0f172a;
          box-shadow: 0 1px 3px rgba(15, 23, 42, 0.12);
        }}
        .wol-pop-msg {{
          font-size: 11.5px;
          color: #15803d;
          min-height: 0;
          word-break: break-word;
        }}
        .wol-pop-msg.is-error {{
          color: #b91c1c;
        }}
        .wol-pop-footer {{
          display: flex;
          align-items: center;
          justify-content: space-between;
          gap: 8px;
          padding-top: 2px;
        }}
        html[data-wol-theme="dark"] #wol-desktop-settings-popover {{
          color-scheme: dark;
          background: rgba(24, 26, 32, 0.96);
          border-color: rgba(255, 255, 255, 0.12);
          box-shadow: 0 18px 42px rgba(0, 0, 0, 0.55);
          color: #f1f5f9;
        }}
        html[data-wol-theme="dark"] .wol-pop-badge {{
          background: rgba(34, 197, 94, 0.18);
          color: #4ade80;
        }}
        html[data-wol-theme="dark"] .wol-pop-badge.is-offline {{
          background: rgba(239, 68, 68, 0.18);
          color: #f87171;
        }}
        html[data-wol-theme="dark"] .wol-pop-card {{
          background: rgba(15, 23, 42, 0.55);
          border-color: rgba(255, 255, 255, 0.08);
        }}
        html[data-wol-theme="dark"] .wol-pop-section-label,
        html[data-wol-theme="dark"] .wol-pop-substatus {{
          color: #94a3b8;
        }}
        html[data-wol-theme="dark"] .wol-pop-status {{
          color: #f8fafc;
        }}
        html[data-wol-theme="dark"] .wol-pop-toggle-row {{
          color: #e2e8f0;
        }}
        html[data-wol-theme="dark"] .wol-pop-btn {{
          background: rgba(255, 255, 255, 0.08);
          border-color: rgba(255, 255, 255, 0.14);
          color: #f1f5f9;
        }}
        html[data-wol-theme="dark"] .wol-pop-btn:hover:not(:disabled) {{
          background: rgba(255, 255, 255, 0.14);
        }}
        html[data-wol-theme="dark"] .wol-pop-btn.is-primary {{
          background: #2563eb;
          border-color: #3b82f6;
          color: #ffffff;
        }}
        html[data-wol-theme="dark"] .wol-pop-btn.is-primary:hover:not(:disabled) {{
          background: #1d4ed8;
        }}
        html[data-wol-theme="dark"] .wol-pop-btn.is-danger {{
          color: #f87171;
          border-color: rgba(248, 113, 113, 0.30);
        }}
        html[data-wol-theme="dark"] .wol-pop-btn.is-danger:hover:not(:disabled) {{
          background: rgba(248, 113, 113, 0.14);
          border-color: rgba(248, 113, 113, 0.50);
        }}
        html[data-wol-theme="dark"] .wol-pop-segmented {{
          background: rgba(255, 255, 255, 0.08);
        }}
        html[data-wol-theme="dark"] .wol-pop-seg-btn {{
          color: #94a3b8;
        }}
        html[data-wol-theme="dark"] .wol-pop-seg-btn.is-selected {{
          background: rgba(255, 255, 255, 0.16);
          color: #ffffff;
        }}
        html[data-wol-theme="dark"] .wol-pop-msg {{
          color: #4ade80;
        }}
        html[data-wol-theme="dark"] .wol-pop-msg.is-error {{
          color: #f87171;
        }}
      `;
      (document.head || document.documentElement).appendChild(navStyle);
    }}

    const pill = document.createElement("div");
    pill.id = "wol-desktop-nav-pill";
    pill.setAttribute("role", "navigation");
    pill.setAttribute("aria-label", "History navigation");

    const backBtn = document.createElement("button");
    backBtn.type = "button";
    backBtn.title = "Back";
    backBtn.setAttribute("aria-label", "Back");
    backBtn.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><polyline points="15 18 9 12 15 6"/></svg>';
    backBtn.addEventListener("click", function() {{ window.history.back(); }});

    const sep = document.createElement("span");
    sep.className = "wol-nav-sep";
    sep.setAttribute("aria-hidden", "true");

    const fwdBtn = document.createElement("button");
    fwdBtn.type = "button";
    fwdBtn.title = "Forward";
    fwdBtn.setAttribute("aria-label", "Forward");
    fwdBtn.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><polyline points="9 18 15 12 9 6"/></svg>';
    fwdBtn.addEventListener("click", function() {{ window.history.forward(); }});

    pill.appendChild(backBtn);
    pill.appendChild(sep);
    pill.appendChild(fwdBtn);

    const settingsSep = document.createElement("span");
    settingsSep.className = "wol-nav-sep";
    settingsSep.setAttribute("aria-hidden", "true");

    const settingsBtn = document.createElement("button");
    settingsBtn.id = "wol-desktop-settings-btn";
    settingsBtn.type = "button";
    settingsBtn.title = "Highlights & Settings";
    settingsBtn.setAttribute("aria-label", "Highlights & Settings");
    settingsBtn.setAttribute("aria-expanded", "false");
    settingsBtn.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>';

    pill.appendChild(settingsSep);
    pill.appendChild(settingsBtn);

    const popover = document.createElement("div");
    popover.id = "wol-desktop-settings-popover";
    popover.setAttribute("role", "dialog");
    popover.setAttribute("aria-label", "OfflineWOL Quick Settings");
    popover.innerHTML = `
      <div class="wol-pop-header">
        <div class="wol-pop-title">
          <span>OfflineWOL</span>
        </div>
        <span id="wol-pop-net" class="wol-pop-badge"><span class="wol-pop-badge-dot"></span><span id="wol-pop-net-text">Online</span></span>
      </div>
      <div class="wol-pop-card">
        <div class="wol-pop-section-label">Study Backup &amp; Highlights</div>
        <div>
          <div id="wol-pop-hl-status" class="wol-pop-status">No backup loaded</div>
          <div id="wol-pop-hl-file" class="wol-pop-substatus">Load a .jwlibrary or .json backup to view highlights</div>
        </div>
        <label id="wol-pop-toggle-wrap" class="wol-pop-toggle-row" style="display:none;">
          <span>Show highlights on page</span>
          <input id="wol-pop-enabled" type="checkbox">
        </label>
        <div class="wol-pop-btn-row">
          <button id="wol-pop-load" type="button" class="wol-pop-btn is-primary">
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
            <span>Load Backup</span>
          </button>
          <button id="wol-pop-export" type="button" class="wol-pop-btn">
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
            <span>Export</span>
          </button>
        </div>
        <button id="wol-pop-reset" type="button" class="wol-pop-btn is-danger" style="width:100%;" disabled>
          <svg viewBox="0 0 24 24" aria-hidden="true"><polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6"/><path d="M14 11v6"/><path d="M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2"/></svg>
          <span>Reset All Highlights &amp; Comments</span>
        </button>
        <div id="wol-pop-msg" class="wol-pop-msg"></div>
      </div>
      <div class="wol-pop-card">
        <div class="wol-pop-section-label">Appearance</div>
        <div class="wol-pop-segmented" role="group" aria-label="Appearance">
          <button type="button" class="wol-pop-seg-btn" data-theme="system">System</button>
          <button type="button" class="wol-pop-seg-btn" data-theme="light">Light</button>
          <button type="button" class="wol-pop-seg-btn" data-theme="dark">Dark</button>
        </div>
      </div>
      <div class="wol-pop-footer">
        <button id="wol-pop-home" type="button" class="wol-pop-btn" style="flex:1;">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 9l9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/></svg>
          <span>Home</span>
        </button>
        <button id="wol-pop-full" type="button" class="wol-pop-btn" style="flex:1.4;">
          <svg viewBox="0 0 24 24" aria-hidden="true"><line x1="4" y1="21" x2="4" y2="14"/><line x1="4" y1="10" x2="4" y2="3"/><line x1="12" y1="21" x2="12" y2="12"/><line x1="12" y1="8" x2="12" y2="3"/><line x1="20" y1="21" x2="20" y2="16"/><line x1="20" y1="12" x2="20" y2="3"/><line x1="1" y1="14" x2="7" y2="14"/><line x1="9" y1="8" x2="15" y2="8"/><line x1="17" y1="16" x2="23" y2="16"/></svg>
          <span>All Settings…</span>
        </button>
      </div>
    `;

    let popCachedSettings = null;
    function setPopMessage(text, isErr) {{
      const el = popover.querySelector("#wol-pop-msg");
      if (!el) return;
      el.textContent = text || "";
      el.classList.toggle("is-error", Boolean(isErr));
    }}

    function syncPopNet() {{
      const badge = popover.querySelector("#wol-pop-net");
      const text = popover.querySelector("#wol-pop-net-text");
      if (!badge || !text) return;
      const online = navigator.onLine;
      badge.classList.toggle("is-offline", !online);
      text.textContent = online ? "Online" : "Offline";
    }}

    async function refreshPopoverState() {{
      syncPopNet();
      setPopMessage("");
      const statusEl = popover.querySelector("#wol-pop-hl-status");
      const fileEl = popover.querySelector("#wol-pop-hl-file");
      const toggleWrap = popover.querySelector("#wol-pop-toggle-wrap");
      const enabledCb = popover.querySelector("#wol-pop-enabled");
      const exportBtn = popover.querySelector("#wol-pop-export");
      const resetBtn = popover.querySelector("#wol-pop-reset");
      const segBtns = popover.querySelectorAll(".wol-pop-seg-btn");

      try {{
        if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {{
          const [marks, settings] = await Promise.all([
            window.__TAURI_INTERNALS__.invoke("get_marks"),
            window.__TAURI_INTERNALS__.invoke("get_settings")
          ]);
          if (marks !== undefined) currentMarks = marks;
          popCachedSettings = settings;

          if (marks && (marks.highlightCount > 0 || marks.noteCount > 0)) {{
            const hl = marks.highlightCount || 0;
            const nt = marks.noteCount || 0;
            const parts = [hl.toLocaleString() + " highlight" + (hl === 1 ? "" : "s")];
            if (nt > 0) parts.push(nt.toLocaleString() + " note" + (nt === 1 ? "" : "s"));
            statusEl.textContent = parts.join(" · ");
            fileEl.textContent = marks.fileName ? "From " + marks.fileName : "Active study backup";
            toggleWrap.style.display = "flex";
            enabledCb.checked = marks.enabled !== false;
            exportBtn.disabled = false;
            resetBtn.disabled = false;
          }} else {{
            statusEl.textContent = "No backup loaded";
            fileEl.textContent = "Load a .jwlibrary or .json backup to view highlights";
            toggleWrap.style.display = "none";
            exportBtn.disabled = true;
            resetBtn.disabled = true;
          }}

          const activeTheme = (settings && settings.theme) || "system";
          segBtns.forEach(function(btn) {{
            btn.classList.toggle("is-selected", btn.getAttribute("data-theme") === activeTheme);
          }});
        }}
      }} catch (err) {{
        console.warn("[__wolBridge] refreshPopoverState error:", err);
      }}
    }}

    function closePopover() {{
      popover.classList.remove("is-open");
      settingsBtn.classList.remove("is-active");
      settingsBtn.setAttribute("aria-expanded", "false");
    }}

    function togglePopover() {{
      const willOpen = !popover.classList.contains("is-open");
      if (willOpen) {{
        popover.classList.add("is-open");
        settingsBtn.classList.add("is-active");
        settingsBtn.setAttribute("aria-expanded", "true");
        refreshPopoverState();
      }} else {{
        closePopover();
      }}
    }}

    settingsBtn.addEventListener("click", function(e) {{
      e.stopPropagation();
      togglePopover();
    }});

    popover.addEventListener("click", function(e) {{
      e.stopPropagation();
    }});

    document.addEventListener("click", function() {{
      if (popover.classList.contains("is-open")) closePopover();
    }});

    document.addEventListener("keydown", function(e) {{
      if (e.key === "Escape" && popover.classList.contains("is-open")) closePopover();
    }});

    popover.querySelector("#wol-pop-enabled").addEventListener("change", async function(e) {{
      if (!currentMarks) return;
      currentMarks.enabled = e.target.checked;
      try {{
        await window.__TAURI_INTERNALS__.invoke("set_marks", {{ marks: currentMarks }});
      }} catch (err) {{
        setPopMessage("Failed to update highlights.", true);
      }}
    }});

    popover.querySelector("#wol-pop-load").addEventListener("click", async function() {{
      closePopover();
      try {{
        await window.__TAURI_INTERNALS__.invoke("open_manager_window", {{ tab: "highlights" }});
      }} catch (err) {{}}
    }});

    popover.querySelector("#wol-pop-export").addEventListener("click", async function() {{
      setPopMessage("");
      try {{
        const defaultName = "UserdataBackup_" + new Date().toISOString().slice(0, 10) + "_OfflineWOL.jwlibrary";
        const savedPath = await window.__TAURI_INTERNALS__.invoke("export_marks_to_file", {{ defaultName: defaultName }});
        if (savedPath) {{
          const fileName = savedPath.split(/[\\/]/).pop() || savedPath;
          setPopMessage("Exported to " + fileName);
        }}
      }} catch (err) {{
        setPopMessage(String(err), true);
      }}
    }});

    popover.querySelector("#wol-pop-reset").addEventListener("click", async function() {{
      if (!window.confirm("Reset all highlights and comments? This will clear all markers and notes so you can start fresh.")) return;
      setPopMessage("");
      try {{
        currentMarks = null;
        await window.__TAURI_INTERNALS__.invoke("set_marks", {{ marks: null }});
        await refreshPopoverState();
        setPopMessage("All highlights and comments reset.");
      }} catch (err) {{
        setPopMessage("Failed to reset highlights.", true);
      }}
    }});

    popover.querySelectorAll(".wol-pop-seg-btn").forEach(function(btn) {{
      btn.addEventListener("click", async function() {{
        const theme = btn.getAttribute("data-theme");
        if (!popCachedSettings) return;
        const updated = Object.assign({{}}, popCachedSettings, {{ theme: theme }});
        try {{
          await window.__TAURI_INTERNALS__.invoke("save_settings", {{ settings: updated }});
          popCachedSettings = updated;
          popover.querySelectorAll(".wol-pop-seg-btn").forEach(function(b) {{
            b.classList.toggle("is-selected", b.getAttribute("data-theme") === theme);
          }});
        }} catch (err) {{
          setPopMessage("Failed to save theme.", true);
        }}
      }});
    }});

    popover.querySelector("#wol-pop-home").addEventListener("click", async function() {{
      closePopover();
      try {{
        await window.__TAURI_INTERNALS__.invoke("navigate_home");
      }} catch (err) {{}}
    }});

    popover.querySelector("#wol-pop-full").addEventListener("click", async function() {{
      closePopover();
      try {{
        await window.__TAURI_INTERNALS__.invoke("open_manager_window", {{ tab: "settings" }});
      }} catch (err) {{}}
    }});

    // Contextual offline download button on /wol/binav/... and /wol/library/... pages
    const binavRe = /^(?:\/[^/]+)?\/wol\/binav\/r\d+\/lp-[^/]+(?:\/[^/]+(\/\d+)?)?\/?$/;
    const libraryRe = /^(?:\/[^/]+)?\/wol\/library\/r\d+\/lp-[^/]+\/.+/;
    const downloadIcon = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>';
    const checkIcon = '<svg viewBox="0 0 24 24" aria-hidden="true"><polyline points="20 6 9 17 4 12"/></svg>';

    let bibleSep = null;
    let bibleBtn = null;
    let isCrawling = false;
    let activeMode = null;
    let lastProgress = null;
    let lastProgressPath = null;

    function ensureBibleBtn() {{
      if (bibleBtn) return;
      bibleSep = document.createElement("span");
      bibleSep.className = "wol-nav-sep";
      bibleSep.setAttribute("aria-hidden", "true");

      bibleBtn = document.createElement("button");
      bibleBtn.id = "wol-desktop-bible-btn";
      bibleBtn.type = "button";
      bibleBtn.className = "wol-bible-btn";

      bibleBtn.addEventListener("click", async function() {{
        if (!window.__TAURI_INTERNALS__ || !window.__TAURI_INTERNALS__.invoke) return;
        if (isCrawling) {{
          try {{
            const cancelCmd = activeMode === "library" ? "cancel_library_offline" : "cancel_bible_offline";
            await window.__TAURI_INTERNALS__.invoke(cancelCmd);
          }} catch (e) {{}}
          return;
        }}
        const path = window.location.pathname || "";
        const isLib = libraryRe.test(path);
        activeMode = isLib ? "library" : "bible";
        isCrawling = true;
        lastProgress = null;
        lastProgressPath = path;
        bibleBtn.classList.remove("is-done", "is-error");
        bibleBtn.innerHTML = downloadIcon + "<span>Indexing…</span>";
        bibleBtn.title = "Downloading offline… click to cancel";
        try {{
          if (isLib) {{
            await window.__TAURI_INTERNALS__.invoke("save_library_offline", {{
              libraryUrl: window.location.href
            }});
          }} else {{
            await window.__TAURI_INTERNALS__.invoke("save_bible_offline", {{
              binavUrl: window.location.href
            }});
          }}
        }} catch (err) {{
          isCrawling = false;
          bibleBtn.classList.add("is-error");
          bibleBtn.innerHTML = downloadIcon + "<span>Retry</span>";
          bibleBtn.title = "Download error: " + String(err);
        }}
      }});

      pill.appendChild(bibleSep);
      pill.appendChild(bibleBtn);
    }}

    function renderBibleProgress(p, defaultLabel, defaultTitle) {{
      if (!bibleBtn) return;
      bibleBtn.classList.remove("is-done", "is-error");
      if (!p) {{
        bibleBtn.innerHTML = downloadIcon + "<span>" + defaultLabel + "</span>";
        bibleBtn.title = defaultTitle;
        return;
      }}
      const total = p.total_chapters !== undefined ? p.total_chapters : (p.total_articles || 0);
      const completed = p.completed_chapters !== undefined ? p.completed_chapters : (p.completed_articles || 0);
      const failed = p.failed_chapters !== undefined ? p.failed_chapters : (p.failed_articles || 0);
      const unit = p.total_articles !== undefined ? "articles" : "chapters";

      if (p.done) {{
        isCrawling = false;
        if (p.cancelled) {{
          bibleBtn.innerHTML = downloadIcon + "<span>Resume (" + completed + "/" + total + ")</span>";
          bibleBtn.title = "Download cancelled — click to resume";
        }} else if (failed > 0) {{
          bibleBtn.classList.add("is-done");
          bibleBtn.innerHTML = checkIcon + "<span>" + completed + "/" + total + "</span>";
          bibleBtn.title = failed + " " + unit + " failed — click to retry";
        }} else {{
          bibleBtn.classList.add("is-done");
          bibleBtn.innerHTML = checkIcon + "<span>Cached</span>";
          bibleBtn.title = "Saved offline (" + completed + " " + unit + ")";
        }}
      }} else {{
        isCrawling = true;
        let label = "Indexing…";
        if (p.phase === "indexing" && p.categories_scanned > 0) {{
          label = "Indexing (" + p.categories_scanned + ")…";
        }} else if (total > 0) {{
          label = completed + "/" + total;
        }}
        bibleBtn.innerHTML = downloadIcon + "<span>" + label + "</span>";
        bibleBtn.title = "Downloading offline… click to cancel";
      }}
    }}

    function syncBibleBtn() {{
      const path = window.location.pathname || "";
      const binavMatch = binavRe.exec(path);
      const libMatch = !binavMatch ? libraryRe.exec(path) : null;
      const hasInvoke = Boolean(window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke);
      if ((!binavMatch && !libMatch) || !hasInvoke) {{
        if (bibleSep) bibleSep.style.display = "none";
        if (bibleBtn) bibleBtn.style.display = "none";
        return;
      }}
      ensureBibleBtn();
      bibleSep.style.display = "";
      bibleBtn.style.display = "";
      let defaultLabel;
      let defaultTitle;
      if (binavMatch) {{
        const isSingleBook = Boolean(binavMatch[1]);
        defaultLabel = isSingleBook ? "Save Book" : "Save Bible";
        defaultTitle = isSingleBook
          ? "Download all chapters in this book for offline use"
          : "Download the entire Bible for offline use";
      }} else {{
        defaultLabel = "Save Offline";
        defaultTitle = "Download all articles in this publication or category for offline use";
      }}
      const effectiveProgress = (isCrawling || lastProgressPath === path) ? lastProgress : null;
      renderBibleProgress(effectiveProgress, defaultLabel, defaultTitle);
    }}

    window.__wolBibleCrawlProgress = function(p) {{
      activeMode = "bible";
      lastProgress = p;
      if (p && !p.done) isCrawling = true;
      if (p && p.done) isCrawling = false;
      syncBibleBtn();
    }};

    window.__wolLibraryCrawlProgress = function(p) {{
      activeMode = "library";
      lastProgress = p;
      if (p && !p.done) isCrawling = true;
      if (p && p.done) isCrawling = false;
      syncBibleBtn();
    }};

    syncBibleBtn();
    window.addEventListener("pageshow", syncBibleBtn);
    window.addEventListener("popstate", syncBibleBtn);
    if (window.navigation && window.navigation.addEventListener) {{
      window.navigation.addEventListener("currententrychange", syncBibleBtn);
    }}
    if (!window.__wolHistoryPatched) {{
      window.__wolHistoryPatched = true;
      const origPush = history.pushState;
      const origReplace = history.replaceState;
      history.pushState = function() {{
        const r = origPush.apply(this, arguments);
        setTimeout(syncBibleBtn, 0);
        return r;
      }};
      history.replaceState = function() {{
        const r = origReplace.apply(this, arguments);
        setTimeout(syncBibleBtn, 0);
        return r;
      }};
    }}

    function syncNavState() {{
      if (window.navigation && typeof window.navigation.canGoBack === "boolean") {{
        backBtn.disabled = !window.navigation.canGoBack;
        fwdBtn.disabled = !window.navigation.canGoForward;
      }}
    }}

    syncNavState();
    window.addEventListener("pageshow", syncNavState);
    window.addEventListener("popstate", syncNavState);
    if (window.navigation && window.navigation.addEventListener) {{
      window.navigation.addEventListener("currententrychange", syncNavState);
    }}

    if (document.body) {{
      document.body.appendChild(pill);
      document.body.appendChild(popover);
    }}

    // Two-finger touchpad horizontal swipe support (back / forward)
    if (!window.__wolSwipeNavInstalled) {{
      window.__wolSwipeNavInstalled = true;
      let swipeDeltaX = 0;
      let swipeResetTimer = null;
      let swipeCooldownUntil = 0;

      function canScrollHorizontally(target, deltaX) {{
        let el = target && target.nodeType === 1 ? target : null;
        while (el && el !== document.documentElement) {{
          if (el.scrollWidth > el.clientWidth + 1) {{
            const style = window.getComputedStyle(el);
            const overflowX = style.overflowX;
            if (overflowX === "auto" || overflowX === "scroll") {{
              if (deltaX < 0 && el.scrollLeft > 0) return true;
              if (deltaX > 0 && el.scrollLeft + el.clientWidth < el.scrollWidth - 1) return true;
            }}
          }}
          el = el.parentElement;
        }}
        if (document.documentElement.scrollWidth > document.documentElement.clientWidth + 1) {{
          if (deltaX < 0 && window.scrollX > 0) return true;
          if (deltaX > 0 && window.scrollX + window.innerWidth < document.documentElement.scrollWidth - 1) return true;
        }}
        return false;
      }}

      function triggerSwipe(direction) {{
        const btn = direction === "back" ? backBtn : fwdBtn;
        if (btn.disabled) return;
        btn.style.background = "rgba(128, 128, 128, 0.22)";
        setTimeout(function() {{ btn.style.background = ""; }}, 180);
        if (direction === "back") {{
          window.history.back();
        }} else {{
          window.history.forward();
        }}
      }}

      window.addEventListener("wheel", function(e) {{
        const now = Date.now();
        if (now < swipeCooldownUntil) return;
        if (Math.abs(e.deltaX) <= Math.abs(e.deltaY) * 1.5 || Math.abs(e.deltaX) < 2) {{
          swipeDeltaX = 0;
          return;
        }}
        if (canScrollHorizontally(e.target, e.deltaX)) {{
          swipeDeltaX = 0;
          return;
        }}

        swipeDeltaX += e.deltaX;
        if (swipeResetTimer) clearTimeout(swipeResetTimer);
        swipeResetTimer = setTimeout(function() {{ swipeDeltaX = 0; }}, 220);

        if (swipeDeltaX <= -90) {{
          swipeDeltaX = 0;
          swipeCooldownUntil = now + 600;
          triggerSwipe("back");
        }} else if (swipeDeltaX >= 90) {{
          swipeDeltaX = 0;
          swipeCooldownUntil = now + 600;
          triggerSwipe("forward");
        }}
      }}, {{ passive: true }});
    }}
  }}

  // --- Dark Reader Theme Controller ---
  let currentTheme = "{initial_theme_str}";
  const darkMediaQuery = window.matchMedia ? window.matchMedia("(prefers-color-scheme: dark)") : null;

  function resolveIsDark(mode) {{
    if (mode === "dark") return true;
    if (mode === "light") return false;
    return !!(darkMediaQuery && darkMediaQuery.matches);
  }}

  function applyTheme(mode) {{
    currentTheme = mode || "system";
    const isDark = resolveIsDark(currentTheme);
    if (document.documentElement) {{
      document.documentElement.setAttribute("data-wol-theme", isDark ? "dark" : "light");
    }}
    if (typeof DarkReader === "undefined") return;
    try {{
      if (isDark) {{
        if (typeof DarkReader.setFetchMethod === "function") {{
          DarkReader.setFetchMethod(window.fetch.bind(window));
        }}
        DarkReader.enable(
          {{ brightness: 100, contrast: 95, sepia: 0 }},
          {{
            css: [
              "mark.wol-hl {{ color: #1a1a1a !important; }}",
              ".wol-hl-toolbar {{ background: #1e2228 !important; border-color: rgba(255, 255, 255, 0.14) !important; }}",
              ".wol-hl-btn-remove {{ background: #2c313a !important; color: #e6edf3 !important; border-color: rgba(255, 255, 255, 0.14) !important; }}"
            ].join("\n")
          }}
        );
      }} else {{
        DarkReader.disable();
      }}
    }} catch (e) {{
      console.warn("[WOL Theme] DarkReader error:", e);
    }}
  }}

  window.__wolApplyTheme = applyTheme;
  if (darkMediaQuery && typeof darkMediaQuery.addEventListener === "function") {{
    darkMediaQuery.addEventListener("change", function() {{
      if (currentTheme === "system") {{
        applyTheme("system");
      }}
    }});
  }}
  applyTheme(currentTheme);

  function onDomReady() {{
    applyTheme(currentTheme);
    injectStyle();
    injectNavPill();
  }}

  if (document.readyState === "loading") {{
    document.addEventListener("DOMContentLoaded", onDomReady);
  }} else {{
    onDomReady();
  }}
}})();
"##
    );

    format!(
        "{DARKREADER_JS}\n{shim}\n{TOKENIZE_JS}\n{LANGUAGES_JS}\n{HIGHLIGHT_JS}\n{CONTENT_JS}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_scripts_are_non_empty() {
        assert!(!DARKREADER_JS.is_empty());
        assert!(!TOKENIZE_JS.is_empty());
        assert!(!LANGUAGES_JS.is_empty());
        assert!(!HIGHLIGHT_JS.is_empty());
        assert!(!CONTENT_JS.is_empty());
        assert!(!CONTENT_CSS.is_empty());
    }

    #[test]
    fn build_injected_script_contains_all_components() {
        let script = build_injected_script(Some("{\"version\":1}"), None);
        assert!(script.contains("DarkReader"));
        assert!(script.contains("__wolApplyTheme"));
        assert!(script.contains("__wolBridge"));
        assert!(script.contains("wol-desktop-settings-btn"));
        assert!(script.contains("wol-desktop-settings-popover"));
        assert!(script.contains("export_marks_to_file"));
        assert!(script.contains("tokenize"));
        assert!(script.contains("highlightBlock"));
        assert!(script.contains("wol-highlights-css"));
        assert!(script.contains(
            ".article { max-width: none !important; padding: 1em 20px 5em 20px !important; }"
        ));
        assert!(script.contains("__wolCookiesShimInstalled"));
        assert!(script.contains("cookieConsent-FUNCTIONAL"));
        assert!(script.contains("wol-desktop-nav-pill"));
        assert!(script.contains("html[data-wol-theme=\"dark\"]"));
        assert!(script.contains("navStyle.classList.add(\"darkreader\")"));
    }

    #[test]
    fn build_injected_script_respects_initial_theme() {
        let dark_script = build_injected_script_with_theme(None, None, ThemeMode::Dark);
        assert!(dark_script.contains("let currentTheme = \"dark\";"));
        let light_script = build_injected_script_with_theme(None, None, ThemeMode::Light);
        assert!(light_script.contains("let currentTheme = \"light\";"));
    }
}
