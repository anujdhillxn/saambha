// The chat panel's rows (ui/stage/chat.js). Run: node --test
import assert from "node:assert/strict";
import { test } from "node:test";

import { rows, when } from "../ui/stage/chat.js";

process.env.TZ = "UTC";
const NOW = new Date(Date.UTC(2026, 9, 10, 16, 5));
const at = (hours, minutes, day = 10) => Date.UTC(2026, 9, day, hours, minutes) / 1000;

test("today shows the time", () => {
  assert.equal(when(at(15, 5), NOW), "15:05");
});

test("earlier days show the date too", () => {
  assert.equal(when(at(16, 5, 9), NOW), "Oct 9, 16:05");
});

test("a time starts each run of exchanges", () => {
  const exchanges = [
    { time: at(15, 5), said: "play music", reply: "Playing" },
    { time: at(15, 6), said: "louder", reply: "Done" },
    { time: at(16, 4), said: "what time is it", reply: "4:04" },
  ];

  assert.deepEqual(rows(exchanges, NOW), [
    ["when", "15:05"], ["said", "play music"], ["reply", "Playing"], ["said", "louder"], ["reply", "Done"],
    ["when", "16:04"], ["said", "what time is it"], ["reply", "4:04"],
  ]);
});

test("only the conversation since a new one was last started", () => {
  const entries = [
    { time: at(15, 1), said: "hello", reply: "Hi" },
    { time: at(15, 2), new_conversation: true },
    { time: at(15, 5), said: "play music", reply: "Playing" },
    { time: at(15, 6), new_conversation: true },
    { time: at(15, 7), said: "louder", reply: "Done" },
  ];

  assert.deepEqual(rows(entries, NOW), [["when", "15:07"], ["said", "louder"], ["reply", "Done"]]);
});

test("nothing yet in a new conversation", () => {
  assert.deepEqual(rows([{ time: at(15, 5), said: "hi", reply: "Hi" }, { time: at(15, 6), new_conversation: true }], NOW), []);
});

test("no rows yet", () => {
  assert.deepEqual(rows([], NOW), []);
});
