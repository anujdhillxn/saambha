"""What a process and its descendants use, from /proc: for the benchmarks."""
import os
import time
from dataclasses import dataclass
from pathlib import Path

TICKS_PER_S = os.sysconf("SC_CLK_TCK")


@dataclass(frozen=True)
class Usage:
    cpu: float  # of one core
    wakeups: float  # a second: every thread's context switches
    memory_mb: float  # PSS: shared pages split between the processes sharing them


def processes(pid: int) -> list[Path]:
    """[pid] and its descendants."""
    proc = Path("/proc") / str(pid)
    children = " ".join(p.read_text() for p in proc.glob("task/*/children")).split()
    return [proc, *(p for child in children for p in processes(int(child)))]


def counters(pid: int) -> tuple[dict[str, float], dict[str, int]]:
    """The CPU seconds and context switches (wakeups) [pid] and its descendants have had so far, each
    process's and thread's by its id: what one that's gone had is gone with it."""
    cpu, switches = {}, {}
    for proc in processes(pid):
        fields = (proc / "stat").read_text().rsplit(") ", 1)[1].split()
        cpu[proc.name] = (int(fields[11]) + int(fields[12])) / TICKS_PER_S  # utime, stime
        for task in proc.glob("task/*"):
            switches[task.name] = sum(int(line.split()[1]) for line in (task / "status").read_text().splitlines()
                                      if "ctxt_switches:" in line)  # voluntary and not
    return cpu, switches


def growth(before: dict, after: dict) -> float:
    """How much the counters in [after] grew since [before] (ones new since then, from nothing)."""
    return sum(n - before.get(id_, 0) for id_, n in after.items())


def measure(pid: int, seconds: float) -> Usage:
    """What [pid] and its descendants use over the next [seconds]."""
    cpu, switches = counters(pid)
    time.sleep(seconds)
    cpu_after, switches_after = counters(pid)
    pss_kb = sum(int(line.split()[1]) for proc in processes(pid)
                 for line in (proc / "smaps_rollup").read_text().splitlines() if line.startswith("Pss:"))
    return Usage(growth(cpu, cpu_after) / seconds, growth(switches, switches_after) / seconds, pss_kb / 1024)
