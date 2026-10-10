"""A fake OpenAI-compatible model server for running the real `pi` in tests.

It streams /v1/chat/completions replies as SSE:
  - the user mentions "music": first a `read` tool call for the spotify skill's SKILL.md,
    then (once the tool result is back) "Playing music"
  - the user says "run <command>": a `bash` tool call for <command>, then "Ran it"
  - anything else: "You said: <last user message>"

Every request body is appended to <state>/llm_requests.jsonl.

Usage: llm_server.py <state dir>   (prints the port it listens on)
"""
import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def text_of(message) -> str:
    content = message.get("content") or ""
    if isinstance(content, list):
        return "".join(part.get("text", "") for part in content if isinstance(part, dict))
    return content


def reply_for(messages: list[dict]) -> dict:
    """{"text": ...} or {"tool": name, "args": {...}} for the conversation so far."""
    user = next(m for m in reversed(messages) if m["role"] == "user")
    after_user = messages[messages.index(user) + 1:]
    asked = text_of(user).strip()
    if any(m["role"] == "tool" for m in after_user):
        return {"text": "Playing music" if "music" in asked else "Ran it"}
    if "music" in asked:
        return {"tool": "read", "args": {"path": "skills/spotify/SKILL.md"}}
    if asked.startswith("run "):
        return {"tool": "bash", "args": {"command": asked[4:]}}
    return {"text": f"You said: {asked}"}


def chunk(delta: dict, finish: str | None = None) -> bytes:
    body = {"id": "fake", "object": "chat.completion.chunk", "created": 0, "model": "fake-model",
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
    if finish:
        body["usage"] = {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    return f"data: {json.dumps(body)}\n\n".encode()


class Handler(BaseHTTPRequestHandler):
    state: Path

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        with (self.state / "llm_requests.jsonl").open("a") as f:
            f.write(json.dumps(body) + "\n")

        reply = reply_for(body["messages"])
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        if "text" in reply:
            self.wfile.write(chunk({"role": "assistant", "content": reply["text"]}))
            self.wfile.write(chunk({}, "stop"))
        else:
            call = {"index": 0, "id": "call_1", "type": "function",
                    "function": {"name": reply["tool"], "arguments": json.dumps(reply["args"])}}
            self.wfile.write(chunk({"role": "assistant", "tool_calls": [call]}))
            self.wfile.write(chunk({}, "tool_calls"))
        self.wfile.write(b"data: [DONE]\n\n")

    def log_message(self, *args):
        pass


def start(state: Path) -> ThreadingHTTPServer:
    """Starts the server on a free port in a background thread."""
    handler = type("BoundHandler", (Handler,), {"state": state})
    server = ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


if __name__ == "__main__":
    server = start(Path(sys.argv[1]))
    print(server.server_address[1], flush=True)
    threading.Event().wait()
