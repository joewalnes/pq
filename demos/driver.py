#!/usr/bin/env python3
"""
Drive a TUI application for asciinema recording.

Spawns a command on a PTY, forwards output to stdout in real-time
via a background thread, and injects keystrokes on a schedule.

Usage as a library:
    from driver import Session
    s = Session("./target/release/pq", ["view", "data.parquet"])
    s.wait(2.0)
    s.type("j", repeat=5, delay=0.3)
    s.pause(0.5)
    s.key("q")
    s.run()

Usage from command line (runs a demo script):
    python3 driver.py <script.py>
"""

import importlib.util
import os
import pty
import signal
import select
import struct
import sys
import fcntl
import termios
import threading
import time


class Session:
    """Scriptable TUI session that records to stdout."""

    def __init__(self, binary, args, cols=120, rows=35, cwd=None):
        self.binary = binary
        self.args = args
        self.cols = cols
        self.rows = rows
        self.cwd = cwd
        self._steps = []

    # ── Script DSL ────────────────────────────────────────────────────

    def wait(self, seconds):
        """Pause before the next action."""
        self._steps.append(("wait", seconds))
        return self

    def key(self, k, delay=0.3):
        """Send a single keystroke."""
        self._steps.append(("key", k, delay))
        return self

    def type(self, keys, repeat=1, delay=0.3):
        """Send a key multiple times, or type a string character by character."""
        if repeat > 1:
            for _ in range(repeat):
                self._steps.append(("key", keys, delay))
        else:
            for c in keys:
                self._steps.append(("key", c, delay))
        return self

    def enter(self, delay=0.3):
        """Send Enter."""
        self._steps.append(("key", "\r", delay))
        return self

    def tab(self, delay=0.3):
        """Send Tab."""
        self._steps.append(("key", "\t", delay))
        return self

    def ctrl_d(self, delay=0.3):
        """Send Ctrl+D (EOF)."""
        self._steps.append(("key", "\x04", delay))
        return self

    # ── Execution ─────────────────────────────────────────────────────

    def run(self):
        """Execute the script: spawn the process, replay steps, quit."""
        master_fd, slave_fd = pty.openpty()
        winsize = struct.pack("HHHH", self.rows, self.cols, 0, 0)
        fcntl.ioctl(master_fd, termios.TIOCSWINSZ, winsize)

        pid = os.fork()
        if pid == 0:
            os.close(master_fd)
            os.setsid()
            fcntl.ioctl(slave_fd, termios.TIOCSCTTY, 0)
            os.dup2(slave_fd, 0)
            os.dup2(slave_fd, 1)
            os.dup2(slave_fd, 2)
            if slave_fd > 2:
                os.close(slave_fd)
            os.environ["TERM"] = "xterm-256color"
            if self.cwd:
                os.chdir(self.cwd)
            os.execvp(self.binary, [self.binary] + self.args)

        os.close(slave_fd)

        # Background thread: continuously drain PTY output to stdout
        stop = threading.Event()
        pump = threading.Thread(
            target=self._output_pump, args=(master_fd, stop), daemon=True
        )
        pump.start()

        try:
            self._replay(master_fd, pid)
        finally:
            stop.set()
            pump.join(timeout=2)
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            try:
                os.waitpid(pid, 0)
            except ChildProcessError:
                pass
            try:
                os.close(master_fd)
            except OSError:
                pass

    def _replay(self, master_fd, pid):
        for step in self._steps:
            if step[0] == "wait":
                time.sleep(step[1])
            elif step[0] == "key":
                _, k, delay = step
                data = k.encode() if isinstance(k, str) else k
                os.write(master_fd, data)
                time.sleep(delay)

        # Wait for child to exit
        for _ in range(20):
            result = os.waitpid(pid, os.WNOHANG)
            if result != (0, 0):
                return
            time.sleep(0.1)

    @staticmethod
    def _output_pump(master_fd, stop_event):
        while not stop_event.is_set():
            try:
                r, _, _ = select.select([master_fd], [], [], 0.05)
                if r:
                    data = os.read(master_fd, 65536)
                    if not data:
                        break
                    os.write(1, data)
            except OSError:
                break


def load_and_run(script_path):
    """Load a demo script module and call its demo() function."""
    spec = importlib.util.spec_from_file_location("demo_script", script_path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    if not hasattr(mod, "demo"):
        print(f"Error: {script_path} must define a demo() function", file=sys.stderr)
        sys.exit(1)
    mod.demo()


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <script.py>", file=sys.stderr)
        sys.exit(1)
    load_and_run(sys.argv[1])
