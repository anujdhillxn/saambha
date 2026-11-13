// A benchmark: the app, left alone, must cost next to nothing. It runs the release build on its own
// port and files (a second dot shows meanwhile) and measures what it uses while it sits idle. The
// desktop is in use meanwhile, so it judges the quietest of a few windows. Needs a desktop session and
// `cargo build --release`.
// Run: node --test bench/*.js
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { test } from "node:test";

const APP = new URL("../target/release/saambha", import.meta.url).pathname;
const SETTLE_MS = 10000; // to start up (longer after a fresh build)
const WINDOW_MS = 5000;
const WINDOWS = 3; // to judge the quietest
// Budgets, a little lenient.
const CPU = 0.01; // of one core
const WAKEUPS = 20; // a second
const MEMORY_MB = 30;

const TICKS_PER_S = 100; // USER_HZ, the unit of /proc/<pid>/stat's CPU times

/** [pid] and its descendants (started from any of its threads). */
function processes(pid) {
  const children = readdirSync(`/proc/${pid}/task`)
    .flatMap((task) => readFileSync(`/proc/${pid}/task/${task}/children`, "utf8").split(" ").filter(Boolean));
  return [pid, ...children.flatMap((child) => processes(Number(child)))];
}

/**
 * The CPU seconds and context switches (wakeups) [pid] and its descendants have had so far, each
 * process's and thread's by its id: what one that's gone had is gone with it.
 */
function counters(pid) {
  const cpu = new Map();
  const switches = new Map();
  for (const p of processes(pid)) {
    const fields = readFileSync(`/proc/${p}/stat`, "utf8").split(") ")[1].split(" ");
    cpu.set(p, (Number(fields[11]) + Number(fields[12])) / TICKS_PER_S); // utime, stime
    for (const task of readdirSync(`/proc/${p}/task`)) {
      const status = readFileSync(`/proc/${p}/task/${task}/status`, "utf8");
      switches.set(task, [...status.matchAll(/ctxt_switches:\s+(\d+)/g)].reduce((sum, [, n]) => sum + Number(n), 0));
    }
  }
  return { cpu, switches };
}

/** How much the counters in [after] grew since [before] (ones new since then, from nothing). */
function growth(before, after) {
  return [...after].reduce((sum, [id, n]) => sum + n - (before.get(id) ?? 0), 0);
}

/** Their memory in MB of PSS: shared pages split between the processes sharing them. */
function memoryMb(pid) {
  const kb = (p) => Number(readFileSync(`/proc/${p}/smaps_rollup`, "utf8").match(/^Pss:\s+(\d+)/m)[1]);
  return processes(pid).reduce((sum, p) => sum + kb(p), 0) / 1024;
}

/** A port nothing's listening on. */
function freePort() {
  return new Promise((resolve) => {
    const server = createServer().listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

const skip = !(process.env.WAYLAND_DISPLAY || process.env.DISPLAY) ? "no desktop session"
  : !existsSync(APP) ? "no release build" : false;

test("the app left alone costs next to nothing", { skip }, async () => {
  const work = mkdtempSync(join(tmpdir(), "saambha-bench-"));
  const env = {
    ...process.env,
    XDG_STATE_HOME: work, // where the dot was left
    SAAMBHA_PORT: String(await freePort()),
    SAAMBHA_DEVICES_FILE: join(work, "devices.json"),
    SAAMBHA_HISTORY_FILE: join(work, "history.jsonl"),
    SAAMBHA_SESSION_FILE: join(work, "session_id"),
    SAAMBHA_LOG_FILE: join(work, "saambha.log"),
  };
  const app = spawn(APP, [], { stdio: "ignore", env });
  try {
    await sleep(SETTLE_MS);
    const idle = [];
    for (let n = 0; n < WINDOWS; n++) {
      const before = counters(app.pid);
      await sleep(WINDOW_MS);
      const after = counters(app.pid);
      const seconds = WINDOW_MS / 1000;
      idle.push({
        cpu: growth(before.cpu, after.cpu) / seconds,
        wakeups: growth(before.switches, after.switches) / seconds,
      });
    }
    const cpu = Math.min(...idle.map((w) => w.cpu));
    const wakeups = Math.min(...idle.map((w) => w.wakeups));
    const memory = memoryMb(app.pid);

    console.log(`${(cpu * 100).toFixed(1)}% of a core, ${wakeups} wakeups a second, ${memory.toFixed(0)} MB`);
    assert.ok(cpu < CPU, `${(cpu * 100).toFixed(1)}% of a core`);
    assert.ok(wakeups < WAKEUPS, `${wakeups} wakeups a second`);
    assert.ok(memory < MEMORY_MB, `${memory.toFixed(0)} MB`);
  } finally {
    app.kill();
    rmSync(work, { recursive: true, force: true });
  }
});
