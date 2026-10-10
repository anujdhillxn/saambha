"""Is the user running something heavy (a game, a render, a training job...)? Each signal
returns a short reason when it sees one, or None; HeavyTaskDetector asks them all."""
import logging
import os
import subprocess
from collections.abc import Callable, Sequence
from pathlib import Path

log = logging.getLogger(__name__)

Signal = Callable[[], str | None]


class HeavyTaskDetector:
    def __init__(self, signals: Sequence[Signal]):
        self.signals = signals
        self._broken: set[int] = set()

    def heavy_task(self) -> str | None:
        """Why the machine is busy (e.g. "the CPU is busy (load 20 on 24 cores)"), or None.

        A signal that fails (e.g. no nvidia-smi) counts as "not busy", logged once."""
        for i, signal in enumerate(self.signals):
            try:
                reason = signal()
            except Exception as e:
                if i not in self._broken:
                    log.warning(f"{type(signal).__name__} check failed: {e}")
                    self._broken.add(i)
                continue
            self._broken.discard(i)
            if reason:
                return reason
        return None


def nvidia_gpu_memory() -> list[tuple[str, int]]:
    """(process name, MB of GPU memory) for every process on the NVIDIA GPU, graphics or compute."""
    out = subprocess.run(["nvidia-smi", "pmon", "-c", "1", "-s", "m"],
                         capture_output=True, text=True, timeout=10, check=True).stdout
    usage = []
    for line in out.splitlines():
        if line.startswith("#"):
            continue
        # gpu pid type fb ccpm command...
        fields = line.split(None, 5)
        if len(fields) == 6 and fields[3].isdigit():
            usage.append((fields[5].strip(), int(fields[3])))
    return usage


# The model server, and the display servers: on X11, Xorg holds every window's textures, so it grows
# with ordinary desktop use (games render directly and show up under their own name).
NOT_HEAVY = ("ollama", "llama-server", "Xorg", "Xwayland", "gnome-shell", "cosmic-comp", "kwin")


class GpuMemoryHog:
    """A process (other than those in [ignore]) using at least [min_mb] of GPU memory.
    Desktop apps use a few hundred MB at most; games, renders and ML jobs use gigabytes."""

    def __init__(self, min_mb: int, ignore: Sequence[str] = NOT_HEAVY,
                 read_usage: Callable[[], list[tuple[str, int]]] = nvidia_gpu_memory):
        self.min_mb = min_mb
        self.ignore = ignore
        self.read_usage = read_usage

    def __call__(self) -> str | None:
        for name, mb in self.read_usage():
            if mb >= self.min_mb and not name.startswith(tuple(self.ignore)):
                return f"{name} is using {mb / 1024:.1f} GB of GPU memory"
        return None


def available_memory_mb(meminfo: Path = Path("/proc/meminfo")) -> int:
    for line in meminfo.read_text().splitlines():
        if line.startswith("MemAvailable:"):
            return int(line.split()[1]) // 1024
    raise ValueError("no MemAvailable in /proc/meminfo")


class LowMemory:
    def __init__(self, min_available_mb: int, read_available: Callable[[], int] = available_memory_mb):
        self.min_available_mb = min_available_mb
        self.read_available = read_available

    def __call__(self) -> str | None:
        available = self.read_available()
        if available < self.min_available_mb:
            return f"only {available / 1024:.1f} GB of RAM is free"
        return None


class HighCpuLoad:
    """The 1-minute load average above [max_per_core] times the number of cores."""

    def __init__(self, max_per_core: float, read_load: Callable[[], float] = lambda: os.getloadavg()[0],
                 cores: int = os.cpu_count() or 1):
        self.max_per_core = max_per_core
        self.read_load = read_load
        self.cores = cores

    def __call__(self) -> str | None:
        load = self.read_load()
        if load > self.max_per_core * self.cores:
            return f"the CPU is busy (load {load:.0f} on {self.cores} cores)"
        return None
