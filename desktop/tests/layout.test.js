// Where the dot and the chat panel go (ui/stage/layout.js). Run: node --test
import assert from "node:assert/strict";
import { test } from "node:test";

import { keepOn, placeDot, placePanel } from "../ui/stage/layout.js";

const SCREEN = { width: 1920, height: 1080 };
const DOT = 40;
const PANEL = { width: 400, height: 740 };

test("the dot starts in the bottom-right corner", () => {
  assert.deepEqual(placeDot(SCREEN, null, DOT, 24, 8), { x: 1920 - 40 - 24, y: 1080 - 40 - 24, width: 40, height: 40 });
});

test("the dot goes back where it was left", () => {
  assert.deepEqual(placeDot(SCREEN, { x: 500, y: 300 }, DOT, 24, 8), { x: 500, y: 300, width: 40, height: 40 });
});

test("a dot left off a smaller screen comes back on", () => {
  const at = placeDot({ width: 1280, height: 720 }, { x: 1800, y: -20 }, DOT, 24, 8);

  assert.deepEqual([at.x, at.y], [1280 - 40 - 8, 8]);
});

test("a box on screen stays put", () => {
  const box = { x: 10, y: 10, width: 100, height: 100 };

  assert.deepEqual(keepOn(SCREEN, box, 8), box);
});

test("the panel opens up and left from a dot in the bottom right", () => {
  const dot = placeDot(SCREEN, null, DOT, 24, 8);

  const panel = placePanel(SCREEN, dot, PANEL, 12, 8);

  assert.ok(panel.up && panel.fromRight);
  assert.equal(panel.x + panel.width, dot.x + dot.width); // right edges line up
  assert.equal(panel.y + panel.height, dot.y - 12); // just above the dot
});

test("the panel opens down and right from a dot in the top left", () => {
  const panel = placePanel(SCREEN, { x: 20, y: 20, width: 40, height: 40 }, PANEL, 12, 8);

  assert.deepEqual(panel, { x: 20, y: 72, ...PANEL, up: false, fromRight: false });
});

test("the panel stays on screen beside a dot halfway down", () => {
  const panel = placePanel(SCREEN, { x: 30, y: 600, width: 40, height: 40 }, PANEL, 12, 8);

  assert.deepEqual(keepOn(SCREEN, panel, 8), panel);
});
