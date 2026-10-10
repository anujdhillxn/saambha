import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

from saambha.model.ollama import OllamaClient


@pytest.fixture
def fake_ollama():
    """A fake Ollama API that records requests; /api/ps lists one loaded model."""
    requests = []

    class Handler(BaseHTTPRequestHandler):
        def _reply(self, body):
            data = json.dumps(body).encode()
            self.send_response(200)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            requests.append(("GET", self.path, None))
            self._reply({"models": [{"name": "gemma4:e4b", "size": 1}]})

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            requests.append(("POST", self.path, body))
            self._reply({"done": True})

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    yield f"http://127.0.0.1:{server.server_address[1]}/", requests
    server.shutdown()


def test_loaded_models(fake_ollama):
    url, requests = fake_ollama

    assert OllamaClient(url).loaded_models() == ["gemma4:e4b"]
    assert requests == [("GET", "/api/ps", None)]


def test_load_keeps_the_model_loaded(fake_ollama):
    url, requests = fake_ollama

    OllamaClient(url).load("gemma4:e4b")

    assert requests == [("POST", "/api/generate", {"model": "gemma4:e4b", "keep_alive": -1})]


def test_unload(fake_ollama):
    url, requests = fake_ollama

    OllamaClient(url).unload("gemma4:e4b")

    assert requests == [("POST", "/api/generate", {"model": "gemma4:e4b", "keep_alive": 0})]


def test_unreachable():
    with pytest.raises(OSError):
        OllamaClient("http://127.0.0.1:1", timeout=2).loaded_models()
