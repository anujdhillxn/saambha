from saambha.model.activity import GpuMemoryHog, HeavyTaskDetector, HighCpuLoad, LowMemory, available_memory_mb

# GpuMemoryHog

DESKTOP = [("Xorg", 225), ("gnome-shell", 42), ("chrome --type=g", 21), ("llama-server", 4856)]


def test_desktop_apps_and_the_model_server_are_not_heavy():
    assert GpuMemoryHog(1500, read_usage=lambda: DESKTOP)() is None


def test_process_using_lots_of_gpu_memory():
    usage = DESKTOP + [("Cyberpunk2077.ex", 6144)]

    assert GpuMemoryHog(1500, read_usage=lambda: usage)() == "Cyberpunk2077.ex is using 6.0 GB of GPU memory"


def test_old_ollama_runner_name_is_ignored_too():
    assert GpuMemoryHog(1500, read_usage=lambda: [("ollama", 5000)])() is None


def test_display_server_holding_the_windows_is_not_heavy():
    assert GpuMemoryHog(1500, read_usage=lambda: [("Xorg", 1993), ("gnome-shell", 1600)])() is None


# LowMemory, HighCpuLoad

def test_low_memory():
    assert LowMemory(1500, read_available=lambda: 1024)() == "only 1.0 GB of RAM is free"
    assert LowMemory(1500, read_available=lambda: 6000)() is None


def test_reads_real_free_memory():
    assert available_memory_mb() > 0


def test_cpu_load_is_per_core():
    assert HighCpuLoad(0.75, read_load=lambda: 20.0, cores=24)() == "the CPU is busy (load 20 on 24 cores)"
    assert HighCpuLoad(0.75, read_load=lambda: 17.0, cores=24)() is None


# HeavyTaskDetector

def test_first_busy_signal_wins():
    detector = HeavyTaskDetector([lambda: None, lambda: "a game is running", lambda: "the CPU is busy"])

    assert detector.heavy_task() == "a game is running"


def test_nothing_heavy():
    assert HeavyTaskDetector([lambda: None, lambda: None]).heavy_task() is None


def test_broken_signal_does_not_hide_the_others(caplog):
    def no_nvidia_smi():
        raise FileNotFoundError("nvidia-smi")
    detector = HeavyTaskDetector([no_nvidia_smi, lambda: "a game is running"])

    assert detector.heavy_task() == "a game is running"
    assert detector.heavy_task() == "a game is running"
    assert len([r for r in caplog.records if "check failed" in r.message]) == 1  # logged once
