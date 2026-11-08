const $ = (id) => document.getElementById(id);
let lastHash = null,
  loading = false;
function mib(n) {
  return (n / 1048576).toFixed(1);
}
async function api(path, options = {}) {
  const r = await fetch(path, options);
  const v = await r.json();
  if (!r.ok) throw Error(v.error || r.statusText);
  return v;
}
function busy(value) {
  loading = value;
  $("add-button").disabled = value;
  $("file-button").disabled = value;
  $("hint").textContent = value
    ? "Resolving torrent and finding peers…"
    : "Video stays in RAM. Use VLC or IINA for 4K files.";
}
async function add(options, path = "/api/torrents") {
  busy(true);
  $("error").textContent = "";
  try {
    await api(path, options);
    $("link").value = "";
    await refresh();
  } catch (e) {
    $("error").textContent = e.message;
  } finally {
    busy(false);
  }
}
$("add").onsubmit = (e) => {
  e.preventDefault();
  add({
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ link: $("link").value.trim() }),
  });
};
$("file-button").onclick = () => $("upload").click();
$("upload").onchange = async () => {
  const f = $("upload").files[0];
  if (!f) return;
  if (f.size > 2097152) {
    $("error").textContent = "Torrent metadata must be at most 2 MiB.";
    return;
  }
  await add(
    {
      method: "POST",
      headers: { "Content-Type": "application/x-bittorrent" },
      body: await f.arrayBuffer(),
    },
    "/api/torrents/file",
  );
  $("upload").value = "";
};
$("stop").onclick = async () => {
  try {
    await api("/api/torrents", { method: "DELETE" });
    await refresh();
  } catch (e) {
    $("error").textContent = e.message;
  }
};
function desktop(message) {
  window.webkit?.messageHandlers?.desktop?.postMessage(message);
}
async function copy(url, b) {
  const original = b.textContent;
  try {
    if (window.torrstreamDesktop) {
      desktop({ action: "copy", text: url });
    } else if (navigator.clipboard) {
      await navigator.clipboard.writeText(url);
    } else {
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
    b.textContent = "Copied";
    setTimeout(() => (b.textContent = original), 1600);
  } catch (e) {
    $("error").textContent = url;
  }
}
let networkBusy = false,
  networkState = null,
  networkKey = "";
function showNetwork(n) {
  networkState = n;
  $("tv-panel").hidden = false;
  $("tv-enabled").checked = n.enabled;
  $("tv-enabled").disabled = !n.local_control || networkBusy;
  $("tv-options").hidden = !n.local_control;
  if (document.activeElement !== $("tv-port")) $("tv-port").value = n.port;
  $("tv-status").textContent = !n.enabled
    ? "Turn on TV access to show the address for your TV app."
    : n.addresses.length
      ? "Enter this server address in your TV app."
      : "Waiting for a home-network connection. Connect this Mac to Wi-Fi or Ethernet.";
  $("network-error").textContent = n.errors.join("\n");
  if (n.errors.length && !n.addresses.length)
    $("tv-status").textContent =
      "TV access could not start. Try another port in Connection options.";
  const key = JSON.stringify(n.addresses);
  if (key !== networkKey) {
    networkKey = key;
    $("tv-addresses").replaceChildren();
    for (const a of n.addresses) {
      const row = document.createElement("div");
      row.className = "tv-address";
      const text = document.createElement("div");
      const address = document.createElement("code");
      address.textContent = a.url;
      const label = document.createElement("small");
      label.textContent = "TV server address";
      text.append(address, label);
      const button = document.createElement("button");
      button.textContent = "Copy address";
      button.onclick = () => copy(a.url, button);
      row.append(text, button);
      $("tv-addresses").append(row);
    }
  }
}
async function refreshNetwork() {
  if (networkBusy) return;
  try {
    showNetwork(await api("/api/network"));
  } catch (e) {
    $("network-error").textContent = e.message;
  }
}
async function updateNetwork(enabled, port) {
  networkBusy = true;
  $("tv-enabled").disabled = true;
  $("tv-apply").disabled = true;
  try {
    const n = await api("/api/network", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ enabled, port }),
    });
    showNetwork(n);
    desktop({ action: "saveTVSettings", enabled: n.enabled, port: n.port });
  } catch (e) {
    $("network-error").textContent = e.message;
    if (networkState) $("tv-enabled").checked = networkState.enabled;
  } finally {
    networkBusy = false;
    $("tv-enabled").disabled = !networkState?.local_control;
    $("tv-apply").disabled = false;
  }
}
$("tv-enabled").onchange = () =>
  updateNetwork($("tv-enabled").checked, Number($("tv-port").value));
$("tv-apply").onclick = () =>
  updateNetwork($("tv-enabled").checked, Number($("tv-port").value));
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
    $("network-error").textContent = e.message;
  }
}
initNetwork();
setInterval(refreshNetwork, 3000);
function files(t) {
  const hash = t?.hash || null;
  if (hash === lastHash) return;
  lastHash = hash;
  $("files").replaceChildren();
  $("stop").hidden = !t;
  $("torrent-name").textContent = t ? t.title : "Ready to stream";
  if (!t) {
    const e = document.createElement("div");
    e.className = "empty";
    e.textContent = "Add a torrent, then open a file in your player.";
    $("files").append(e);
    return;
  }
  for (const f of t.file_stats) {
    const row = document.createElement("div");
    row.className = "file";
    const name = document.createElement("div");
    name.className = "file-name";
    name.textContent = f.path;
    const size = document.createElement("small");
    size.textContent = (f.length / 1073741824).toFixed(2) + " GiB";
    name.append(size);
    const actions = document.createElement("div");
    actions.className = "file-actions";
    const url =
      location.origin +
      "/stream?link=" +
      encodeURIComponent(t.hash) +
      "&index=" +
      f.id +
      "&play";
    const cp = document.createElement("button");
    cp.className = "secondary";
    cp.textContent = "Copy stream URL";
    cp.onclick = () => copy(url, cp);
    const play = document.createElement("a");
    play.className = "button";
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
  try {
    const s = await api("/api/status");
    $("rss").innerHTML =
      (s.resident_bytes ? mib(s.resident_bytes) : "—") + " <span>MiB</span>";
    $("cache").innerHTML =
      mib(s.cache.reserved_bytes) +
      " <span>/ " +
      mib(s.cache.budget_bytes) +
      " MiB</span>";
    $("cache-detail").textContent =
      s.cache.cached_pieces + " pieces · " + s.cache.evictions + " evictions";
    $("meter").style.width =
      (100 * s.cache.reserved_bytes) / s.cache.budget_bytes + "%";
    const live = s.torrent?.engine?.live;
    $("speed").innerHTML =
      ((live?.download_speed?.mbps || 0) * 8.388608).toFixed(1) +
      " <span>Mbps</span>";
    $("peers").textContent = s.torrent
      ? s.torrent.active_readers +
        " active requests · peer limit " +
        s.peer_limit
      : "No torrent loaded";
    $("footer-status").textContent =
      "TorrStream Lite " +
      s.version +
      " · " +
      (loading || s.resolving
        ? "Resolving…"
        : s.torrent
          ? "Ready for playback"
          : "Idle");
    files(s.torrent);
    if (s.torrent?.engine?.error)
      $("error").textContent = s.torrent.engine.error;
  } catch (e) {
    $("footer-status").textContent = "Server unavailable: " + e.message;
  }
}
refresh();
setInterval(refresh, 1500);
