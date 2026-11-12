#!/usr/bin/env python3
"""
PL: Jeden start serwera: czas od `horizon-server start` do otwarcia portu, po 10 s RSS silnika; potem `stop`.
    Zimny start: najpierw usuń <katalog>/.horizon-cache/mod-content.
EN: One server start: time from `horizon-server start` to the open port, engine RSS after 10 s; then `stop`.
    Cold start: remove <dir>/.horizon-cache/mod-content first.

Użycie / usage: srvbench.py <katalog serwera> <port> <etykieta>
"""

import json
import os
import socket
import subprocess
import sys
import time

SB = os.path.expanduser(
    "~/Library/Application Support/horizon-launcher/versions/nightly-2026-11-05-66f3981/horizon-server"
)


def main() -> None:
    d, port, label = os.path.abspath(sys.argv[1]), int(sys.argv[2]), sys.argv[3]
    t0 = time.monotonic()
    start = subprocess.Popen([SB, "--dir", d, "start"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    while True:
        try:
            socket.create_connection(("127.0.0.1", port), timeout=0.05).close()
            break
        except OSError:
            if time.monotonic() - t0 > 180:
                sys.exit(f"{label}: port {port} not open after 180 s, see {d}/logs/supervisor.log")
            time.sleep(0.02)
    port_open = time.monotonic() - t0
    start.wait()
    time.sleep(10)
    pids = subprocess.run(
        ["pgrep", "-f", f"control-socket {d}/.horizon/engine.json"], capture_output=True, text=True
    ).stdout.split()
    rss = int(subprocess.run(["ps", "-o", "rss=", "-p", pids[0]], capture_output=True, text=True).stdout) // 1024
    print(json.dumps({"label": label, "port_open_s": round(port_open, 2), "rss_mb": rss}))
    subprocess.run([SB, "--dir", d, "stop"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


if __name__ == "__main__":
    main()
