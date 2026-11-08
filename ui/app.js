const $ = (id) => document.getElementById(id);
let lastHash,
  loading = false,
  statusBusy = false;
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
  licenseRun = 0;
const peerRates = new PeerRates();

function mib(n) {
  return (n / 1048576).toFixed(1);
}
function size(n) {
  if (n >= 1073741824) return (n / 1073741824).toFixed(2) + " GiB";
  if (n >= 1048576) return mib(n) + " MiB";
  if (n >= 1024) return (n / 1024).toFixed(1) + " KiB";
  return n + " B";
}
function message(id, text) {
  $(id).textContent = text;
  $(id).hidden = !text;
}
function speedMarkup(bytes) {
  return (((bytes || 0) * 8) / 1000000).toFixed(1) + " <span>Mbps</span>";
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
    const value = plain ? await response.text() : await response.json();
    if (!response.ok) throw Error(value.error || response.statusText);
    return value;
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener("abort", abort);
  }
}
function desktop(value) {
  window.webkit?.messageHandlers?.desktop?.postMessage(value);
}
function busy(value) {
  loading = value;
  $("add-button").disabled = value;
  $("file-button").disabled = value;
  message("hint", value ? "Resolving torrent and finding peers…" : "");
}
async function add(options, path = "/api/torrents") {
  busy(true);
  message("error", "");
  try {
    await api(path, { ...options, timeout: 100000 });
    $("link").value = "";
    await refresh();
  } catch (e) {
    message("error", e.message);
  } finally {
    busy(false);
  }
}
$("add").onsubmit = (event) => {
  event.preventDefault();
  add({
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ link: $("link").value.trim() }),
  });
};
$("file-button").onclick = () => $("upload").click();
$("upload").onchange = async () => {
  const file = $("upload").files[0];
  if (!file) return;
  if (file.size > 2097152) {
    message("error", "Torrent metadata must be at most 2 MiB.");
    $("upload").value = "";
    return;
  }
  await add(
    {
      method: "POST",
      headers: { "Content-Type": "application/x-bittorrent" },
      body: await file.arrayBuffer(),
    },
    "/api/torrents/file",
  );
  $("upload").value = "";
};
$("stop").onclick = async () => {
  $("stop").disabled = true;
  try {
    await api("/api/torrents", { method: "DELETE" });
    message("error", "");
    await refresh();
  } catch (e) {
    message("error", e.message);
  } finally {
    $("stop").disabled = false;
  }
};
async function copy(url, button) {
  const original = button.textContent;
  try {
    if (window.torrstreamDesktop) desktop({ action: "copy", text: url });
    else if (navigator.clipboard) await navigator.clipboard.writeText(url);
    else {
      const field = document.createElement("textarea");
      field.value = url;
      field.style.position = "fixed";
      field.style.opacity = "0";
      document.body.append(field);
      field.select();
      const ok = document.execCommand("copy");
      field.remove();
      if (!ok) throw Error("Copy unavailable");
    }
    button.textContent = "Copied";
    setTimeout(() => {
      if (button.isConnected) button.textContent = original;
    }, 1600);
  } catch {
    message("error", url);
  }
}

function showNetwork(n) {
  networkState = n;
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
  const fallback = !n.enabled ? "TV access off" : "Waiting for home network…";
  const key = JSON.stringify([urls, fallback]);
  if (key !== networkKey) {
    networkKey = key;
    $("tv-addresses").replaceChildren();
    for (const url of urls.length ? urls : [fallback]) {
      const address = document.createElement("span");
      address.textContent = url;
      if (!urls.length) address.className = "muted";
      $("tv-addresses").append(address);
    }
  }
  const error = n.errors.join("\n");
  message("network-error", error);
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
      message("network-error", e.message);
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
    message("network-error", e.message);
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
  updateNetwork($("tv-enabled").checked, networkState?.port || 8091);
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
    if (window.torrstreamDesktop && n.local_control && window.torrstreamTV) {
      await updateNetwork(
        window.torrstreamTV.enabled,
        window.torrstreamTV.port,
      );
    }
  } catch (e) {
    message("network-error", e.message);
  }
}

