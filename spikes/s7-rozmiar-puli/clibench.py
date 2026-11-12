#!/usr/bin/env python3
"""
PL: Start klienta: od pojawienia się procesu do ekranu `playing` z chunkami; po 15 s RSS
    i ostrzeżenia z logu instancji. Uruchom w tle, potem MCP `play --server ...`.
EN: Client start: from the process appearing to the `playing` screen with chunks; after 15 s RSS
    and the instance log's warnings. Run in the background, then MCP `play --server ...`.

Użycie / usage: H=<katalog> clibench.py <etykieta>
"""

import json
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
INST = os.environ.get(
    "INST", os.path.expanduser("~/Library/Application Support/horizon/instances/hot-spike")
)


def client_pid() -> str | None:
    out = subprocess.run(["pgrep", "-fl", "horizon-client"], capture_output=True, text=True).stdout
    # PL: silniki serwera to ten sam program, ale z `engine.json` w linii poleceń.
    # EN: server engines run the same program, with `engine.json` on the command line.
    return next((line.split()[0] for line in out.splitlines() if "engine.json" not in line), None)


def main() -> None:
    label = sys.argv[1]
    started = time.monotonic()
    while not (pid := client_pid()):
        if time.monotonic() - started > 300:
            sys.exit(f"{label}: no client process after 300 s")
        time.sleep(0.02)
    t0 = time.monotonic()
    while True:
        out = subprocess.run([f"{HERE}/ctl.sh", "status"], capture_output=True, text=True).stdout
        try:
            status = json.loads(out)
        except json.JSONDecodeError:
            status = {}
        if status.get("screen") == "playing" and status.get("chunks", 0) > 0:
            break
        if time.monotonic() - t0 > 300:
            sys.exit(f"{label}: not playing after 300 s, last status: {status}")
        time.sleep(0.1)
    playing = time.monotonic() - t0
    time.sleep(15)
    rss = int(subprocess.run(["ps", "-o", "rss=", "-p", pid], capture_output=True, text=True).stdout) // 1024
    with open(f"{INST}/logs/latest.log", errors="replace") as log:
        warns = [line for line in log if "WARN" in line or "ERROR" in line]
    print(json.dumps({
        "label": label,
        "playing_s": round(playing, 2),
        "rss_mb": rss,
        "warn_lines": len(warns),
        "missing_model_lines": sum("missing block model" in w for w in warns),
    }))


if __name__ == "__main__":
    main()
