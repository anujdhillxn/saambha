// Following the server's status (ui/shared/status.js). Run: node --test
import assert from "node:assert/strict";
import { test } from "node:test";

import { OFFLINE, watchStatus } from "../ui/shared/status.js";

const READY = { state: "ready", version: 4 };
const LISTENING = { state: "listening", version: 5 };
const STOP = new Error("stop watching");

/** A server answering with [replies] in turn (an Error is thrown), and what it was asked. */
function server(...replies) {
  const asked = [];
  return {
    asked,
    request: async (args) => {
      asked.push(args);
      const reply = replies.shift() ?? STOP;
      if (reply instanceof Error) throw reply;
      return reply;
    },
  };
}

test("each request waits for a change from the version last shown", async () => {
  const { asked, request } = server(READY, LISTENING);
  const shown = [];

  await assert.rejects(watchStatus((s) => shown.push(s), request, () => Promise.reject(STOP)), STOP);

  assert.deepEqual(asked, [{}, { after: 4 }, { after: 5 }]);
  assert.deepEqual(shown, [READY, LISTENING, OFFLINE]);
});

test("once the server's back, it starts over from its status now", async () => {
  const { asked, request } = server(new Error("refused"), READY);
  const waits = [];
  const wait = (ms) => (waits.push(ms), waits.length > 1 ? Promise.reject(STOP) : Promise.resolve());

  await assert.rejects(watchStatus(() => {}, request, wait), STOP);

  assert.deepEqual(asked, [{}, {}, { after: 4 }]);
  assert.deepEqual(waits, [2000, 2000]);
});
