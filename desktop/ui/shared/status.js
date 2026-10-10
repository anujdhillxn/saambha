// What Saambha is doing, from the server (asked through the app's Rust side).

export const OFFLINE = { state: "cold", line: "Can't reach the Saambha server", pairing: null, speaking: false };

const RETRY_MS = 2000;

const ask = (args) => window.__TAURI__.core.invoke("status", args);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Calls [show] with the status, then again at each change, and OFFLINE while the server can't be
 * reached. Each request names the version last shown, and the server answers it once that's changed.
 */
export async function watchStatus(show, request = ask, wait = sleep) {
  let version = null;
  for (;;) {
    try {
      const status = await request(version === null ? {} : { after: version });
      version = status.version;
      show(status);
    } catch {
      version = null;
      show(OFFLINE);
      await wait(RETRY_MS);
    }
  }
}
