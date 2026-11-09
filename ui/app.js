// The control page (see docs/ui.md).
const $ = (id) => document.getElementById(id);
const desktopApp = Boolean(window.torrstreamDesktop);
document.documentElement.classList.toggle("desktop", desktopApp);

let current = null,
  lastHash,
  filesRun = 0,
  loading = false,
  resolving = false,
  statusBusy = false,
  playing = null,
  dismissedError = "";
let networkBusy = false,
  networkReadBusy = false,
  networkRevision = 0,
  networkState = null,
  networkKey = "";
let dialog = null,
  returnFocus = null;
let peerRun = 0,
  peerTimer = null,
  peerAbort = null,
  licenseRun = 0,
  toastTimer = null;
const peerRates = new PeerRates();

// Static, trusted icon markup. File names and server text always use textContent.
const ICONS = {
  play: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5.5v13l10.5-6.5z"/></svg>',
  copy: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M15 9V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h3"/></svg>',
  video:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 5v14M17 5v14M3 9.5h4M17 9.5h4M3 14.5h4M17 14.5h4"/></svg>',
  audio:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 18V6l10-2v12"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="16.5" cy="16" r="2.5"/></svg>',
  subtitles:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 12h3M13 12h4M7 15.5h6M15 15.5h2"/></svg>',
  other:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/></svg>',
};
const KINDS = [
  ["video", /\.(mkv|mp4|m4v|avi|mov|webm|ts|m2ts|wmv|mpg|mpeg)$/i],
  ["audio", /\.(mp3|flac|m4a|aac|ogg|opus|wav)$/i],
  ["subtitles", /\.(srt|ass|ssa|vtt|sub|idx)$/i],
];
const kindOf = (path) => KINDS.find(([, re]) => re.test(path))?.[0] || "other";

function mib(n) {
  return (n / 1048576).toFixed(1);
}
function size(n) {
  if (n >= 1073741824) return (n / 1073741824).toFixed(2) + " GiB";
  if (n >= 1048576) return mib(n) + " MiB";
  if (n >= 1024) return (n / 1024).toFixed(1) + " KiB";
  return n + " B";
}
function mbps(bytes) {
  return (((bytes || 0) * 8) / 1000000).toFixed(1);
}
function message(id, text) {
  $(id).textContent = text;
  $(id).hidden = !text;
}
function banner(text) {
  message("banner-text", text);
  $("banner").hidden = !text;
}
function toast(text) {
  $("toast").textContent = text;
  $("toast").hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => ($("toast").hidden = true), 2200);
}
async function api(
  path,
  { timeout = 10000, signal, ...options } = {},
  plain = false,
) {
  const controller = new AbortController();
  const abort = () => controller.abort();
  if (signal?.aborted) abort();
  signal?.addEventListener("abort", abort, { once: true });
  const timer = setTimeout(abort, timeout);
  try {
    const response = await fetch(path, {
      ...options,
      signal: controller.signal,
      cache: "no-store",
    });
    const text = await response.text();
    let value = text;
    if (!plain) {
      try {
        value = text ? JSON.parse(text) : null;
      } catch {
        // Plain-text errors (for example a refused browser origin) stay readable.
        if (response.ok) throw Error("The server sent an unexpected reply.");
      }
    }
    if (!response.ok) throw Error(value?.error || text || response.statusText);
    return value;
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener("abort", abort);
  }
}
function desktop(value) {
  window.webkit?.messageHandlers?.desktop?.postMessage(value);
}

