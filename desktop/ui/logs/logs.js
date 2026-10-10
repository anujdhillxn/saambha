// The server's log, following it as it grows.
const { invoke } = window.__TAURI__.core;
const POLL_MS = 700;
const KEEP = 2_000_000; // characters shown, the oldest dropped beyond that
const log = document.getElementById("log");
let offset = null;

async function follow() {
  try {
    const piece = await invoke("log", { offset });
    offset = piece.offset;
    if (!piece.text) return;
    const atBottom = log.scrollTop + log.clientHeight >= log.scrollHeight - 4;
    log.textContent = (log.textContent + piece.text).slice(-KEEP);
    if (atBottom) log.scrollTop = log.scrollHeight; // don't yank the view away from what's being read
  } catch {
    // the server is down: try again next time
  }
}

follow();
setInterval(follow, POLL_MS);
