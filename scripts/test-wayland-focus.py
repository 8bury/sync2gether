#!/usr/bin/env python3
"""Regressão EGL na sessão Hyprland real, sem mídia ou alterações de configuração."""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import time


def check():
    root = Path(__file__).resolve().parent.parent
    os.chdir(root)
    for tool in ("cargo", "gdb", "hyprctl"):
        if not shutil.which(tool):
            raise RuntimeError(f"Dependência ausente: {tool}")
    if not os.environ.get("WAYLAND_DISPLAY"):
        raise RuntimeError("Execute na sessão Wayland/Hyprland")
    subprocess.run(["cargo", "build", "--locked"], check=True)
    Path(".cache").mkdir(exist_ok=True)
    path = Path(".cache/wayland-focus-backtrace.log")
    with path.open("w") as output:
        debugger = subprocess.Popen(
            ["gdb", "-q", "-batch",
             "-ex", "set pagination off",
             "-ex", "set debuginfod enabled off",
             "-ex", "set print thread-events off",
             "-ex", "set print frame-arguments none",
             "-ex", "handle SIGPIPE nostop noprint pass",
             "-ex", "run",
             "-ex", "thread apply all bt 25",
             "-ex", "set confirm off",
             "-ex", "quit",
             "--args", "target/debug/sync2gether"],
            stdout=output, stderr=subprocess.STDOUT,
        )
        try:
            deadline = time.monotonic() + 20
            client = None
            while time.monotonic() < deadline and debugger.poll() is None:
                children = Path(
                    f"/proc/{debugger.pid}/task/{debugger.pid}/children"
                ).read_text().split()
                clients = json.loads(subprocess.check_output(["hyprctl", "-j", "clients"]))
                client = next((c for c in clients if str(c["pid"]) in children), None)
                if client:
                    break
                time.sleep(0.2)
            if not client:
                raise RuntimeError(f"A janela não abriu; consulte {path}")
            # Só move a janela criada por este teste. Não modifica regras do compositor.
            time.sleep(2)
            subprocess.run(
                ["hyprctl", "dispatch",
                 'hl.dsp.window.move({workspace="special:sync2gether-check", '
                 f'window={json.dumps("address:" + client["address"])}, follow=false}})'],
                check=True,
            )
            print("Janela de teste oculta por 12 segundos...", flush=True)
            time.sleep(12)
            debugger.send_signal(signal.SIGINT)
            debugger.wait(timeout=15)
        finally:
            if debugger.poll() is None:
                debugger.send_signal(signal.SIGINT)
                try:
                    debugger.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    debugger.kill()
                    debugger.wait()
    text = path.read_text()
    main = text.rsplit("\nThread 1 (", 1)[-1]
    if "\nThread 1 (" not in text or "winit::" not in main:
        raise RuntimeError(f"Backtrace incompleto; consulte {path}")
    if "SwapBuffers" in main:
        raise RuntimeError(f"Thread da janela bloqueada em EGL; consulte {path}")
    if "poll" not in main:
        raise RuntimeError(f"Thread fora da espera normal de eventos; consulte {path}")
    print(f"Wayland/Hyprland: janela oculta sem bloqueio em EGL. Backtrace: {path}")


if __name__ == "__main__":
    check()