// Adding and stopping
function setLoading(value) {
  loading = value;
  showAddState();
}
function showAddState() {
  const busy = loading || resolving;
  $("link").disabled = busy;
  $("add-button").disabled = busy;
  $("file-button").disabled = busy;
  $("progress").hidden = !busy;
  if (!current) {
    $("torrent-name").textContent = busy
      ? "Adding a torrent…"
      : "Add a torrent";
    $("torrent-meta").textContent = busy
      ? "Looking up the torrent"
      : "Paste a magnet link or open a .torrent file";
  }
}
async function add(options, path = "/api/torrents") {
  setLoading(true);
  banner("");
  try {
    await api(path, { ...options, timeout: 100000 });
    $("link").value = "";
    await refresh();
  } catch (e) {
    banner(e.message);
  } finally {
    setLoading(false);
  }
}
function addLink(link) {
  add({
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ link }),
  });
}
function addTorrentBytes(body) {
  if (current) {
    banner(
      "Stop the current torrent before adding another. They share one RAM cache.",
    );
    return;
  }
  return add(
    {
      method: "POST",
      headers: { "Content-Type": "application/x-bittorrent" },
      body,
    },
    "/api/torrents/file",
  );
}
async function addFile(file) {
  if (!file) return;
  if (file.size > 2097152) {
    banner(
      "This .torrent file is larger than 2 MiB, the limit for torrent metadata.",
    );
    return;
  }
  await addTorrentBytes(await file.arrayBuffer());
}
// The Mac app's File → Open Torrent File… (⌘O) passes the file as base64.
window.torrstreamAddTorrent = (base64) =>
  addTorrentBytes(Uint8Array.from(atob(base64), (c) => c.charCodeAt(0)));
$("add").onsubmit = (event) => {
  event.preventDefault();
  const link = $("link").value.trim();
  if (link) addLink(link);
};
$("file-button").onclick = () => $("upload").click();
$("upload").onchange = async () => {
  await addFile($("upload").files[0]);
  $("upload").value = "";
};
$("stop").onclick = async () => {
  $("stop").disabled = true;
  try {
    await api("/api/torrents", { method: "DELETE" });
    banner("");
    playing = null;
    await refresh();
  } catch (e) {
    banner(e.message);
  } finally {
    $("stop").disabled = false;
  }
};
$("banner-close").onclick = () => {
  dismissedError = $("banner-text").textContent;
  banner("");
};

// A .torrent file dropped anywhere, or a magnet link pasted outside a text field.
let dragDepth = 0;
const draggingFiles = (event) =>
  [...(event.dataTransfer?.types || [])].includes("Files");
document.addEventListener("dragenter", (event) => {
  if (!draggingFiles(event)) return;
  event.preventDefault();
  dragDepth += 1;
  $("drop-overlay").hidden = false;
});
document.addEventListener("dragover", (event) => {
  if (draggingFiles(event)) event.preventDefault();
});
document.addEventListener("dragleave", () => {
  dragDepth = Math.max(0, dragDepth - 1);
  if (!dragDepth) $("drop-overlay").hidden = true;
});
document.addEventListener("drop", (event) => {
  if (!draggingFiles(event)) return;
  event.preventDefault();
  dragDepth = 0;
  $("drop-overlay").hidden = true;
  addFile(event.dataTransfer.files[0]);
});
document.addEventListener("paste", (event) => {
  if (
    current ||
    loading ||
    resolving ||
    dialog ||
    event.target.closest?.("input, textarea")
  )
    return;
  const text = event.clipboardData?.getData("text")?.trim() || "";
  if (!/^(magnet:\?|https?:\/\/)/i.test(text)) return;
  event.preventDefault();
  $("link").value = text;
  addLink(text);
});

async function copy(text, done) {
  try {
    if (desktopApp) desktop({ action: "copy", text });
    else if (navigator.clipboard) await navigator.clipboard.writeText(text);
    else {
      const field = document.createElement("textarea");
      field.value = text;
      field.style.position = "fixed";
      field.style.opacity = "0";
      document.body.append(field);
      field.select();
      const ok = document.execCommand("copy");
      field.remove();
      if (!ok) throw Error("Copy unavailable");
    }
    toast(done);
  } catch {
    banner("Copy this address by hand: " + text);
  }
}

