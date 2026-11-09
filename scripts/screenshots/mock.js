// Screenshot fixture: answers the control page's requests with a fixed, realistic
// state (a series streaming, eight peers, home network on). Runs before app.js.
(() => {
  const MiB = 1048576, GiB = 1073741824, BUDGET = 96 * MiB, PIECE = 4 * MiB;
  const theme = location.hash.slice(1) || "light";
  window.torrstreamDesktop = true;
  window.torrstreamTheme = theme;
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.setProperty("--titlebar", "28px");
  // Offscreen snapshots count as hidden pages; the page should poll as if visible.
  Object.defineProperty(document, "hidden", { get: () => false });
  Object.defineProperty(document, "visibilityState", { get: () => "visible" });

  const files = [
    ...Array.from({ length: 12 }, (_, i) => [
      `Season 1/Coastline.Stories.S01E${String(i + 1).padStart(2, "0")}.1080p.mkv`,
      Math.round((1.2 + ((i * 37) % 9) / 20) * GiB),
    ]),
    ...Array.from({ length: 10 }, (_, i) => [
      `Season 2/Coastline.Stories.S02E${String(i + 1).padStart(2, "0")}.1080p.mkv`,
      Math.round((1.3 + ((i * 23) % 7) / 20) * GiB),
    ]),
    ["Extras/Behind the Scenes.mp4", 412 * MiB],
    ["Subtitles/Coastline.Stories.S01.en.srt", 61440],
    ["Subtitles/Coastline.Stories.S02.en.srt", 58368],
    ["Coastline.Stories.nfo", 2048],
  ];
  const hash = "5be01d9c3a7f44e2b6c8d0e1f2a3b4c5d6e7f809";
  // What /api/status reports while it plays; the files come from /api/files.
  const torrent = {
    title: "Coastline Stories, Seasons 1–2",
    hash,
    file_count: files.length,
    torrent_size: files.reduce((sum, f) => sum + f[1], 0),
    download_speed: 2.31 * MiB,
    active_peers: 8,
    stat: 3,
    error: null,
    preload_error: null,
    active_readers: 1,
  };
  const replies = {
    "/api/status": () => ({
      server: "TorrStream Lite",
      version: "0.3.0",
      mode: "RAM only",
      memory_bytes: Math.round(124.3 * MiB),
      resident_bytes: Math.round(131.6 * MiB),
      cache: {
        budget_bytes: BUDGET,
        allocated_bytes: BUDGET,
        reserved_bytes: BUDGET,
        cached_pieces: BUDGET / PIECE,
        slots: BUDGET / PIECE,
        piece_bytes: PIECE,
        peak_reserved_bytes: BUDGET,
        evictions: 212,
        reservations_denied: 0,
      },
      peer_limit: 20,
      uptime_seconds: 1520,
      torrent,
      resolving: false,
    }),
    "/api/files": () => ({
      hash,
      files: files.map(([path, length], i) => ({ id: i + 1, path, length, stream_url: "/api/stream/" + (i + 1) })),
    }),
    "/api/network": () => ({
      enabled: true,
      port: 8090,
      addresses: [{ interface: "en0", url: "http://192.168.1.35:8090" }],
      errors: [],
      local_control: true,
    }),
    "/api/peers": () => ({ session: hash + ":1", sampled_at_ms: performance.now(), peer_limit: 20, peers: [] }),
  };
  window.fetch = async (input) => {
    const path = new URL(typeof input === "string" ? input : input.url, location.href).pathname;
    const reply = replies[path];
    return new Response(JSON.stringify(reply ? reply() : { error: "Not found" }), {
      status: reply ? 200 : 404,
      headers: { "Content-Type": "application/json" },
    });
  };
  // Play links use the app's URL scheme; keep the page where it is.
  document.addEventListener(
    "click",
    (event) => {
      if (event.target.closest?.('a[href^="torrstream-player:"]')) event.preventDefault();
    },
    true,
  );
})();
