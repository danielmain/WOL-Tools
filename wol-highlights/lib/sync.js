(function (globalScope) {
  "use strict";

  async function googleCredentials(ext) {
    const { wolGoogleOAuth } = await ext.storage.local.get("wolGoogleOAuth");
    const clientId = String(wolGoogleOAuth?.clientId || "").trim();
    const clientSecret = String(wolGoogleOAuth?.clientSecret || "").trim();
    if (!clientId || !clientSecret) {
      throw new Error("Configure Google OAuth credentials in extension local storage; see README.");
    }
    return { clientId, clientSecret };
  }
  const GOOGLE_AUTH_URL = "https://accounts.google.com/o/oauth2/auth";
  const GOOGLE_TOKEN_URL = "https://oauth2.googleapis.com/token";
  const GOOGLE_DRIVE_SCOPE = "https://www.googleapis.com/auth/drive.file";
  const SYNC_FILE_NAME = "wol-highlights-sync.json";
  const DEFAULT_REMOTE_PATH = "OfflineWOL/wol-highlights-sync.json";

  function nowIsoUtc() {
    return new Date().toISOString();
  }

  function unixSecondsNow() {
    return Math.floor(Date.now() / 1000);
  }

  function randomHex(byteLen) {
    const bytes = new Uint8Array(byteLen);
    globalScope.crypto.getRandomValues(bytes);
    return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  }

  function base64EncodeUtf8(str) {
    const bytes = new TextEncoder().encode(str);
    let binary = "";
    for (let i = 0; i < bytes.length; i += 1) {
      binary += String.fromCharCode(bytes[i]);
    }
    return btoa(binary);
  }

  function base64UrlEncodeBytes(bytes) {
    let binary = "";
    for (let i = 0; i < bytes.length; i += 1) {
      binary += String.fromCharCode(bytes[i]);
    }
    return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  }

  async function pkceChallenge(verifier) {
    const data = new TextEncoder().encode(verifier);
    const digest = await globalScope.crypto.subtle.digest("SHA-256", data);
    return base64UrlEncodeBytes(new Uint8Array(digest));
  }

  function normalizeNextcloudServerUrl(raw) {
    const trimmed = String(raw || "").trim();
    if (!trimmed) {
      throw new Error("Nextcloud server URL is required.");
    }
    const withScheme =
      trimmed.startsWith("http://") || trimmed.startsWith("https://")
        ? trimmed
        : `https://${trimmed}`;
    const parsed = new URL(withScheme);
    if (!parsed.hostname) {
      throw new Error("Server URL must include a hostname.");
    }
    const path = parsed.pathname.replace(/\/+$/, "");
    return `${parsed.origin}${path && path !== "/" ? path : ""}`;
  }

  function encodePathSegments(remotePath) {
    return String(remotePath || DEFAULT_REMOTE_PATH)
      .replace(/^\/+|\/+$/g, "")
      .split("/")
      .filter(Boolean)
      .map((seg) => encodeURIComponent(seg))
      .join("/");
  }

  function nextcloudWebdavRootUrl(serverUrl, username) {
    const base = serverUrl.replace(/\/+$/, "");
    return `${base}/remote.php/dav/files/${encodeURIComponent(username)}/`;
  }

  function nextcloudWebdavFileUrl(serverUrl, username, remotePath) {
    const root = nextcloudWebdavRootUrl(serverUrl, username);
    return `${root}${encodePathSegments(remotePath || DEFAULT_REMOTE_PATH)}`;
  }

  function marksHasContent(marks) {
    if (!marks || typeof marks !== "object") return false;
    if ((marks.highlightCount || 0) > 0 || (marks.noteCount || 0) > 0) return true;
    const hasKeys = (obj) => Boolean(obj && typeof obj === "object" && Object.keys(obj).length > 0);
    return (
      hasKeys(marks.docs) ||
      hasKeys(marks.bible) ||
      hasKeys(marks.notes?.docs) ||
      hasKeys(marks.notes?.bible) ||
      hasKeys(marks.inputFields?.docs) ||
      hasKeys(marks.inputFields?.bible)
    );
  }

  function decideSyncAction(localHasData, localUpdatedAt, lastSyncedAt, remoteUpdatedAt) {
    if (!remoteUpdatedAt && localHasData) return "push";
    let localDirty = false;
    if (localUpdatedAt && lastSyncedAt) {
      localDirty = localUpdatedAt > lastSyncedAt;
    } else if (!lastSyncedAt) {
      localDirty = Boolean(localHasData);
    }

    let remoteNewer = false;
    if (remoteUpdatedAt && lastSyncedAt) {
      remoteNewer = remoteUpdatedAt > lastSyncedAt;
    } else if (remoteUpdatedAt && !lastSyncedAt) {
      remoteNewer = true;
    }

    if (!localDirty && !remoteNewer) return "noop";
    if (!localDirty && remoteNewer) return "pull";
    if (localDirty && !remoteNewer) return "push";
    if (!localHasData && !lastSyncedAt) return "pull";
    return "merge";
  }

  function mergeRangeList(a, b) {
    const map = new Map();
    for (const list of [a, b]) {
      if (!Array.isArray(list)) continue;
      for (const item of list) {
        if (!Array.isArray(item) || item.length < 3) continue;
        const s = Number(item[0]);
        const e = Number(item[1]);
        const c = Number(item[2]);
        if (Number.isFinite(s) && Number.isFinite(e) && Number.isFinite(c)) {
          map.set(`${s}:${e}`, [s, e, c]);
        }
      }
    }
    return Array.from(map.values()).sort((x, y) => x[0] - y[0] || x[1] - y[1]);
  }

  function mergeNestedRanges(a, b) {
    const out = {};
    const keys = new Set([
      ...Object.keys(a && typeof a === "object" ? a : {}),
      ...Object.keys(b && typeof b === "object" ? b : {}),
    ]);
    for (const key of keys) {
      const aInner = a?.[key] && typeof a[key] === "object" ? a[key] : {};
      const bInner = b?.[key] && typeof b[key] === "object" ? b[key] : {};
      const innerKeys = new Set([...Object.keys(aInner), ...Object.keys(bInner)]);
      const mergedInner = {};
      for (const ik of innerKeys) {
        const list = mergeRangeList(aInner[ik], bInner[ik]);
        if (list.length > 0) mergedInner[ik] = list;
      }
      if (Object.keys(mergedInner).length > 0) out[key] = mergedInner;
    }
    return out;
  }

  function noteDedupKey(note) {
    if (note?.guid && String(note.guid).trim()) {
      return `guid:${note.guid}`;
    }
    return `tc:${note?.title || ""}\0${note?.content || ""}`;
  }

  function mergeNoteList(a, b) {
    const map = new Map();
    for (const src of [a, b]) {
      if (!src) continue;
      const items = Array.isArray(src) ? src : typeof src === "object" ? [src] : [];
      for (const item of items) {
        if (!item || (!item.title?.trim() && !item.content?.trim())) continue;
        const key = noteDedupKey(item);
        const existing = map.get(key);
        if (!existing || String(item.lastModified || "") >= String(existing.lastModified || "")) {
          map.set(key, item);
        }
      }
    }
    return Array.from(map.values());
  }

  function mergeNestedNotes(a, b) {
    const out = {};
    const keys = new Set([
      ...Object.keys(a && typeof a === "object" ? a : {}),
      ...Object.keys(b && typeof b === "object" ? b : {}),
    ]);
    for (const key of keys) {
      const aInner = a?.[key] && typeof a[key] === "object" ? a[key] : {};
      const bInner = b?.[key] && typeof b[key] === "object" ? b[key] : {};
      const innerKeys = new Set([...Object.keys(aInner), ...Object.keys(bInner)]);
      const mergedInner = {};
      for (const ik of innerKeys) {
        const list = mergeNoteList(aInner[ik], bInner[ik]);
        if (list.length > 0) mergedInner[ik] = list;
      }
      if (Object.keys(mergedInner).length > 0) out[key] = mergedInner;
    }
    return out;
  }

  function mergeNestedInputs(a, b) {
    const out = {};
    const keys = new Set([
      ...Object.keys(a && typeof a === "object" ? a : {}),
      ...Object.keys(b && typeof b === "object" ? b : {}),
    ]);
    for (const key of keys) {
      const mergedInner = {};
      for (const src of [a?.[key], b?.[key]]) {
        if (!src || typeof src !== "object") continue;
        for (const [k, v] of Object.entries(src)) {
          if (typeof v === "string" && v.trim()) {
            mergedInner[k] = v;
          }
        }
      }
      if (Object.keys(mergedInner).length > 0) out[key] = mergedInner;
    }
    return out;
  }

  function mergeMarks(local, remote) {
    if (!local && !remote) return null;
    if (local && !remote) return local;
    if (!local && remote) return remote;

    const docs = mergeNestedRanges(local.docs, remote.docs);
    const bible = mergeNestedRanges(local.bible, remote.bible);
    const notesDocs = mergeNestedNotes(local.notes?.docs, remote.notes?.docs);
    const notesBible = mergeNestedNotes(local.notes?.bible, remote.notes?.bible);
    const inputsDocs = mergeNestedInputs(local.inputFields?.docs, remote.inputFields?.docs);
    const inputsBible = mergeNestedInputs(local.inputFields?.bible, remote.inputFields?.bible);

    let highlightCount = 0;
    const colorCounts = {};
    for (const group of [docs, bible]) {
      for (const inner of Object.values(group)) {
        for (const ranges of Object.values(inner)) {
          for (const range of ranges) {
            highlightCount += 1;
            const c = String(range[2] ?? 1);
            colorCounts[c] = (colorCounts[c] || 0) + 1;
          }
        }
      }
    }

    let noteCount = 0;
    for (const group of [notesDocs, notesBible]) {
      for (const inner of Object.values(group)) {
        for (const list of Object.values(inner)) {
          noteCount += Array.isArray(list) ? list.length : 1;
        }
      }
    }
    for (const group of [inputsDocs, inputsBible]) {
      for (const inner of Object.values(group)) {
        noteCount += Object.keys(inner).length;
      }
    }

    return {
      version: 1,
      fileName: local.fileName || remote.fileName || "Cloud Sync",
      loadedAt: nowIsoUtc(),
      enabled: local.enabled !== undefined ? local.enabled : remote.enabled !== false,
      highlightCount,
      noteCount,
      ignored: 0,
      documentCount: Object.keys(docs).length,
      bibleChapterCount: Object.keys(bible).length,
      colorCounts,
      docs,
      bible,
      notes: { docs: notesDocs, bible: notesBible },
      inputFields: { docs: inputsDocs, bible: inputsBible },
    };
  }

  function nextcloudAuthHeader(cfg) {
    return `Basic ${base64EncodeUtf8(`${cfg.username}:${cfg.password}`)}`;
  }

  async function nextcloudVerify(cfg) {
    const rootUrl = nextcloudWebdavRootUrl(cfg.serverUrl, cfg.username);
    const resp = await fetch(rootUrl, {
      method: "PROPFIND",
      headers: {
        Authorization: nextcloudAuthHeader(cfg),
        Depth: "0",
      },
    });
    if (resp.status === 401) {
      throw new Error("Nextcloud rejected username or password (401 Unauthorized).");
    }
    if (!resp.ok && resp.status !== 207) {
      throw new Error(`Nextcloud WebDAV returned HTTP ${resp.status}.`);
    }
  }

  async function nextcloudEnsureParentDirs(cfg) {
    const parts = String(cfg.remotePath || DEFAULT_REMOTE_PATH)
      .replace(/^\/+|\/+$/g, "")
      .split("/")
      .filter(Boolean);
    if (parts.length <= 1) return;
    let current = nextcloudWebdavRootUrl(cfg.serverUrl, cfg.username);
    for (let i = 0; i < parts.length - 1; i += 1) {
      current += `${encodeURIComponent(parts[i])}/`;
      try {
        await fetch(current, {
          method: "MKCOL",
          headers: { Authorization: nextcloudAuthHeader(cfg) },
        });
      } catch (_) {
        // Ignore directory already exists
      }
    }
  }

  async function nextcloudDownload(cfg) {
    const fileUrl = nextcloudWebdavFileUrl(cfg.serverUrl, cfg.username, cfg.remotePath);
    const resp = await fetch(fileUrl, {
      method: "GET",
      headers: { Authorization: nextcloudAuthHeader(cfg) },
    });
    if (resp.status === 404) return null;
    if (!resp.ok) throw new Error(`Nextcloud download HTTP ${resp.status}`);
    return await resp.json();
  }

  async function nextcloudUpload(cfg, envelope) {
    await nextcloudEnsureParentDirs(cfg);
    const fileUrl = nextcloudWebdavFileUrl(cfg.serverUrl, cfg.username, cfg.remotePath);
    const resp = await fetch(fileUrl, {
      method: "PUT",
      headers: {
        Authorization: nextcloudAuthHeader(cfg),
        "Content-Type": "application/json",
      },
      body: JSON.stringify(envelope, null, 2),
    });
    if (!resp.ok) throw new Error(`Nextcloud upload HTTP ${resp.status}`);
  }

  async function googleEnsureToken(syncState, ext) {
    const gd = syncState.googleDrive;
    if (!gd) throw new Error("Google Drive is not connected.");
    if (unixSecondsNow() + 60 < (gd.expiresAt || 0)) {
      return gd;
    }
    const { clientId, clientSecret } = await googleCredentials(ext);
    const body = new URLSearchParams({
      client_id: clientId,
      client_secret: clientSecret,
      refresh_token: gd.refreshToken,
      grant_type: "refresh_token",
    });
    const resp = await fetch(GOOGLE_TOKEN_URL, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: body.toString(),
    });
    if (!resp.ok) {
      const txt = await resp.text().catch(() => "");
      throw new Error(`Google token refresh error: ${txt || resp.status}`);
    }
    const refreshed = await resp.json();
    gd.accessToken = refreshed.access_token;
    gd.expiresAt = unixSecondsNow() + (refreshed.expires_in || 3600);
    return gd;
  }

  async function googleFindFileId(accessToken) {
    const url = new URL("https://www.googleapis.com/drive/v3/files");
    url.searchParams.set("q", `name='${SYNC_FILE_NAME}' and trashed=false`);
    url.searchParams.set("spaces", "drive");
    url.searchParams.set("fields", "files(id,name,modifiedTime)");
    url.searchParams.set("orderBy", "modifiedTime desc");
    const resp = await fetch(url.toString(), {
      headers: { Authorization: `Bearer ${accessToken}` },
    });
    if (!resp.ok) throw new Error(`Google Drive list HTTP ${resp.status}`);
    const data = await resp.json();
    return data.files?.[0]?.id || null;
  }

  async function googleDownload(gd) {
    let fileId = gd.fileId || (await googleFindFileId(gd.accessToken));
    if (!fileId) return null;
    gd.fileId = fileId;

    let resp = await fetch(
      `https://www.googleapis.com/drive/v3/files/${encodeURIComponent(fileId)}?alt=media`,
      { headers: { Authorization: `Bearer ${gd.accessToken}` } }
    );
    if (resp.status === 404) {
      gd.fileId = null;
      fileId = await googleFindFileId(gd.accessToken);
      if (!fileId) return null;
      gd.fileId = fileId;
      resp = await fetch(
        `https://www.googleapis.com/drive/v3/files/${encodeURIComponent(fileId)}?alt=media`,
        { headers: { Authorization: `Bearer ${gd.accessToken}` } }
      );
    }
    if (!resp.ok) throw new Error(`Google Drive download HTTP ${resp.status}`);
    return await resp.json();
  }

  async function googleUpload(gd, envelope) {
    const jsonBody = JSON.stringify(envelope, null, 2);
    if (gd.fileId) {
      const resp = await fetch(
        `https://www.googleapis.com/upload/drive/v3/files/${encodeURIComponent(gd.fileId)}?uploadType=media`,
        {
          method: "PATCH",
          headers: {
            Authorization: `Bearer ${gd.accessToken}`,
            "Content-Type": "application/json",
          },
          body: jsonBody,
        }
      );
      if (!resp.ok) throw new Error(`Google Drive update HTTP ${resp.status}`);
      return;
    }

    const boundary = `wol_sync_${randomHex(12)}`;
    const meta = JSON.stringify({ name: SYNC_FILE_NAME, mimeType: "application/json" });
    const multipartBody =
      `--${boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n${meta}\r\n` +
      `--${boundary}\r\nContent-Type: application/json\r\n\r\n${jsonBody}\r\n--${boundary}--`;

    const resp = await fetch(
      "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&fields=id",
      {
        method: "POST",
        headers: {
          Authorization: `Bearer ${gd.accessToken}`,
          "Content-Type": `multipart/related; boundary=${boundary}`,
        },
        body: multipartBody,
      }
    );
    if (!resp.ok) throw new Error(`Google Drive create HTTP ${resp.status}`);
    const created = await resp.json().catch(() => null);
    if (created?.id) gd.fileId = created.id;
  }

  async function fetchGoogleEmail(accessToken) {
    try {
      const resp = await fetch(
        "https://www.googleapis.com/drive/v3/about?fields=user(emailAddress)",
        { headers: { Authorization: `Bearer ${accessToken}` } }
      );
      if (!resp.ok) return null;
      const data = await resp.json();
      return data?.user?.emailAddress || null;
    } catch (_) {
      return null;
    }
  }

  function buildStatus(syncState, changedLocal = false, syncing = false) {
    if (!syncState || syncState.provider === "none" || !syncState.provider) {
      return {
        provider: "none",
        connected: false,
        accountLabel: null,
        serverUrl: null,
        remotePath: null,
        lastSyncedAt: syncState?.lastSyncedAt || null,
        localUpdatedAt: syncState?.localUpdatedAt || null,
        lastError: syncState?.lastError || null,
        syncing,
        changedLocal,
      };
    }
    if (syncState.provider === "nextcloud") {
      return {
        provider: "nextcloud",
        connected: Boolean(syncState.nextcloud),
        accountLabel: syncState.nextcloud?.username || null,
        serverUrl: syncState.nextcloud?.serverUrl || null,
        remotePath: syncState.nextcloud?.remotePath || DEFAULT_REMOTE_PATH,
        lastSyncedAt: syncState.lastSyncedAt || null,
        localUpdatedAt: syncState.localUpdatedAt || null,
        lastError: syncState.lastError || null,
        syncing,
        changedLocal,
      };
    }
    return {
      provider: "googledrive",
      connected: Boolean(syncState.googleDrive),
      accountLabel: syncState.googleDrive?.email || "Google Drive",
      serverUrl: null,
      remotePath: SYNC_FILE_NAME,
      lastSyncedAt: syncState.lastSyncedAt || null,
      localUpdatedAt: syncState.localUpdatedAt || null,
      lastError: syncState.lastError || null,
      syncing,
      changedLocal,
    };
  }

  async function getOrInitSyncState(ext) {
    const stored = await ext.storage.local.get("wolSync");
    if (stored?.wolSync && stored.wolSync.deviceId) {
      return stored.wolSync;
    }
    const init = {
      provider: "none",
      deviceId: randomHex(16),
      localUpdatedAt: null,
      lastSyncedAt: null,
      lastError: null,
      nextcloud: null,
      googleDrive: null,
    };
    await ext.storage.local.set({ wolSync: init });
    return init;
  }

  let activeSyncPromise = null;

  async function syncNow(ext) {
    if (activeSyncPromise) return activeSyncPromise;
    activeSyncPromise = (async () => {
      const syncState = await getOrInitSyncState(ext);
      try {
        if (!syncState.provider || syncState.provider === "none") {
          console.info("[WOL Highlights Sync] Skipped: no provider configured");
          return buildStatus(syncState, false, false);
        }
        console.info("[WOL Highlights Sync] Starting", syncState.provider);
        const storedMarks = await ext.storage.local.get("wolMarks");
        const localMarks = storedMarks?.wolMarks || null;
        const localHasData = marksHasContent(localMarks);

        let remote = null;
        let uploadFn = null;

        if (syncState.provider === "nextcloud") {
          const cfg = syncState.nextcloud;
          if (!cfg) throw new Error("Nextcloud is not configured.");
          remote = await nextcloudDownload(cfg);
          uploadFn = (env) => nextcloudUpload(cfg, env);
        } else if (syncState.provider === "googledrive") {
          const gd = await googleEnsureToken(syncState, ext);
          remote = await googleDownload(gd);
          uploadFn = (env) => googleUpload(gd, env);
        }

        const action = decideSyncAction(
          localHasData,
          syncState.localUpdatedAt,
          syncState.lastSyncedAt,
          remote?.updatedAt || null
        );

        console.info("[WOL Highlights Sync] Decision", { action, localHasData, remotePresent: !!remote });
        let changedLocal = false;

        if (action === "noop") {
          syncState.lastSyncedAt = nowIsoUtc();
          syncState.lastError = null;
          await ext.storage.local.set({ wolSync: syncState });
        } else if (action === "pull" && remote) {
          syncState.localUpdatedAt = remote.updatedAt;
          syncState.lastSyncedAt = remote.updatedAt;
          syncState.lastError = null;
          syncState._internalWriteAt = Date.now();
          await ext.storage.local.set({
            wolSync: syncState,
            wolMarks: remote.wolMarks ?? null,
          });
          changedLocal = true;
        } else if (action === "push") {
          const now = nowIsoUtc();
          await uploadFn({
            format: "wol-highlights-sync",
            version: 1,
            updatedAt: now,
            deviceId: syncState.deviceId,
            wolMarks: localMarks,
          });
          syncState.localUpdatedAt = now;
          syncState.lastSyncedAt = now;
          syncState.lastError = null;
          await ext.storage.local.set({ wolSync: syncState });
        } else if (action === "merge") {
          const merged = mergeMarks(localMarks, remote?.wolMarks || null);
          const now = nowIsoUtc();
          await uploadFn({
            format: "wol-highlights-sync",
            version: 1,
            updatedAt: now,
            deviceId: syncState.deviceId,
            wolMarks: merged,
          });
          syncState.localUpdatedAt = now;
          syncState.lastSyncedAt = now;
          syncState.lastError = null;
          syncState._internalWriteAt = Date.now();
          await ext.storage.local.set({
            wolSync: syncState,
            wolMarks: merged,
          });
          changedLocal = true;
        }

        console.info("[WOL Highlights Sync] Finished", { changedLocal });
        return buildStatus(syncState, changedLocal, false);
      } catch (err) {
        syncState.lastError = err?.message || String(err);
        await ext.storage.local.set({ wolSync: syncState });
        throw err;
      } finally {
        activeSyncPromise = null;
      }
    })();
    return activeSyncPromise;
  }

  async function connectGoogleDrive(ext) {
    const { clientId, clientSecret } = await googleCredentials(ext);
    const redirectUri = "http://localhost";
    const verifier = randomHex(32);
    const challenge = await pkceChallenge(verifier);
    const stateToken = randomHex(16);

    const authUrl = new URL(GOOGLE_AUTH_URL);
    authUrl.searchParams.set("client_id", clientId);
    authUrl.searchParams.set("redirect_uri", redirectUri);
    authUrl.searchParams.set("response_type", "code");
    authUrl.searchParams.set("scope", GOOGLE_DRIVE_SCOPE);
    authUrl.searchParams.set("access_type", "offline");
    authUrl.searchParams.set("prompt", "consent");
    authUrl.searchParams.set("code_challenge", challenge);
    authUrl.searchParams.set("code_challenge_method", "S256");
    authUrl.searchParams.set("state", stateToken);

    const code = await new Promise((resolve, reject) => {
      let authTabId = null;
      let done = false;

      const cleanup = () => {
        ext.tabs.onUpdated.removeListener(onUpdated);
        ext.tabs.onRemoved.removeListener(onRemoved);
      };

      const timeout = setTimeout(() => {
        if (done) return;
        done = true;
        cleanup();
        reject(new Error("Google sign-in timed out."));
      }, 180000);

      function onRemoved(tabId) {
        if (tabId === authTabId && !done) {
          done = true;
          clearTimeout(timeout);
          cleanup();
          reject(new Error("Google sign-in tab was closed."));
        }
      }

      function onUpdated(tabId, changeInfo, tab) {
        if (tabId !== authTabId || done) return;
        const urlStr = changeInfo.url || tab?.url || "";
        if (!urlStr.startsWith("http://localhost")) return;

        done = true;
        clearTimeout(timeout);
        cleanup();
        ext.tabs.remove(tabId).catch(() => {});

        try {
          const parsed = new URL(urlStr);
          const err = parsed.searchParams.get("error");
          if (err) {
            reject(new Error(`Google sign-in failed: ${err}`));
            return;
          }
          if (parsed.searchParams.get("state") !== stateToken) {
            reject(new Error("OAuth state mismatch."));
            return;
          }
          const authCode = parsed.searchParams.get("code");
          if (!authCode) {
            reject(new Error("Missing OAuth code."));
            return;
          }
          resolve(authCode);
        } catch (e) {
          reject(e);
        }
      }

      ext.tabs.onUpdated.addListener(onUpdated);
      ext.tabs.onRemoved.addListener(onRemoved);
      ext.tabs
        .create({ url: authUrl.toString() })
        .then((tab) => {
          authTabId = tab.id;
        })
        .catch((err) => {
          if (!done) {
            done = true;
            clearTimeout(timeout);
            cleanup();
            reject(err);
          }
        });
    });

    const tokenBody = new URLSearchParams({
      code,
      client_id: clientId,
      client_secret: clientSecret,
      redirect_uri: redirectUri,
      grant_type: "authorization_code",
      code_verifier: verifier,
    });
    const tokenResp = await fetch(GOOGLE_TOKEN_URL, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: tokenBody.toString(),
    });
    if (!tokenResp.ok) {
      const txt = await tokenResp.text().catch(() => "");
      throw new Error(`Google token exchange failed: ${txt || tokenResp.status}`);
    }
    const tokens = await tokenResp.json();
    if (!tokens.refresh_token) {
      throw new Error("Google did not return a refresh token.");
    }
    const email = await fetchGoogleEmail(tokens.access_token);

    const syncState = await getOrInitSyncState(ext);
    syncState.provider = "googledrive";
    syncState.googleDrive = {
      accessToken: tokens.access_token,
      refreshToken: tokens.refresh_token,
      expiresAt: unixSecondsNow() + (tokens.expires_in || 3600),
      fileId: null,
      email,
    };
    syncState.nextcloud = null;
    syncState.lastSyncedAt = null;
    syncState.lastError = null;
    await ext.storage.local.set({ wolSync: syncState });

    return await syncNow(ext);
  }

  async function connectNextcloud(ext, { serverUrl, username, password, remotePath }) {
    const normalized = normalizeNextcloudServerUrl(serverUrl);
    const cleanUser = String(username || "").trim();
    if (!cleanUser || !password) {
      throw new Error("Username and password/app password are required.");
    }
    const cleanPath =
      String(remotePath || "")
        .trim()
        .replace(/^\/+/, "") || DEFAULT_REMOTE_PATH;

    const cfg = {
      serverUrl: normalized,
      username: cleanUser,
      password,
      remotePath: cleanPath,
    };
    await nextcloudVerify(cfg);

    const syncState = await getOrInitSyncState(ext);
    syncState.provider = "nextcloud";
    syncState.nextcloud = cfg;
    syncState.googleDrive = null;
    syncState.lastSyncedAt = null;
    syncState.lastError = null;
    await ext.storage.local.set({ wolSync: syncState });

    return await syncNow(ext);
  }

  async function connectNextcloudBrowser(ext, { serverUrl, remotePath }) {
    const normalized = normalizeNextcloudServerUrl(serverUrl);
    const cleanPath =
      String(remotePath || "")
        .trim()
        .replace(/^\/+/, "") || DEFAULT_REMOTE_PATH;

    const initResp = await fetch(`${normalized}/index.php/login/v2`, { method: "POST" });
    if (!initResp.ok) {
      throw new Error(
        `Nextcloud browser login returned HTTP ${initResp.status}. Try entering an App Password manually.`
      );
    }
    const flow = await initResp.json();
    const tab = await ext.tabs.create({ url: flow.login });

    for (let i = 0; i < 120; i += 1) {
      await new Promise((r) => setTimeout(r, 1500));
      const pollResp = await fetch(flow.poll.endpoint, {
        method: "POST",
        headers: { "Content-Type": "application/x-www-form-urlencoded" },
        body: new URLSearchParams({ token: flow.poll.token }).toString(),
      }).catch(() => null);

      if (pollResp && pollResp.status === 200) {
        const creds = await pollResp.json();
        if (tab?.id) ext.tabs.remove(tab.id).catch(() => {});
        return await connectNextcloud(ext, {
          serverUrl: creds.server,
          username: creds.loginName,
          password: creds.appPassword,
          remotePath: cleanPath,
        });
      }
    }
    throw new Error("Timed out waiting for Nextcloud browser sign-in.");
  }

  async function disconnectSync(ext) {
    const syncState = await getOrInitSyncState(ext);
    syncState.provider = "none";
    syncState.nextcloud = null;
    syncState.googleDrive = null;
    syncState.lastSyncedAt = null;
    syncState.lastError = null;
    await ext.storage.local.set({ wolSync: syncState });
    return buildStatus(syncState, false, false);
  }

  const api = {
    DEFAULT_REMOTE_PATH,
    normalizeNextcloudServerUrl,
    nextcloudWebdavFileUrl,
    decideSyncAction,
    mergeMarks,
    marksHasContent,
    buildStatus,
    getOrInitSyncState,
    syncNow,
    connectGoogleDrive,
    connectNextcloud,
    connectNextcloudBrowser,
    disconnectSync,
  };

  globalScope.WOLSync = api;
  if (typeof module !== "undefined" && module.exports) {
    module.exports = api;
  }
})(typeof globalThis !== "undefined" ? globalThis : self);
