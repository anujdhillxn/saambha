import os
from itertools import pairwise
from pathlib import Path

import pytest

from saambha.mouse import Pacer, VirtualMouse

UINPUT = Path("/dev/uinput")
DEVICES = Path("/proc/bus/input/devices")  # the kernel's input devices


@pytest.mark.skipif(not os.access(UINPUT, os.W_OK), reason="needs access to /dev/uinput (scripts/install.sh)")
def test_the_mouse_is_on_the_desktop_only_while_open():
    with VirtualMouse(UINPUT, name="Saambha test mouse") as mouse:
        assert 'N: Name="Saambha test mouse"' in DEVICES.read_text()
        mouse.move(0, 0)
        mouse.scroll(0)

    assert "Saambha test mouse" not in DEVICES.read_text()



def paced(events, window=1.0, max_delay=0.15) -> list[float]:
    """When a Pacer plays [events], each (happened on the device, arrived here)."""
    pacer = Pacer(window, max_delay)
    return [round(pacer.play_at(happened, arrived), 3) for happened, arrived in events]


def test_a_steady_stream_plays_as_it_comes():
    # The device's clock is 50s behind ours, and the network takes 5ms.
    events = [(t / 100, 50 + t / 100 + 0.005) for t in range(5)]

    assert paced(events) == [round(arrived, 3) for _, arrived in events]


def test_bunches_are_spaced_out_at_the_pace_they_happened():
    # Moves every 10ms, delivered 4 at a time every 40ms.
    events = [(t * 0.01, 50 + 0.005 + (t // 4 + 1) * 0.04) for t in range(12)]

    played = paced(events)

    assert [round(b - a, 3) for a, b in pairwise(played[4:])] == [0.01] * 7
    assert all(play >= arrived for play, (_, arrived) in zip(played, events, strict=True))


def test_holds_back_no_more_than_max_delay():
    events = [(0.0, 50.0), (0.01, 50.5)]  # the second came half a second late

    assert paced(events, max_delay=0.15)[1] == 50.0 + 0.01 + 0.15  # it's late: played at once


def test_forgets_how_the_network_was_after_the_window():
    late = [(0.0, 50.0), (0.01, 50.2)]
    later = [(5.0, 55.0), (5.01, 55.01)]

    assert paced(late + later)[2:] == [55.0, 55.01]
