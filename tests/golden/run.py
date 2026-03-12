#!/usr/bin/env python3
"""Golden test runner for pq.

Parses markdown files containing `file:<path>` and `console` fenced code blocks,
creates files, executes commands, and compares output against expected results.

Usage:
    python3 tests/golden/run.py                          # run all .md files
    python3 tests/golden/run.py tests/golden/tutorials/  # run one directory
    python3 tests/golden/run.py getting-started.md        # run one file
    python3 tests/golden/run.py --update                  # overwrite expected output in-place
"""

import argparse
import difflib
import os
import subprocess
import sys
import tempfile
from pathlib import Path


def find_pq_binary():
    """Resolve the pq binary path."""
    if "PQ" in os.environ:
        return str(Path(os.environ["PQ"]).resolve())
    # Try to build and use target/debug/pq
    repo_root = Path(__file__).resolve().parent.parent.parent
    binary = repo_root / "target" / "debug" / "pq"
    if not binary.exists():
        print("Building pq...", file=sys.stderr)
        result = subprocess.run(
            ["cargo", "build"],
            cwd=repo_root,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            print(f"cargo build failed:\n{result.stderr}", file=sys.stderr)
            sys.exit(1)
    if not binary.exists():
        print(f"pq binary not found at {binary}", file=sys.stderr)
        sys.exit(1)
    return str(binary)


def parse_blocks(lines):
    """Parse markdown lines into a list of blocks.

    Returns list of dicts:
        {"type": "file", "path": str, "content": str, "line": int}
        {"type": "console", "commands": [{"cmd": str, "expected": str, "exit_code": int}],
         "line": int, "end_line": int}
    """
    blocks = []
    i = 0
    while i < len(lines):
        line = lines[i]
        stripped = line.strip()

        # Match ```file:<path> or ```console
        if stripped.startswith("```file:"):
            path = stripped[len("```file:"):].strip()
            content_lines = []
            i += 1
            while i < len(lines) and lines[i].strip() != "```":
                content_lines.append(lines[i])
                i += 1
            blocks.append({
                "type": "file",
                "path": path,
                "content": "\n".join(content_lines),
                "line": i - len(content_lines),  # start of content
            })
            i += 1  # skip closing ```
            continue

        if stripped == "```console":
            block_start = i
            i += 1
            raw_lines = []
            while i < len(lines) and lines[i].strip() != "```":
                raw_lines.append(lines[i])
                i += 1
            block_end = i
            i += 1  # skip closing ```

            # Parse commands and expected output from raw_lines
            commands = []
            j = 0
            while j < len(raw_lines):
                raw = raw_lines[j]
                if raw.startswith("$ "):
                    cmd_line = raw[2:]
                    # Check for exit code annotation
                    exit_code = 0
                    if "# [exit:" in cmd_line:
                        idx = cmd_line.index("# [exit:")
                        exit_str = cmd_line[idx + len("# [exit:"):].rstrip("]").strip()
                        exit_code = int(exit_str)
                        cmd_line = cmd_line[:idx].strip()

                    j += 1
                    expected_lines = []
                    while j < len(raw_lines) and not raw_lines[j].startswith("$ "):
                        expected_lines.append(raw_lines[j])
                        j += 1

                    # Strip trailing empty lines from expected output
                    while expected_lines and expected_lines[-1].strip() == "":
                        expected_lines.pop()

                    commands.append({
                        "cmd": cmd_line,
                        "expected": "\n".join(expected_lines) if expected_lines else None,
                        "exit_code": exit_code,
                    })
                else:
                    j += 1

            blocks.append({
                "type": "console",
                "commands": commands,
                "line": block_start,
                "end_line": block_end,
            })
            continue

        i += 1

    return blocks


def run_command(cmd, cwd, env, timeout=30):
    """Execute a shell command and return (output, exit_code)."""
    result = subprocess.run(
        ["sh", "-c", cmd],
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        env=env,
        timeout=timeout,
    )
    output = result.stdout.decode("utf-8", errors="replace")
    # Strip trailing newline for comparison
    if output.endswith("\n"):
        output = output[:-1]
    return output, result.returncode


def strip_trailing_ws(text):
    """Strip trailing whitespace from each line."""
    return "\n".join(line.rstrip() for line in text.split("\n"))


def run_file(md_path, pq_binary, update=False):
    """Run a single markdown file. Returns (passed, failed, updates)."""
    with open(md_path, "r") as f:
        lines = f.readlines()

    # Normalize: keep newlines for rewriting but strip for parsing
    raw_lines = [line.rstrip("\n") for line in lines]

    # Detect if this is a network test (longer timeouts for remote files)
    full_text = "\n".join(raw_lines[:20])
    cmd_timeout = 120 if "<!-- requires: network -->" in full_text else 30

    blocks = parse_blocks(raw_lines)
    passed = 0
    failed = 0
    updates = []  # (block, command_index, actual_output) for --update mode

    with tempfile.TemporaryDirectory() as tmpdir:
        env = os.environ.copy()
        env["NO_COLOR"] = "1"
        env["COLUMNS"] = "200"
        env["TERM"] = "dumb"
        # Put pq binary dir on PATH so `pq` works directly
        env["PATH"] = str(Path(pq_binary).parent) + ":" + env.get("PATH", "")

        for block in blocks:
            if block["type"] == "file":
                filepath = Path(tmpdir) / block["path"]
                filepath.parent.mkdir(parents=True, exist_ok=True)
                filepath.write_text(block["content"])
                continue

            if block["type"] == "console":
                for ci, command in enumerate(block["commands"]):
                    cmd = command["cmd"]
                    try:
                        actual_output, actual_exit = run_command(
                            cmd, tmpdir, env, timeout=cmd_timeout,
                        )
                    except subprocess.TimeoutExpired:
                        line_num = block["line"] + 2  # approximate
                        print(f"  TIMEOUT: {md_path}:{line_num}: $ {cmd}")
                        failed += 1
                        continue

                    # Check exit code
                    if actual_exit != command["exit_code"]:
                        line_num = block["line"] + 2
                        print(
                            f"  FAIL (exit code): {md_path}:{line_num}: $ {cmd}\n"
                            f"    expected exit {command['exit_code']}, got {actual_exit}"
                        )
                        if actual_output:
                            print(f"    output: {actual_output[:200]}")
                        failed += 1
                        continue

                    # Check output if expected
                    if command["expected"] is not None:
                        expected = strip_trailing_ws(command["expected"])
                        actual = strip_trailing_ws(actual_output)

                        if expected == actual:
                            passed += 1
                        else:
                            line_num = block["line"] + 2
                            failed += 1
                            print(f"  FAIL: {md_path}:{line_num}: $ {cmd}")
                            diff = difflib.unified_diff(
                                expected.splitlines(keepends=True),
                                actual.splitlines(keepends=True),
                                fromfile="expected",
                                tofile="actual",
                                lineterm="",
                            )
                            for d in diff:
                                print(f"    {d}")
                            if update:
                                updates.append((block, ci, actual_output))
                    else:
                        # No expected output — just check exit code (already done)
                        passed += 1

    # Apply updates if --update mode
    if update and updates:
        _apply_updates(md_path, raw_lines, blocks, updates)

    return passed, failed


def _apply_updates(md_path, raw_lines, blocks, updates):
    """Rewrite the markdown file with actual output replacing expected output."""
    # Build a map from (block_line, cmd_index) -> actual_output
    update_map = {}
    for block, ci, actual in updates:
        update_map[(block["line"], ci)] = actual

    # Reconstruct the file
    new_lines = list(raw_lines)

    # Process updates in reverse order so line numbers stay valid
    for block in reversed(blocks):
        if block["type"] != "console":
            continue

        # Re-parse commands within this block to find output line ranges
        block_content_start = block["line"] + 1  # line after ```console
        block_content_end = block["end_line"]     # line of closing ```

        # Rebuild the console block content with updated outputs
        rebuilt = []
        ci = 0
        j = block_content_start
        while j < block_content_end:
            line = new_lines[j]
            if line.startswith("$ "):
                rebuilt.append(line)
                j += 1
                # Skip old expected output lines
                while j < block_content_end and not new_lines[j].startswith("$ "):
                    j += 1

                # Insert actual output if we have an update for this command
                if (block["line"], ci) in update_map:
                    actual = update_map[(block["line"], ci)]
                    if actual:
                        for out_line in actual.split("\n"):
                            rebuilt.append(out_line)
                else:
                    # Keep original expected output — re-read from original
                    orig_j = block_content_start
                    orig_ci = 0
                    while orig_j < block_content_end:
                        if raw_lines[orig_j].startswith("$ "):
                            if orig_ci == ci:
                                orig_j += 1
                                while orig_j < block_content_end and not raw_lines[orig_j].startswith("$ "):
                                    rebuilt.append(raw_lines[orig_j])
                                    orig_j += 1
                                break
                            orig_ci += 1
                            orig_j += 1
                            while orig_j < block_content_end and not raw_lines[orig_j].startswith("$ "):
                                orig_j += 1
                        else:
                            orig_j += 1
                ci += 1
            else:
                rebuilt.append(line)
                j += 1

        # Replace block content
        new_lines[block_content_start:block_content_end] = rebuilt

    with open(md_path, "w") as f:
        f.write("\n".join(new_lines) + "\n")

    print(f"  Updated: {md_path}")


def find_md_files(paths):
    """Find all .md files under the given paths."""
    files = []
    for p in paths:
        p = Path(p)
        if p.is_file() and p.suffix == ".md":
            files.append(p)
        elif p.is_dir():
            files.extend(sorted(p.rglob("*.md")))
    return files


def main():
    parser = argparse.ArgumentParser(description="Golden test runner for pq")
    parser.add_argument(
        "paths",
        nargs="*",
        help="Markdown files or directories to test (default: tests/golden/tutorials/ + tests/golden/tests/)",
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="Update expected output in markdown files to match actual output",
    )
    parser.add_argument(
        "--network",
        action="store_true",
        help="Include tests that require network access (<!-- requires: network -->)",
    )
    args = parser.parse_args()

    pq_binary = find_pq_binary()

    if args.paths:
        md_files = find_md_files(args.paths)
    else:
        golden_dir = Path(__file__).resolve().parent
        md_files = find_md_files([
            golden_dir / "tutorials",
            golden_dir / "tests",
        ])

    if not md_files:
        print("No markdown files found.", file=sys.stderr)
        sys.exit(1)

    total_passed = 0
    total_failed = 0
    total_skipped = 0

    for md_file in md_files:
        rel = md_file
        try:
            rel = md_file.relative_to(Path.cwd())
        except ValueError:
            pass

        # Check for <!-- requires: network --> tag
        if not args.network:
            with open(md_file, "r") as f:
                header = f.read(1024)
            if "<!-- requires: network -->" in header:
                print(f"Skipping {rel} (needs --network)")
                total_skipped += 1
                continue

        print(f"Running {rel}...")
        p, f = run_file(str(md_file), pq_binary, update=args.update)
        total_passed += p
        total_failed += f

    print()
    summary = f"{total_passed} passed, {total_failed} failed"
    if total_skipped:
        summary += f", {total_skipped} skipped"
    print(summary)

    if total_failed > 0:
        sys.exit(1)


if __name__ == "__main__":
    main()
