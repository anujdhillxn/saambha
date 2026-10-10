// A benchmark: the app, left alone, must cost next to nothing. It runs the release build (a second
// dot shows meanwhile) against the Saambha server on this machine, and measures what it and its
// web views use while Saambha sits idle. The desktop is in use meanwhile, so it judges the quietest
// of a few windows, and only ones where Saambha's status didn't change. Needs a desktop session,
// `cargo build --release` and the server running.
// Run: node --test bench/*.js
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { get } from "node:https";
import { setTimeout as sleep } from "node:timers/promises";
import { test } from "node:test";

const APP = new URL("../target/release/saambha-desktop", import.meta.url).pathname;
const CA = new URL("../../server/ca.pem", import.meta.url).pathname;
const SERVER = process.env.SAAMBHA_URL ?? "https://127.0.0.1:8765";
const SETTLE_MS = 10000; // to start up and load its pages (longer after a fresh build)
const WINDOW_MS = 5000;
const WINDOWS = 3; // idle ones, to judge the quietest
const TRIES = 8; // windows, for that many idle ones
// Budgets, a little lenient.
const CPU = 0.01; // of one core
const WAKEUPS = 20; // a second
const MEMORY_MB = 300;

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

/** The version of the server's status: it changes with whatever Saambha does. */
function statusVersion() {
  return new Promise((resolve, reject) => {
    get(`${SERVER}/desktop/status`, { ca: readFileSync(CA) }, (res) => {
      let body = "";
      res.on("data", (chunk) => (body += chunk)).on("end", () => resolve(JSON.parse(body).version));
    }).on("error", reject);
  });
}

const skip = !(process.env.WAYLAND_DISPLAY || process.env.DISPLAY) ? "no desktop session"
  : !existsSync(APP) ? "no release build"
  : !existsSync(CA) ? "no server CA" : false;

test("the app left alone costs next to nothing", { skip }, async () => {
  const app = spawn(APP, [SERVER, CA], { stdio: "ignore" });
  try {
    await sleep(SETTLE_MS);
    const idle = [];
    for (let tries = TRIES; tries && idle.length < WINDOWS; tries--) {
      const version = await statusVersion();
      const before = counters(app.pid);
      await sleep(WINDOW_MS);
      const after = counters(app.pid);
      if ((await statusVersion()) !== version) continue;
      const seconds = WINDOW_MS / 1000;
      idle.push({
        cpu: growth(before.cpu, after.cpu) / seconds,
        wakeups: growth(before.switches, after.switches) / seconds,
      });
    }
    assert.ok(idle.length, "Saambha was never left alone");
    const cpu = Math.min(...idle.map((w) => w.cpu));
    const wakeups = Math.min(...idle.map((w) => w.wakeups));
    const memory = memoryMb(app.pid);

    console.log(`${(cpu * 100).toFixed(1)}% of a core, ${wakeups} wakeups a second, ${memory.toFixed(0)} MB`);
    assert.ok(cpu < CPU, `${(cpu * 100).toFixed(1)}% of a core`);
    assert.ok(wakeups < WAKEUPS, `${wakeups} wakeups a second`);
    assert.ok(memory < MEMORY_MB, `${memory.toFixed(0)} MB`);
  } finally {
    app.kill();
  }
});
