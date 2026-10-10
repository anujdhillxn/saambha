// The stage: see-through over the whole screen, below the dot. It knows where the dot is and puts
// the dot's window there, draws the dot under the pointer while it's dragged (dot.js says how far),
// and shows the chat panel, growing out of the dot. It takes clicks only while the panel is open,
// so that a click away from the panel puts it away; otherwise they go to the windows below.
import { OFFLINE, watchStatus } from "../shared/status.js";
import { keepOn, placeDot, placePanel } from "./layout.js";
import * as panel from "./panel.js";

const { invoke } = window.__TAURI__.core;
const { emitTo, listen } = window.__TAURI__.event;

const EDGE = 8; // the dot's first distance from the screen's corner
const MARGIN = 8; // the panel's least distance from the screen's edges
const GAP = 4; // between the dot and the panel
const SETTLE_MS = 200; // after a drop, while the dot fades back in where dropped
const SAVED = "dot"; // where the dot was left, in localStorage

const card = document.getElementById("card");
const dragged = document.getElementById("dragged");
let at; // the dot's box
let dragFrom = null; // where the dot was when the drag started
let settling = null;
let open = false;
let status = OFFLINE;
let asked = null; // the device last asking to connect

const screen = () => ({ width: innerWidth, height: innerHeight });

function saved() {
  try {
    return JSON.parse(localStorage.getItem(SAVED));
  } catch {
    return null;
  }
}

function save() {
  try {
    localStorage.setItem(SAVED, JSON.stringify({ x: at.x, y: at.y }));
  } catch {
    // it starts in the corner next time
  }
}

function placeDotWindow(box) {
  at = box;
  const { width, height } = screen();
  const [x, y] = [Math.round(box.x), Math.round(box.y)];
  return invoke("place_dot", { margins: [x, y, width - x - box.width, height - y - box.height] });
}

function takeInput() {
  invoke("set_input", { parts: open ? [{ x: 0, y: 0, ...screen() }] : [], keyboard: open });
}

function openPanel() {
  const place = placePanel(screen(), at, { width: card.offsetWidth, height: card.offsetHeight }, GAP, MARGIN);
  card.style.translate = `${place.x}px ${place.y}px`;
  card.style.transformOrigin = `${place.fromRight ? "right" : "left"} ${place.up ? "bottom" : "top"}`;
  card.classList.add("open");
  open = true;
  takeInput();
  panel.show(status);
  panel.focus();
}

function closePanel() {
  if (!open) return;
  card.classList.remove("open");
  open = false;
  takeInput();
}

function draggedTo({ dx, dy }) {
  return keepOn(screen(), { ...dragFrom, x: dragFrom.x + dx, y: dragFrom.y + dy }, 0);
}

listen("dot-dragged", ({ payload }) => {
  if (!dragFrom) {
    dragFrom = at;
    clearTimeout(settling);
    closePanel();
  }
  const box = draggedTo(payload);
  dragged.firstElementChild.className = payload.look;
  dragged.style.translate = `${box.x}px ${box.y}px`;
  dragged.classList.add("shown");
});
listen("dot-dropped", async ({ payload }) => {
  await placeDotWindow(draggedTo(payload));
  dragFrom = null;
  save();
  emitTo("dot", "dot-placed");
  settling = setTimeout(() => dragged.classList.remove("shown"), SETTLE_MS);
});
listen("toggle-panel", () => (open ? closePanel() : openPanel()));
addEventListener("pointerdown", (event) => {
  if (open && !card.contains(event.target)) closePanel();
});
addEventListener("keydown", (event) => event.key === "Escape" && closePanel());
panel.wire(closePanel);

// The stage takes its screen's size once shown, and again when that changes.
function place() {
  closePanel();
  placeDotWindow(placeDot(screen(), saved(), dragged.offsetWidth, EDGE, 0));
}
addEventListener("resize", place);
place();

watchStatus((now) => {
  status = now;
  if (open) panel.show(status);
  else if (status.pairing && status.pairing !== asked) openPanel(); // to allow or deny it there
  asked = status.pairing;
});
