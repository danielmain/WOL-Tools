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
    if (window.top !== window.self) return;
    if (!["wol:", "http:", "https:"].includes(window.location.protocol)) return;
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
