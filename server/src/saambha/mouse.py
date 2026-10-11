"""A virtual mouse made with the kernel's uinput, which the phone and watch apps move through /mouse.

The desktop sees it as a real mouse, so it works the same on X11 and on any Wayland desktop, and the
desktop's own pointer acceleration applies to it.
"""
import fcntl
import os
import struct
from collections import deque
from pathlib import Path

EV_SYN, EV_KEY, EV_REL = 0, 1, 2
SYN_REPORT = 0
REL_X, REL_Y, REL_WHEEL = 0, 1, 8
BUTTONS = {"left": 0x110, "right": 0x111}  # BTN_LEFT, BTN_RIGHT
BUS_VIRTUAL = 0x06

INPUT_EVENT = struct.Struct("llHHi")  # struct input_event: a timeval, then type, code and value
UINPUT_SETUP = struct.Struct("4H80sI")  # struct uinput_setup: input_id, name, ff_effects_max


def _ioc(direction: int, number: int, size: int) -> int:
    """An ioctl request number, as the kernel's _IOC macro makes them for uinput ('U')."""
    return direction << 30 | size << 16 | ord("U") << 8 | number


UI_DEV_CREATE = _ioc(0, 1, 0)
UI_DEV_DESTROY = _ioc(0, 2, 0)
UI_DEV_SETUP = _ioc(1, 3, UINPUT_SETUP.size)
UI_SET_EVBIT = _ioc(1, 100, 4)
UI_SET_KEYBIT = _ioc(1, 101, 4)
UI_SET_RELBIT = _ioc(1, 102, 4)


class VirtualMouse:
    """Exists while it's open (`with VirtualMouse(path) as mouse:`): two buttons and a wheel."""

    def __init__(self, path: Path, name: str = "Saambha mouse"):
        self.path = path
        self.name = name
        self._fd = -1

    def __enter__(self) -> "VirtualMouse":
        self._fd = os.open(self.path, os.O_WRONLY | os.O_NONBLOCK)
        try:
            fcntl.ioctl(self._fd, UI_SET_EVBIT, EV_KEY)
            for code in BUTTONS.values():
                fcntl.ioctl(self._fd, UI_SET_KEYBIT, code)
            fcntl.ioctl(self._fd, UI_SET_EVBIT, EV_REL)
            for code in (REL_X, REL_Y, REL_WHEEL):
                fcntl.ioctl(self._fd, UI_SET_RELBIT, code)
            fcntl.ioctl(self._fd, UI_DEV_SETUP, UINPUT_SETUP.pack(BUS_VIRTUAL, 0, 0, 0, self.name.encode(), 0))
            fcntl.ioctl(self._fd, UI_DEV_CREATE)
        except OSError:
            os.close(self._fd)
            raise
        return self

    def __exit__(self, *exc):
        fcntl.ioctl(self._fd, UI_DEV_DESTROY)
        os.close(self._fd)

    def move(self, dx: int, dy: int):
        self._send((EV_REL, REL_X, dx), (EV_REL, REL_Y, dy))

    def click(self, button: str):
        self._send((EV_KEY, BUTTONS[button], 1))
        self._send((EV_KEY, BUTTONS[button], 0))

    def scroll(self, notches: int):
        """Positive scrolls up, like turning a wheel away from you."""
        self._send((EV_REL, REL_WHEEL, notches))

    def _send(self, *events: tuple[int, int, int]):
        """[events] as one report: the desktop takes them in together."""
        os.write(self._fd, b"".join(INPUT_EVENT.pack(0, 0, *e) for e in (*events, (EV_SYN, SYN_REPORT, 0))))


class Pacer:
    """Plays a touchpad's events at the pace they happened, not in the bunches the network brings them
    in (a watch on weak Wi-Fi delivers ~10 at a time). The desktop speeds the pointer up by how fast
    moves come, so bunches make it lurch.

    Each event carries when it happened on the device, in that device's clock. The fastest an event
    has come recently is the network at its best; the slowest, how late the bunching makes some.
    Holding every event back by that spread spaces them out again, so it costs as much lag as the
    network jitters: next to nothing on a good connection, and at most [max_delay].
    """

    def __init__(self, window: float, max_delay: float):
        self.window = window  # seconds of recent events it judges the network by
        self.max_delay = max_delay
        self._recent: deque[tuple[float, float]] = deque()  # (arrived, how long after it happened)

    def play_at(self, happened: float, arrived: float) -> float:
        """When to play an event that [happened] (device seconds) and [arrived] (seconds, our clock)."""
        self._recent.append((arrived, arrived - happened))
        while self._recent[0][0] < arrived - self.window:
            self._recent.popleft()
        delays = [delay for _, delay in self._recent]
        return happened + min(max(delays), min(delays) + self.max_delay)