// Home network (the server calls it TV access)
function showNetwork(n) {
  networkState = n;
  $("cache-options").hidden = !n.local_control;
  $("tv-switch").hidden = !n.local_control;
  $("tv-enabled").checked = n.enabled;
  $("tv-enabled").disabled = !n.local_control || networkBusy;
  $("connection-options").hidden = !n.local_control;
  $("remote-settings").hidden = n.local_control;
  $("tv-apply").disabled = !n.local_control || networkBusy;
  if (document.activeElement !== $("tv-port")) $("tv-port").value = n.port;
  const urls = n.local_control
    ? n.addresses.map((a) => a.url)
    : [location.origin];
  const key = JSON.stringify([
    urls,
    n.enabled,
    n.local_control,
    n.errors.length > 0,
  ]);
  if (key !== networkKey) {
    networkKey = key;
    const list = $("tv-addresses");
    list.replaceChildren();
    for (const url of urls) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "address";
      button.title = "Copy address";
      const text = document.createElement("span");
      text.textContent = url;
      button.append(text);
      button.insertAdjacentHTML("beforeend", ICONS.copy);
      button.onclick = () => copy(url, "Address copied");
      list.append(button);
    }
    message(
      "tv-hint",
      !n.local_control
        ? "Connected over your home network."
        : !n.enabled
          ? "Off. Turn on to watch on a TV or another device."
          : urls.length
            ? "On your TV, enter this address in Lampa or another TorrServer app."
            : n.errors.length
              ? ""
              : "Waiting for your home network…",
    );
  }
  const error = n.errors.join("\n");
  message("tv-error", error);
  message("settings-error", error);
}
async function refreshNetwork() {
  if (networkBusy || networkReadBusy || document.hidden) return;
  networkReadBusy = true;
  const revision = networkRevision;
  try {
    const network = await api("/api/network");
    if (revision === networkRevision && !networkBusy) showNetwork(network);
  } catch (e) {
    if (revision === networkRevision && !networkBusy)
      message("tv-error", e.message);
  } finally {
    networkReadBusy = false;
  }
}
async function updateNetwork(enabled, port) {
  if (networkBusy) return false;
  networkRevision += 1;
  networkBusy = true;
  $("tv-enabled").disabled = true;
  $("tv-apply").disabled = true;
  message("settings-error", "");
  try {
    const n = await api("/api/network", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ enabled, port }),
    });
    showNetwork(n);
    desktop({ action: "saveTVSettings", enabled: n.enabled, port: n.port });
    return n.errors.length === 0;
  } catch (e) {
    message("tv-error", e.message);
    message("settings-error", e.message);
    if (networkState) $("tv-enabled").checked = networkState.enabled;
    return false;
  } finally {
    networkBusy = false;
    $("tv-enabled").disabled = !networkState?.local_control;
    $("tv-apply").disabled = !networkState?.local_control;
  }
}
$("tv-enabled").onchange = () =>
  updateNetwork($("tv-enabled").checked, networkState?.port || 8090);
$("port-form").onsubmit = async (event) => {
  event.preventDefault();
  if (
    await updateNetwork(
      networkState?.enabled || false,
      Number($("tv-port").value),
    )
  )
    closeDialog();
};
async function initNetwork() {
  try {
    const n = await api("/api/network");
    showNetwork(n);
    if (desktopApp && n.local_control && window.torrstreamTV) {
      await updateNetwork(
        window.torrstreamTV.enabled,
        window.torrstreamTV.port,
      );
    }
  } catch (e) {
    message("tv-error", e.message);
  }
}