function files(torrent) {
  $("stop").hidden = !torrent;
  $("torrent-name").textContent = torrent ? torrent.title : "Ready to stream";
  $("file-count").textContent = torrent
    ? torrent.file_stats.length +
      (torrent.file_stats.length === 1 ? " file" : " files")
    : "";
  const hash = torrent?.hash || null;
  if (hash === lastHash) return;
  lastHash = hash;
  $("files").replaceChildren();
  if (!torrent) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = "Add a magnet link or open a .torrent file.";
    $("files").append(empty);
    return;
  }
  for (const file of torrent.file_stats) {
    const row = document.createElement("div");
    row.className = "file";
    const name = document.createElement("div");
    name.className = "file-name";
    name.textContent = file.path;
    const length = document.createElement("small");
    length.textContent = size(file.length);
    name.append(length);
    const actions = document.createElement("div");
    actions.className = "file-actions";
    const url =
      location.origin +
      "/stream?link=" +
      encodeURIComponent(torrent.hash) +
      "&index=" +
      file.id +
      "&play";
    const cp = document.createElement("button");
    cp.textContent = "Copy stream URL";
    cp.onclick = () => copy(url, cp);
    const play = document.createElement("a");
    play.className = "button primary";
    play.textContent = window.torrstreamDesktop
      ? "Open in player"
      : "Open in VLC";
    play.href = window.torrstreamDesktop
      ? "torrstream-player://play?url=" + encodeURIComponent(url)
      : "vlc://" + url;
    actions.append(cp, play);
    row.append(name, actions);
    $("files").append(row);
  }
}
async function refresh() {
  if (statusBusy || document.hidden) return;
  statusBusy = true;
  try {
    const s = await api("/api/status");
    $("rss").innerHTML =
      (s.resident_bytes ? mib(s.resident_bytes) : "—") + " <span>MiB</span>";
    $("cache").innerHTML =
      mib(s.cache.reserved_bytes) +
      " <span>/ " +
      mib(s.cache.budget_bytes) +
      " MiB</span>";
    $("cache-pieces").textContent = s.cache.cached_pieces;
    $("cache-evictions").textContent = s.cache.evictions;
    $("cache-piece-size").textContent = s.cache.piece_bytes
      ? size(s.cache.piece_bytes)
      : "—";
    $("meter").style.width =
      Math.min(100, (100 * s.cache.reserved_bytes) / s.cache.budget_bytes) +
      "%";
    $("speed").innerHTML = speedMarkup(s.torrent?.download_speed);
    $("peer-total").innerHTML = speedMarkup(s.torrent?.download_speed);
    const count = s.torrent?.active_peers || 0;
    $("peers").textContent = count
      ? count + (count === 1 ? " connected peer" : " connected peers")
      : "No active peers";
    $("app-version").textContent = s.version;
    $("footer-status").textContent =
      loading || s.resolving
        ? "Resolving…"
        : !s.torrent
          ? "Idle"
          : s.torrent.stat === 2
            ? "Buffering…"
            : s.torrent.active_readers
              ? "Streaming"
              : "Ready for playback";
    files(s.torrent);
    if (s.torrent?.engine?.error) message("error", s.torrent.engine.error);
  } catch (e) {
    $("footer-status").textContent = "Server unavailable: " + e.message;
    if (dialog === "peers") {
      peerRates.reset();
      message("peer-error", "Server unavailable");
    }
  } finally {
    statusBusy = false;
  }
}

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
      peers.length + " connected · limit " + sample.peer_limit;
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
          : ((peer.download_bps * 8) / 1000000).toFixed(1) + " Mbps";
      const received = document.createElement("td");
      received.textContent = size(peer.received_bytes);
      row.append(address, rate, received);
      rows.append(row);
    }
    $("peer-rows").replaceChildren(rows);
    $("peer-table").hidden = peers.length === 0;
    $("peer-empty").hidden = peers.length !== 0;
    $("peer-empty").textContent = sample.session
      ? "Waiting for connected peers…"
      : "No torrent loaded.";
    message("peer-error", "");
  } catch (e) {
    if (run !== peerRun || dialog !== "peers") return;
    peerRates.reset();
    $("peer-rows").replaceChildren();
    $("peer-table").hidden = true;
    message("peer-error", "Could not update peers: " + e.message);
    $("peer-empty").hidden = true;
  } finally {
    if (run === peerRun) {
      peerAbort = null;
      if (dialog === "peers" && !document.hidden)
        peerTimer = setTimeout(() => readPeers(run), 1500);
    }
  }
}
function openDialog(name, trigger) {
  stopPeerUpdates();
  if (trigger) returnFocus = trigger;
  dialog = name;
  $("overlay").hidden = false;
  for (const type of ["settings", "peers", "license"])
    $(type + "-dialog").hidden = type !== name;
  $("app").inert = true;
  $("app").setAttribute("aria-hidden", "true");
  document.body.style.overflow = "hidden";
  $("peers-button").setAttribute("aria-expanded", String(name === "peers"));
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
  for (const type of ["settings", "peers", "license"])
    $(type + "-dialog").hidden = true;
  $("app").inert = false;
  $("app").removeAttribute("aria-hidden");
  document.body.style.overflow = "";
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
document.querySelectorAll("[data-close-dialog]").forEach((button) => {
  button.onclick = closeDialog;
});
$("overlay").onclick = (event) => {
  if (event.target === $("overlay")) closeDialog();
};
document.addEventListener("keydown", (event) => {
  if (!dialog) return;
  if (event.key === "Escape") {
    event.preventDefault();
    closeDialog();
  }
  if (event.key === "Tab" && dialog) {
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
    if (window.torrstreamDesktop) {
      event.preventDefault();
      desktop({ action: "openExternal", url: link.href });
    }
  };
});
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
