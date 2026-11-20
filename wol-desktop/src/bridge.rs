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

  function isWolPage() {{
    const proto = window.location.protocol;
    const host = window.location.hostname;
    if (host === "tauri.localhost" || host.endsWith(".tauri.localhost")) return false;
    return proto === "wol:" || proto === "http:" || proto === "https:";
  }}

  function injectStyle() {{
    if (!isWolPage()) return;
    if (document.getElementById("wol-highlights-css")) return;
    const style = document.createElement("style");
    style.id = "wol-highlights-css";
    style.textContent = {css_json} + "\n.lnc-firstRunPopup {{ display: none !important; }}\n.article {{ max-width: none !important; padding: 1em 20px 5em 20px !important; }}\na[aria-label=\"home\"], header a[href*=\"/wol/h/\"]:has(> span.icon), #regionFooter, .regionFooter, #bibleChapterView {{ display: none !important; }}\nhtml, body {{ min-height: 100vh !important; min-height: 100dvh !important; width: 100% !important; max-width: none !important; margin: 0 !important; padding: 0 !important; }}\nbody {{ display: flex !important; flex-direction: column !important; }}\n#regionMain, main {{ flex: 1 0 auto !important; display: flex !important; flex-direction: column !important; width: 100% !important; min-width: 0 !important; max-width: none !important; margin-left: 0 !important; margin-right: 0 !important; padding-left: 0 !important; padding-right: 0 !important; box-sizing: border-box !important; }}\n#content {{ flex: 1 0 auto !important; display: flex !important; flex-direction: column !important; width: auto !important; min-width: 0 !important; max-width: none !important; margin-left: 0 !important; margin-right: 0 !important; padding-left: 0 !important; padding-right: 0 !important; box-sizing: border-box !important; }}\n.articlePositioner {{ min-width: 0 !important; }}\narticle, #article, .article {{ flex: 1 0 auto !important; width: auto !important; min-width: 0 !important; max-width: none !important; box-sizing: border-box !important; margin-left: 0 !important; margin-right: 0 !important; margin-bottom: 0 !important; }}\n#todayNav, #todayNav nav, #todayNav .resultNavControls {{ width: 100% !important; max-width: none !important; margin-left: 0 !important; margin-right: 0 !important; box-sizing: border-box !important; }}\n#todayNav ul {{ display: flex !important; width: 100% !important; max-width: none !important; justify-content: space-between !important; box-sizing: border-box !important; }}\n#todayNav ul > li.todayNav {{ flex: 1 1 auto !important; text-align: center !important; }}\n";
    (document.head || document.documentElement).appendChild(style);
  }}

  function ensureNavStyle() {{
    if (window.top !== window.self || !isWolPage()) return;
    if (document.getElementById("wol-desktop-nav-css")) return;
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
        }}
        @media (pointer: coarse) {{
          #wol-desktop-nav-pill {{
            top: calc(8px + env(safe-area-inset-top, 0px));
            left: calc(12px + env(safe-area-inset-left, 0px));
            opacity: 0.94;
            padding: 4px;
            gap: 4px;
          }}
          #wol-desktop-nav-pill button {{
            width: 38px;
            height: 38px;
          }}
        }}
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
      `;
    const target = document.head || document.documentElement;
    if (target) target.appendChild(navStyle);
  }}

  function injectNavPill() {{
    if (window.top !== window.self || !isWolPage()) return;
    ensureNavStyle();
    if (document.getElementById("wol-desktop-nav-pill")) return;

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

    // Other sites only need history controls; the remaining controls are for WOL.
    if (window.location.protocol !== "wol:") {{
      if (document.body) document.body.appendChild(pill);
      return;
    }}

    const settingsSep = document.createElement("span");
    settingsSep.className = "wol-nav-sep";
    settingsSep.setAttribute("aria-hidden", "true");

    const settingsBtn = document.createElement("button");
    settingsBtn.id = "wol-desktop-settings-btn";
    settingsBtn.type = "button";
    settingsBtn.title = "Settings & Highlights";
    settingsBtn.setAttribute("aria-label", "Settings & Highlights");
    settingsBtn.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>';

    pill.appendChild(settingsSep);
    pill.appendChild(settingsBtn);

    settingsBtn.addEventListener("click", function() {{
      if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {{
        window.__TAURI_INTERNALS__.invoke("open_manager_window", {{}}).catch(function(err) {{
          console.warn("[__wolBridge] open_manager_window failed:", err);
        }});
      }}
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

    if (document.body) {{
      document.body.appendChild(pill);
    }}

  }}

  // --- Dark Reader Theme Controller ---
  let currentTheme = "{initial_theme_str}";
  try {{
    const cachedTheme = isWolPage() && window.localStorage && window.localStorage.getItem("__wol_theme");
    if (cachedTheme === "dark" || cachedTheme === "light" || cachedTheme === "system") {{
      currentTheme = cachedTheme;
    }}
  }} catch (e) {{}}
  const darkMediaQuery = window.matchMedia ? window.matchMedia("(prefers-color-scheme: dark)") : null;

  function ensureDarkPreload(enable) {{
    const existing = document.getElementById("wol-dark-preload");
    if (!enable) {{
      if (existing) existing.remove();
      return;
    }}
    if (existing) return;
    const target = document.documentElement || document.head;
    if (!target) return;
    const style = document.createElement("style");
    style.id = "wol-dark-preload";
    style.textContent = [
      "html[data-wol-theme=\"dark\"] {{ background-color: #181a1b !important; color-scheme: dark !important; }}",
      "html[data-wol-theme=\"dark\"] body, html[data-wol-theme=\"dark\"] #main, html[data-wol-theme=\"dark\"] #content, html[data-wol-theme=\"dark\"] #article {{ background-color: #181a1b !important; color: #e8e6e3 !important; }}"
    ].join("\n");
    target.appendChild(style);
  }}

  function resolveIsDark(mode) {{
    if (mode === "dark") return true;
    if (mode === "light") return false;
    return !!(darkMediaQuery && darkMediaQuery.matches);
  }}

  function applyTheme(mode) {{
    currentTheme = mode || "system";
    if (!isWolPage()) {{
      ensureDarkPreload(false);
      if (document.documentElement) {{
        document.documentElement.removeAttribute("data-wol-theme");
      }}
      if (typeof DarkReader !== "undefined") {{
        try {{
          DarkReader.disable();
        }} catch (e) {{
          console.warn("[WOL Theme] DarkReader error:", e);
        }}
      }}
      return;
    }}
    try {{
      if (window.localStorage) {{
        window.localStorage.setItem("__wol_theme", currentTheme);
      }}
    }} catch (e) {{}}
    const isDark = resolveIsDark(currentTheme);
    if (document.documentElement) {{
      document.documentElement.setAttribute("data-wol-theme", isDark ? "dark" : "light");
    }}
    ensureDarkPreload(isDark);
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
              "html[data-wol-theme=\"dark\"] mark.wol-hl {{ color: #1a1a1a !important; }}",
              "html[data-wol-theme=\"dark\"] mark.wol-hl a {{ color: #1a4b84 !important; }}",
              "html[data-wol-theme=\"dark\"] mark.wol-hl.wol-c1 {{ background-color: #ffe36b !important; }}",
              "html[data-wol-theme=\"dark\"] mark.wol-hl.wol-c2 {{ background-color: #b6e89a !important; }}",
              "html[data-wol-theme=\"dark\"] mark.wol-hl.wol-c3 {{ background-color: #a9d4fb !important; }}",
              "html[data-wol-theme=\"dark\"] mark.wol-hl.wol-c4 {{ background-color: #f7b4c4 !important; }}",
              "html[data-wol-theme=\"dark\"] mark.wol-hl.wol-c5 {{ background-color: #ffc48a !important; }}",
              "html[data-wol-theme=\"dark\"] mark.wol-hl.wol-c6 {{ background-color: #d8c0f8 !important; }}",
              "html[data-wol-theme=\"dark\"] mark.wol-hl.wol-c0 {{ background-color: #e6e6e6 !important; }}",
              "html[data-wol-theme=\"dark\"] .wol-hl-toolbar .wol-hl-btn-1 {{ background-color: #ffe36b !important; }}",
              "html[data-wol-theme=\"dark\"] .wol-hl-toolbar .wol-hl-btn-2 {{ background-color: #b6e89a !important; }}",
              "html[data-wol-theme=\"dark\"] .wol-hl-toolbar .wol-hl-btn-3 {{ background-color: #a9d4fb !important; }}",
              "html[data-wol-theme=\"dark\"] .wol-hl-toolbar .wol-hl-btn-4 {{ background-color: #f7b4c4 !important; }}",
              "html[data-wol-theme=\"dark\"] .wol-hl-toolbar .wol-hl-btn-5 {{ background-color: #ffc48a !important; }}",
              "html[data-wol-theme=\"dark\"] .wol-hl-toolbar .wol-hl-btn-6 {{ background-color: #d8c0f8 !important; }}",
              ".wol-hl-toolbar {{ background: #1e2228 !important; border-color: rgba(255, 255, 255, 0.14) !important; }}",
              ".wol-hl-btn-remove {{ background: #2c313a !important; color: #e6edf3 !important; border-color: rgba(255, 255, 255, 0.14) !important; }}"
            ].join("\n")
          }}
        );
        requestAnimationFrame(function() {{
          if (document.querySelector(".darkreader--override, .darkreader--fallback")) {{
            ensureDarkPreload(false);
          }}
        }});
      }} else {{
        DarkReader.disable();
        ensureNavStyle();
      }}
    }} catch (e) {{
      console.warn("[WOL Theme] DarkReader error:", e);
    }}
  }}

  window.__wolApplyTheme = applyTheme;
  function syncThemeFromSettings() {{
    if (!isWolPage()) return;
    if (!(window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke)) return;
    window.__TAURI_INTERNALS__.invoke("get_settings").then(function(s) {{
      if (s && s.theme) {{
        applyTheme(s.theme);
      }}
    }}).catch(function() {{}});
  }}
  if (darkMediaQuery && typeof darkMediaQuery.addEventListener === "function") {{
    darkMediaQuery.addEventListener("change", function() {{
      if (currentTheme === "system") {{
        applyTheme("system");
      }}
    }});
  }}
  applyTheme(currentTheme);
  syncThemeFromSettings();
  window.addEventListener("pageshow", syncThemeFromSettings);

  function applyFontScale(scaleRaw) {{
    if (!isWolPage()) return;
    const scale = parseFloat(scaleRaw);
    const id = "__wol_desktop_font_scale";
    let el = document.getElementById(id);
    if (!Number.isFinite(scale) || scale <= 0 || Math.abs(scale - 1) < 0.001) {{
      if (el) el.remove();
      return;
    }}
    const pct = Math.round(scale * 10000) / 100;
    const cssText = ".scalableui {{ font-size: " + pct + "% !important; }}";
    if (!el) {{
      el = document.createElement("style");
      el.id = id;
      (document.head || document.documentElement).appendChild(el);
    }}
    if (el.textContent !== cssText) {{
      el.textContent = cssText;
    }}
  }}

  let lastSyncedRb = (cookieStore.rb && cookieStore.rb.value) || null;
  let lastSyncedAudioPlayer = (cookieStore.audioPlayer && cookieStore.audioPlayer.value) || null;

  function syncPreferencesFromCookies(evt) {{
    if (!isWolPage()) return;
    if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {{
      window.__TAURI_INTERNALS__.invoke("get_cookies").then(function(cookies) {{
        if (cookies && typeof cookies === "object") {{
          const rbNow = cookies.rb || null;
          const audioNow = cookies.audioPlayer || null;
          const needsReload = Boolean(
            evt && evt.persisted && (rbNow !== lastSyncedRb || audioNow !== lastSyncedAudioPlayer)
          );
          lastSyncedRb = rbNow;
          lastSyncedAudioPlayer = audioNow;
          if (typeof mergeCookieObj === "function") mergeCookieObj(cookies);
          if (typeof persistCookies === "function") persistCookies();
          applyFontScale(cookies.fontScale);
          if (needsReload) {{
            window.location.reload();
          }}
        }}
      }}).catch(function() {{}});
    }}
  }}
  window.addEventListener("pageshow", syncPreferencesFromCookies);

  function interceptWolSettingsLinks() {{
    if (!isWolPage()) return;
    document.addEventListener("click", function(e) {{
      const anchor = e.target && e.target.closest
        ? e.target.closest('a[href*="/wol/pref/"]')
        : null;
      if (!anchor) return;
      if (!(window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke)) return;
      e.preventDefault();
      e.stopPropagation();
      window.__TAURI_INTERNALS__.invoke("open_manager_window", {{ tab: "settings" }}).catch(function(err) {{
        console.warn("[__wolBridge] open_manager_window(settings) failed:", err);
      }});
    }}, true);
  }}

  function redirectHomeToPublications() {{
    if (window.top !== window.self || window.location.protocol !== "wol:") return false;
    const path = window.location.pathname || "";
    if (!/^\/?(?:[^/]+\/)?wol\/h\/r\d+\/lp-[^/]+\/?$/.test(path)) return false;
    const pubLink = document.querySelector('#menuPublications a[href*="/wol/library/"]');
    if (!pubLink) return false;
    try {{
      const target = new URL(pubLink.getAttribute("href"), window.location.href);
      if (target.href !== window.location.href) {{
        window.location.replace(target.href);
        return true;
      }}
    }} catch (_) {{}}
    return false;
  }}

  function unwrapElement(el) {{
    if (!el || !el.parentNode) return;
    const frag = document.createDocumentFragment();
    while (el.firstChild) {{
      frag.appendChild(el.firstChild);
    }}
    el.parentNode.replaceChild(frag, el);
  }}

  function isBinavPage() {{
    const path = window.location.pathname || "";
    return /(?:^|\/)wol\/binav\//.test(path);
  }}

  function cleanupWolLayout() {{
    if (!isWolPage()) return;
    document.querySelectorAll("#regionFooter, .regionFooter").forEach(function(el) {{
      el.remove();
    }});
    if (isBinavPage()) {{
      document.querySelectorAll("#bibleChapterView").forEach(function(el) {{
        el.remove();
      }});
    }}
    unwrapElement(document.getElementById("wrapper"));
  }}

  function onDomReady() {{
    if (redirectHomeToPublications()) return;
    applyTheme(currentTheme);
    syncThemeFromSettings();
    syncPreferencesFromCookies();
    injectStyle();
    cleanupWolLayout();
    injectNavPill();
    interceptWolSettingsLinks();
  }}

  window.addEventListener("pageshow", cleanupWolLayout);
  window.addEventListener("popstate", cleanupWolLayout);

  if (document.readyState === "loading") {{
    document.addEventListener("DOMContentLoaded", onDomReady);
  }} else {{
    onDomReady();
  }}
}})();
"##
    );

    format!(
        "{DARKREADER_JS}\n{shim}\n{}\n{TOKENIZE_JS}\n{LANGUAGES_JS}\n{HIGHLIGHT_JS}\n{CONTENT_JS}\n", include_str!("../ui/imported-library.js")
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
        assert!(script.contains("open_manager_window"));
        assert!(script.contains("tokenize"));
        assert!(script.contains("highlightBlock"));
        assert!(script.contains("wol-highlights-css"));
        assert!(script.contains(
            ".article { max-width: none !important; padding: 1em 20px 5em 20px !important; }"
        ));
        assert!(script.contains(r#"a[aria-label=\"home\"]"#));
        assert!(script.contains("#regionFooter, .regionFooter"));
        assert!(script.contains("#bibleChapterView"));
        assert!(script.contains("#content { flex: 1 0 auto !important; display: flex !important; flex-direction: column !important; width: auto !important;"));
        assert!(script.contains(".articlePositioner { min-width: 0 !important; }"));
        assert!(script.contains("isBinavPage"));
        assert!(script.contains("cleanupWolLayout"));
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
        assert!(light_script.contains("isWolPage()"));
        assert!(light_script.contains("tauri.localhost"));
    }

    #[test]
    fn dark_mode_preserves_highlight_colors() {
        let script = build_injected_script_with_theme(None, None, ThemeMode::Dark);
        for (idx, hex) in [
            ("1", "#ffe36b"),
            ("2", "#b6e89a"),
            ("3", "#a9d4fb"),
            ("4", "#f7b4c4"),
            ("5", "#ffc48a"),
            ("6", "#d8c0f8"),
        ] {
            let mark_rule = format!(
                r#"html[data-wol-theme=\"dark\"] mark.wol-hl.wol-c{idx} {{ background-color: {hex} !important; }}"#
            );
            let btn_rule = format!(
                r#"html[data-wol-theme=\"dark\"] .wol-hl-toolbar .wol-hl-btn-{idx} {{ background-color: {hex} !important; }}"#
            );
            assert!(script.contains(&mark_rule), "missing {mark_rule}");
            assert!(script.contains(&btn_rule), "missing {btn_rule}");
        }
        assert!(script.contains(
            r#"html[data-wol-theme=\"dark\"] mark.wol-hl.wol-c0 { background-color: #e6e6e6 !important; }"#
        ));
        assert!(script.contains(
            r#"html[data-wol-theme=\"dark\"] mark.wol-hl { color: #1a1a1a !important; }"#
        ));
        assert!(script.contains(
            r#"html[data-wol-theme=\"dark\"] mark.wol-hl a { color: #1a4b84 !important; }"#
        ));
    }
}
