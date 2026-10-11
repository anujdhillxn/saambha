// The chat panel's contents: what Saambha is doing, the conversation, a box to type a command in,
// and what else can be done about it.
import { rows } from "./chat.js";

const { invoke } = window.__TAURI__.core;

const SHOWN = 50; // entries of the history

const $ = (id) => document.getElementById(id);
let shownHistory = "";

/** Wires up the panel's buttons; [close] puts it away. */
export function wire(close) {
  $("close").onclick = close;
  $("new").onclick = () => invoke("new_conversation").then(() => show(), () => {});
  $("allow").onclick = () => invoke("answer_pairing", { allow: true }).catch(() => {});
  $("deny").onclick = () => invoke("answer_pairing", { allow: false }).catch(() => {});
  $("stop").onclick = () => invoke("stop_speaking").catch(() => {});
  $("logs").onclick = () => invoke("open_logs");
  $("compose").onsubmit = (event) => {
    event.preventDefault();
    send($("text").value.trim());
  };
}

/** Puts the keyboard in the box to type a command in. */
export function focus() {
  $("text").focus();
}

/** Runs [text] as a command, showing it at once; the reply comes with the history. */
function send(text) {
  if (!text) return;
  $("text").value = "";
  $("chat").querySelector(".empty")?.remove();
  const pending = row("said pending", text);
  $("chat").append(pending);
  pending.scrollIntoView();
  invoke("send_command", { text }).then(() => show(), () => {
    pending.remove(); // it didn't go: back in the box, to send again
    $("text").value = text;
  });
}

function row(kind, text) {
  const row = document.createElement("div");
  row.className = kind;
  row.textContent = text;
  return row;
}

/** Shows [status] (else keeps what's shown) and the latest history. */
export async function show(status) {
  if (status) {
    $("light").className = status.state;
    $("line").textContent = status.state === "ready" ? "" : status.line;
    $("pairing").hidden = !status.pairing;
    $("asking").textContent = status.pairing ? `${status.pairing} wants to connect` : "";
    $("stop").hidden = !status.speaking;
  }
  try {
    showChat(await invoke("history", { limit: SHOWN }));
  } catch {
    // keep what's shown
  }
}

function showChat(entries) {
  const key = JSON.stringify(entries);
  if (key === shownHistory) return;
  shownHistory = key;
  const chat = $("chat");
  const shown = rows(entries);
  chat.replaceChildren();
  if (!shown.length) {
    const empty = document.createElement("p");
    empty.className = "empty";
    empty.textContent = "Your commands and Saambha's replies show up here";
    chat.append(empty);
  }
  chat.append(...shown.map(([kind, text]) => row(kind, text)));
  chat.scrollTop = chat.scrollHeight;
}