// Torrent and files
function streamUrl(torrent, file) {
  return (
    location.origin +
    "/stream?link=" +
    encodeURIComponent(torrent.hash) +
    "&index=" +
    file.id +
    "&play"
  );
}
function markPlaying() {
  for (const row of $("files").children) {
    row.querySelector(".tag").hidden =
      Number(row.dataset.id) !== playing || !current?.active_readers;
  }
}
function showTorrent(torrent) {
  current = torrent || null;
  $("add-view").hidden = Boolean(torrent);
  $("files-view").hidden = !torrent;
  $("stop").hidden = !torrent;
  if (torrent) {
    const count = torrent.file_count;
    $("torrent-name").textContent = torrent.title;
    $("torrent-name").title = torrent.title;
    $("torrent-meta").textContent =
      count +
      (count === 1 ? " file · " : " files · ") +
      size(torrent.torrent_size);
  } else {
    $("torrent-name").title = "";
    showAddState();
  }
  const hash = torrent?.hash || null;
  if (hash !== lastHash) {
    lastHash = hash;
    loadFiles(torrent);
  }
  markPlaying();
}
// The status poll leaves out the file list (up to 4,096 paths); it's read once per torrent.
async function loadFiles(torrent) {
  const run = ++filesRun;
  renderFiles(torrent, []);
  if (!torrent) return;
  try {
    const answer = await api("/api/files");
    if (run !== filesRun || answer.hash !== torrent.hash) return;
    renderFiles(torrent, answer.files);
    markPlaying();
  } catch (e) {
    // Read it again with the next status.
    if (run === filesRun) lastHash = undefined;
  }
}
function renderFiles(torrent, files) {
  const list = $("files");
  list.replaceChildren();
  list.scrollTop = 0;
  if (!torrent) return;
  const rows = document.createDocumentFragment();
  for (const file of files) {
    const kind = kindOf(file.path);
    const row = document.createElement("li");
    row.className = "file " + kind;
    row.dataset.id = file.id;
    const icon = document.createElement("span");
    icon.className = "file-icon";
    icon.innerHTML = ICONS[kind];
    const name = document.createElement("span");
    name.className = "file-name";
    name.title = file.path;
    const slash = file.path.lastIndexOf("/");
    if (slash >= 0) {
      const dir = document.createElement("span");
      dir.className = "dir";
      dir.textContent = file.path.slice(0, slash + 1);
      name.append(dir);
    }
    const base = document.createElement("span");
    base.className = "base";
    base.textContent = file.path.slice(slash + 1);
    const tag = document.createElement("span");
    tag.className = "tag";
    tag.textContent = "Playing";
    tag.hidden = true;
    name.append(base, tag);
    const length = document.createElement("span");
    length.className = "file-size";
    length.textContent = size(file.length);
    const url = streamUrl(torrent, file);
    const copyButton = document.createElement("button");
    copyButton.type = "button";
    copyButton.className = "tool icon-only";
    copyButton.title = "Copy stream URL";
    copyButton.setAttribute("aria-label", "Copy stream URL of " + file.path);
    copyButton.innerHTML = ICONS.copy;
    copyButton.onclick = () => copy(url, "Stream URL copied");
    row.append(icon, name, length, copyButton);
    if (kind === "video" || kind === "audio") {
      const play = document.createElement("a");
      play.className = "tool play";
      play.href = desktopApp
        ? "torrstream-player://play?url=" + encodeURIComponent(url)
        : "vlc://" + url;
      play.title = desktopApp ? "Open in VLC or IINA" : "Open in VLC";
      play.setAttribute("aria-label", "Play " + file.path);
      play.innerHTML = ICONS.play + '<span class="label">Play</span>';
      play.addEventListener("click", () => {
        playing = file.id;
        markPlaying();
      });
      row.append(play);
    } else row.append(document.createElement("span"));
    rows.append(row);
  }
  list.append(rows);
}
function setState(kind, text) {
  if ($("state").dataset.kind !== kind) $("state").dataset.kind = kind;
  if ($("state-text").textContent !== text) $("state-text").textContent = text;
}
async function refresh() {
  if (statusBusy || document.hidden) return;
  statusBusy = true;
  try {
    const s = await api("/api/status");
    const t = s.torrent;
    resolving = Boolean(s.resolving);
    const speed = mbps(t?.download_speed) + " <small>Mbps</small>";
    $("speed").innerHTML = speed;
    $("peer-total").innerHTML = speed;
    const count = t?.active_peers || 0;
    $("peers").textContent = !t
      ? "No torrent"
      : count
        ? count + (count === 1 ? " peer" : " peers")
        : "Looking for peers";
    $("cache").innerHTML =
      mib(s.cache.reserved_bytes) +
      " <small>/ " +
      Math.round(s.cache.budget_bytes / 1048576) +
      " MiB</small>";
    $("meter").style.width =
      Math.min(100, (100 * s.cache.reserved_bytes) / s.cache.budget_bytes) +
      "%";
    $("rss").innerHTML =
      (s.memory_bytes ? mib(s.memory_bytes) : "—") + " <small>MiB</small>";
    $("cache-pieces").textContent = s.cache.cached_pieces;
    $("cache-evictions").textContent = s.cache.evictions;
    $("cache-piece-size").textContent = s.cache.piece_bytes
      ? size(s.cache.piece_bytes)
      : "—";
    $("app-version").textContent = s.version;
    showCacheSetting(s.cache_setting_mib);
    showTorrent(t);
    const error = t?.error || t?.preload_error || "";
    if (loading || resolving) setState("busy", "Adding torrent…");
    else if (!t) setState("idle", "Idle");
    else if (error) setState("error", "Torrent error");
    else if (t.stat === 2) setState("busy", "Buffering…");
    else if (t.active_readers) setState("ok", "Streaming");
    else setState("ready", "Ready to play");
    if (error && error !== dismissedError) banner(error);
  } catch (e) {
    setState("error", "Server unavailable");
    if (dialog === "peers") {
      peerRates.reset();
      message("peer-error", "Server unavailable");
    }
  } finally {
    statusBusy = false;
  }
}

