"""Loads and unloads a model in Ollama, through its HTTP API."""
import json
import urllib.request


class OllamaClient:
    def __init__(self, base_url: str, timeout: float = 120):
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout

    def loaded_models(self) -> list[str]:
        return [m["name"] for m in self._call("GET", "/api/ps").get("models", [])]

    def load(self, model: str):
        """Loads [model] (if it isn't already) and keeps it loaded until unload()."""
        self._call("POST", "/api/generate", {"model": model, "keep_alive": -1})

    def unload(self, model: str):
        self._call("POST", "/api/generate", {"model": model, "keep_alive": 0})

    def _call(self, method: str, path: str, body: dict | None = None) -> dict:
        request = urllib.request.Request(
            self.base_url + path, method=method,
            data=json.dumps(body).encode() if body is not None else None,
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=self.timeout) as response:
            return json.loads(response.read() or b"{}")
