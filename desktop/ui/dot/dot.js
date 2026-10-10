// The dot: shows what Saambha is doing, and tells the stage (stage.js) what the pointer does to it,
// a click (the stage opens the chat panel) or a drag (the stage moves it). While Saambha speaks, a
// click stops it instead. While dragged, it stays
// where the drag started, so the pointer's positions here stay steady, and the stage draws it under
// the pointer instead; once dropped, the stage puts this window there.
import { watchStatus } from "../shared/status.js";

const { invoke } = window.__TAURI__.core;
const { emitTo, listen } = window.__TAURI__.event;

const DRAG_FROM = 4; // px the pointer moves with the button down before it's a drag, not a click

const dot = document.getElementById("dot");
let press = null; // where the pointer went down
let moved = null; // how far it's been dragged

dot.addEventListener("pointerdown", (event) => {
  if (event.button !== 0) return;
  press = { x: event.clientX, y: event.clientY };
  dot.setPointerCapture(event.pointerId);
});
dot.addEventListener("pointermove", (event) => {
  if (!press) return;
  const by = { dx: event.clientX - press.x, dy: event.clientY - press.y };
  if (!moved && Math.hypot(by.dx, by.dy) < DRAG_FROM) return;
  moved = by;
  document.body.classList.add("away");
  emitTo("stage", "dot-dragged", { ...by, look: dot.className });
});
dot.addEventListener("pointerup", () => {
  if (moved) emitTo("stage", "dot-dropped", moved);
  else if (press && dot.classList.contains("speaking")) invoke("stop_speaking").catch(() => {});
  else if (press) emitTo("stage", "toggle-panel");
  press = moved = null;
});
dot.addEventListener("pointercancel", () => {
  if (moved) emitTo("stage", "dot-dropped", moved);
  press = moved = null;
});
listen("dot-placed", () => document.body.classList.remove("away"));

watchStatus((status) => {
  const speaking = status.speaking && status.state === "working" && !status.task;
  dot.className = `dot ${status.state}` + (speaking ? " speaking" : "");
  dot.title = status.line;
});