// Peer details
function stopPeerUpdates() {
  peerRun += 1;
  clearTimeout(peerTimer);
  peerTimer = null;
  peerAbort?.abort();
  peerAbort = null;
  peerRates.reset();
}
function startPeerUpdates() {
  stopPeerUpdates();
  $("peer-rows").replaceChildren();
  $("peer-table").hidden = true;
  $("peer-empty").textContent = "Loading peers…";
  $("peer-empty").hidden = false;
  message("peer-error", "");
  if (!document.hidden) readPeers(peerRun);
}
async function readPeers(run) {
  const controller = new AbortController();
  peerAbort = controller;
  try {
    const sample = await api("/api/peers", { signal: controller.signal });
    if (run !== peerRun || dialog !== "peers") return;
    const peers = peerRates.sample(sample);
    $("peer-summary").textContent =
      peers.length +
      (peers.length === 1 ? " peer" : " peers") +
      " connected · limit " +
      sample.peer_limit;
    const rows = document.createDocumentFragment();
    for (const peer of peers) {
      const row = document.createElement("tr");
      const address = document.createElement("td");
      address.textContent = peer.address;
      const client = document.createElement("small");
      client.textContent = peer.client || "Unknown client";
      address.append(client);
      const rate = document.createElement("td");
      rate.textContent =
        peer.download_bps === null
          ? "Measuring…"
          : mbps(peer.download_bps) + " Mbps";
      const received = document.createElement("td");
      received.textContent = size(peer.received_bytes);
      row.append(address, rate, received);
      rows.append(row);
    }
    $("peer-rows").replaceChildren(rows);
    $("peer-table").hidden = peers.length === 0;
    $("peer-empty").hidden = peers.length !== 0;
    $("peer-empty").textContent = sample.session
      ? "Waiting for peers to connect…"
      : "No torrent is loaded.";
    message("peer-error", "");
  } catch (e) {
    if (run !== peerRun || dialog !== "peers") return;
    peerRates.reset();
    $("peer-rows").replaceChildren();
    $("peer-table").hidden = true;
    $("peer-empty").hidden = true;
    message("peer-error", "Could not update peers: " + e.message);
  } finally {
    if (run === peerRun) {
      peerAbort = null;
      if (dialog === "peers" && !document.hidden)
        peerTimer = setTimeout(() => readPeers(run), 1500);
    }
  }
}

// Dialogs
const DIALOGS = ["settings", "peers", "license"];
function openDialog(name, trigger) {
  stopPeerUpdates();
  if (trigger) returnFocus = trigger;
  dialog = name;
  $("overlay").hidden = false;
  for (const type of DIALOGS) $(type + "-dialog").hidden = type !== name;
  $("app").inert = true;
  $("app").setAttribute("aria-hidden", "true");
  $("peers-button").setAttribute("aria-expanded", String(name === "peers"));
  if (name === "settings") showTheme();
  $(name + "-dialog")
    .querySelector("button")
    .focus();
  if (name === "peers") startPeerUpdates();
}
function closeDialog() {
  stopPeerUpdates();
  licenseRun += 1;
  dialog = null;
  $("overlay").hidden = true;
  for (const type of DIALOGS) $(type + "-dialog").hidden = true;
  $("app").inert = false;
  $("app").removeAttribute("aria-hidden");
  $("peers-button").setAttribute("aria-expanded", "false");
  $("peer-rows").replaceChildren();
  returnFocus?.focus();
}
$("settings-button").onclick = (event) =>
  openDialog("settings", event.currentTarget);
