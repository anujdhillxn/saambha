import threading

import pytest

from saambha.model.warden import ModelWarden


class FakeOllama:
    def __init__(self):
        self.loaded: set[str] = set()
        self.calls: list[str] = []
        self.down = False

    def loaded_models(self):
        self._up()
        return [f"{m}" for m in self.loaded]

    def load(self, model):
        self._up()
        self.calls.append(f"load {model}")
        self.loaded.add(model)

    def unload(self, model):
        self._up()
        self.calls.append(f"unload {model}")
        self.loaded.discard(model)

    def _up(self):
        if self.down:
            raise ConnectionRefusedError("ollama is down")


class World:
    """What the warden sees: the heavy task (if any), whether a command runs, and the time."""

    def __init__(self):
        self.heavy: str | None = None
        self.busy = False
        self.now = 0.0
        self.cold: list[str | None] = []  # what on_cold was told


@pytest.fixture
def world():
    return World()


@pytest.fixture
def ollama():
    return FakeOllama()


@pytest.fixture
def warden(world, ollama):
    return ModelWarden(ollama, "gemma4:e4b", heavy_task=lambda: world.heavy, busy=lambda: world.busy,
                       on_cold=world.cold.append, interval=10,
                       resume_after=60, clock=lambda: world.now)


def test_keeps_the_model_loaded(warden, ollama, world):
    warden.check()
    world.now += 10
    warden.check()

    assert ollama.loaded == {"gemma4:e4b"}
    assert ollama.calls == ["load gemma4:e4b", "load gemma4:e4b"]  # re-asserted each time
    assert world.cold == ["Loading the model", None]


def test_loads_it_again_if_something_else_unloaded_it(warden, ollama, world):
    warden.check()
    ollama.loaded.clear()

    warden.check()

    assert ollama.loaded == {"gemma4:e4b"}
    assert world.cold == ["Loading the model", None, "Loading the model", None]


def test_unloads_while_something_heavy_runs(warden, ollama, world):
    warden.check()
    world.heavy = "Cyberpunk2077.ex is using 6.0 GB of GPU memory"

    warden.check()

    assert ollama.loaded == set()
    assert world.cold == ["Loading the model", None, "Paused: Cyberpunk2077.ex is using 6.0 GB of GPU memory"]


def test_stays_unloaded_and_quiet_while_it_keeps_running(warden, ollama, world):
    world.heavy = "Cyberpunk2077.ex is using 6.0 GB of GPU memory"
    for _ in range(5):
        warden.check()
        world.now += 10

    assert ollama.calls == []  # never loaded, and nothing to unload
    assert world.cold == ["Paused: Cyberpunk2077.ex is using 6.0 GB of GPU memory"]


def test_command_during_a_game_unloads_the_model_after_it_finishes(warden, ollama, world):
    world.heavy = "Cyberpunk2077.ex is using 6.0 GB of GPU memory"
    warden.check()
    ollama.loaded.add("gemma4:e4b")  # a command loaded it
    world.busy = True

    warden.check()
    assert ollama.loaded == {"gemma4:e4b"}  # not while the command runs

    world.busy = False
    warden.check()
    assert ollama.loaded == set()


def test_reloads_once_things_have_been_calm_for_a_while(warden, ollama, world):
    world.heavy = "Cyberpunk2077.ex is using 6.0 GB of GPU memory"
    warden.check()
    world.heavy = None

    world.now += 50
    warden.check()
    assert ollama.loaded == set()  # not yet: it might come straight back

    world.now += 10
    warden.check()
    assert ollama.loaded == {"gemma4:e4b"}
    assert world.cold == ["Paused: Cyberpunk2077.ex is using 6.0 GB of GPU memory", "Loading the model", None]


def test_a_brief_spike_restarts_the_calm_period(warden, ollama, world):
    world.heavy = "the CPU is busy (load 20 on 24 cores)"
    warden.check()
    world.heavy = None
    world.now += 50
    warden.check()
    world.heavy = "the CPU is busy (load 19 on 24 cores)"
    warden.check()
    world.heavy = None

    world.now += 30
    warden.check()

    assert ollama.loaded == set()


def test_ollama_down_is_logged_once_and_recovers(warden, ollama, world, caplog):
    ollama.down = True
    warden.check()
    warden.check()
    assert len([r for r in caplog.records if "ollama is down" in r.message]) == 1
    assert world.cold == ["Ollama isn't responding"]

    ollama.down = False
    warden.check()
    assert ollama.loaded == {"gemma4:e4b"}
    assert world.cold == ["Ollama isn't responding", "Loading the model", None]


def test_runs_in_the_background_until_stopped(world, ollama):
    checked = threading.Event()

    def heavy_task():
        checked.set()
        return
    warden = ModelWarden(ollama, "m", heavy_task, busy=lambda: False, on_cold=lambda reason: None,
                         interval=0.01, resume_after=0)

    warden.start()
    assert checked.wait(timeout=2)
    warden.stop()

    assert not warden._thread.is_alive()
