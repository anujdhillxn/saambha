// What the chat panel lists: the current conversation as rows of times, commands and replies.

const RUN_GAP_S = 600; // exchanges this close together share one time

/** When something was said: the time today, else the date too. */
export function when(seconds, now = new Date()) {
  const then = new Date(seconds * 1000);
  const time = then.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
  if (then.toDateString() === now.toDateString()) return time;
  return `${then.toLocaleDateString([], { month: "short", day: "numeric" })}, ${time}`;
}

/**
 * [kind, text] rows of the exchanges since a new conversation was last started, kind being "when",
 * "said" or "reply": a time starts each run of exchanges.
 */
export function rows(entries, now = new Date()) {
  const shown = [];
  let last = null;
  for (const entry of entries.slice(entries.findLastIndex((e) => e.new_conversation) + 1)) {
    if (last === null || entry.time - last >= RUN_GAP_S) shown.push(["when", when(entry.time, now)]);
    shown.push(["said", entry.said], ["reply", entry.reply]);
    last = entry.time;
  }
  return shown;
}
