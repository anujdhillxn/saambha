from pathlib import Path

import uvicorn

from saambha.logs import configure_logging
from saambha.main import load_settings

# This process only supervises auto-reload; the server itself runs in a worker that
# builds the app with saambha.main.build_app, which sets up its own logging (with the log file).
settings = load_settings()
configure_logging(settings, to_file=False)
uvicorn.run("saambha.main:build_app", factory=True, host="::", port=settings.port,
            reload=True, reload_delay=10.0, reload_dirs=[str(Path(__file__).parent)],
            ssl_keyfile=str(settings.key_file), ssl_certfile=str(settings.cert_file),
            timeout_graceful_shutdown=settings.shutdown_wait, log_config=None)