$("peers-button").onclick = (event) => openDialog("peers", event.currentTarget);
window.torrstreamOpenSettings = () =>
  openDialog("settings", $("settings-button"));
$("license-back").onclick = () => {
  licenseRun += 1;
  openDialog("settings");
};
document
  .querySelectorAll("[data-close-dialog]")
  .forEach((button) => (button.onclick = closeDialog));
$("overlay").onclick = (event) => {
  if (event.target === $("overlay")) closeDialog();
};
document.addEventListener("keydown", (event) => {
  if (!dialog) return;
  if (event.key === "Escape") {
    event.preventDefault();
    closeDialog();
  }
  if (event.key === "Tab") {
    const controls = [
      ...$(dialog + "-dialog").querySelectorAll("button,input,a[href],summary"),
    ].filter((el) => !el.disabled && el.getClientRects().length);
    const first = controls[0],
      last = controls[controls.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }
});
document.querySelectorAll("[data-license]").forEach((button) => {
  button.onclick = async () => {
    const type = button.dataset.license,
      run = ++licenseRun;
    $("license-title").textContent =
      type === "app" ? "MIT license" : "Third-party notices";
    $("license-text").textContent = "Loading…";
    openDialog("license");
    try {
      const text = await api("/licenses/" + type, {}, true);
      if (run === licenseRun) $("license-text").textContent = text;
    } catch (e) {
      if (run === licenseRun) $("license-text").textContent = e.message;
    }
  };
});
document.querySelectorAll("a[data-external]").forEach((link) => {
  link.onclick = (event) => {
    if (desktopApp) {
      event.preventDefault();
      desktop({ action: "openExternal", url: link.href });
    }
  };
});

// Settings → Cache: the size of the next torrent's cache. The Mac app keeps it
// and starts the server with it next time.
function showCacheSetting(mib) {
  for (const button of document.querySelectorAll("#cache-size button")) {
    button.setAttribute(
      "aria-checked",
      String(Number(button.dataset.mib) === mib),
    );
  }
}
document.querySelectorAll("#cache-size button").forEach((button) => {
  button.onclick = async () => {
    const mib = Number(button.dataset.mib);
    message("settings-error", "");
    try {
      await api("/api/cache", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ mib }),
      });
      showCacheSetting(mib);
      desktop({ action: "saveCacheSize", mib });
      toast(
        current
          ? `Cache: ${mib} MiB from the next torrent`
          : `Cache: ${mib} MiB`,
      );
      refresh();
    } catch (e) {
      message("settings-error", e.message);
    }
  };
});

// Appearance: System, Light or Dark, as in Unified Mail. The Mac app stores
// it and sets the window's appearance so the titlebar follows.
function currentTheme() {
  return document.documentElement.dataset.theme || "system";
}
function showTheme() {
  for (const button of document.querySelectorAll("#theme button")) {
    button.setAttribute(
      "aria-checked",
      String(button.dataset.theme === currentTheme()),
    );
  }
}
function applyTheme(theme) {
  if (theme === "system")
    document.documentElement.removeAttribute("data-theme");
  else document.documentElement.dataset.theme = theme;
  showTheme();
}
document.querySelectorAll("#theme button").forEach((button) => {
  button.onclick = () => {
    applyTheme(button.dataset.theme);
    try {
      localStorage.setItem("theme", button.dataset.theme);
    } catch {}
    desktop({ action: "setTheme", theme: button.dataset.theme });
  };
});
try {
  applyTheme(
    window.torrstreamTheme || localStorage.getItem("theme") || "system",
  );
} catch {
  applyTheme("system");
}

document.addEventListener("visibilitychange", () => {
  if (document.hidden) stopPeerUpdates();
  else {
    refresh();
    refreshNetwork();
    if (dialog === "peers") startPeerUpdates();
  }
});
initNetwork();
refresh();
setInterval(refreshNetwork, 3000);
setInterval(refresh, 1500);
