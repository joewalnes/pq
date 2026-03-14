"""TUI viewer demo — starts from a shell prompt, launches the viewer."""

import subprocess, sys, os

sys.path.insert(0, os.path.dirname(__file__))
from driver import Session

PQ = os.environ.get("PQ", "./target/release/pq")
DATA_DIR = os.path.abspath("demos/.data")
DATA = os.path.join(DATA_DIR, "test_data.parquet")


def setup():
    """Create a demo parquet file (subset of test_data.parquet)."""
    os.makedirs(DATA_DIR, exist_ok=True)
    if not os.path.exists(DATA):
        subprocess.run(
            [PQ, "sql",
             "SELECT * FROM './test_data.parquet' LIMIT 100000",
             "-o", DATA],
            check=True, capture_output=True,
        )


def demo():
    setup()

    pq_abs = os.path.abspath(PQ)
    pq_dir = os.path.dirname(pq_abs)

    s = Session("env", [
        "-i",
        "PS1=$ ",
        "TERM=xterm-256color",
        f"PATH={pq_dir}:{os.environ.get('PATH', '')}",
        "bash", "--norc", "--noprofile",
    ], cwd=DATA_DIR)

    # Wait for shell prompt
    s.wait(0.8)

    # Type the command character by character
    s.type("pq test_data.parquet", delay=0.06)
    s.wait(0.3)
    s.enter(delay=2.0)   # run it — wait for TUI to render

    # ── Navigate the TUI ──────────────────────────────────────────

    # Scroll down through rows
    s.type("j", repeat=8, delay=0.3)
    s.wait(0.6)

    # Scroll right to reveal more columns
    s.type("l", repeat=5, delay=0.35)
    s.wait(0.6)

    # Scroll back left
    s.type("h", repeat=5, delay=0.25)
    s.wait(0.4)

    # Switch to V-Split layout
    s.key("v", delay=1.2)

    # Scroll in v-split — detail panel updates
    s.type("j", repeat=5, delay=0.5)
    s.wait(0.6)

    # Focus detail panel, scroll it
    s.enter(delay=0.5)
    s.type("j", repeat=6, delay=0.35)
    s.wait(0.6)

    # Back to row list
    s.enter(delay=0.5)

    # Switch to List-only layout
    s.key("v", delay=1.0)

    # Filter: type /shipped
    s.key("/", delay=0.5)
    s.type("shipped", delay=0.12)
    s.wait(0.6)
    s.enter(delay=1.0)

    # Scroll filtered results
    s.type("j", repeat=4, delay=0.35)
    s.wait(0.6)

    # Switch to Schema tab
    s.tab(delay=1.2)

    # Scroll through schema
    s.type("j", repeat=12, delay=0.3)
    s.wait(1.2)

    # Quit TUI — end recording while TUI is still visible
    # (the last frame is held by agg's --last-frame-duration)
    s.key("q", delay=0.1)

    s.run()
