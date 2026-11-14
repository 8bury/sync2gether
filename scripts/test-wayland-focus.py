#!/usr/bin/env python3
"""Regressão do aviso "não está respondendo" com a janela oculta no Wayland.

Abre uma instância sem vídeo pelo GDB numa sessão sway headless (scripts/headless.sh), move
a janela para o scratchpad, onde o compositor deixa de enviar callbacks de
frame, e captura as threads após 12 segundos. Falha se a thread principal
estiver presa em eglSwapBuffers em vez de esperar eventos. Nada aparece na
área de trabalho e a configuração do compositor do usuário não é alterada.
"""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent.parent
BINARY = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug/sync2gether"


def tree_nodes(node):
    yield node
    for child in node.get("nodes", []) + node.get("floating_nodes", []):
        yield from tree_nodes(child)


def find_window(pids):
    tree = json.loads(subprocess.check_output(["swaymsg", "-t", "get_tree"]))
    return next((n for n in tree_nodes(tree) if str(n.get("pid")) in pids), None)


def check():
    os.chdir(ROOT)
    for tool in ("cargo", "gdb", "sway", "swaymsg"):
        if not shutil.which(tool):
            raise RuntimeError(f"Dependência ausente: {tool}")
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
             "--args", str(BINARY)],
            stdout=output, stderr=subprocess.STDOUT,
        )
        try:
            deadline = time.monotonic() + 20
            window = None
            while time.monotonic() < deadline and debugger.poll() is None:
                children = Path(
                    f"/proc/{debugger.pid}/task/{debugger.pid}/children"
                ).read_text().split()
                window = find_window(children)
                if window:
                    break
                time.sleep(0.2)
            if not window:
                raise RuntimeError(f"A janela não abriu; consulte {path}")
            time.sleep(2)
            subprocess.run(
                ["swaymsg", f"[con_id={window['id']}] move scratchpad"],
                check=True, stdout=subprocess.DEVNULL,
            )
            print("Janela de teste oculta por 12 segundos...", flush=True)
            # Uma janela oculta parada não pinta. Redimensionar envia
            # configures, como o Hyprland faz ao ocultar, e força novos frames
            # sem callbacks do compositor.
            for width in (900, 1000, 950, 1050):
                time.sleep(1)
                subprocess.run(
                    ["swaymsg", f"[con_id={window['id']}] resize set {width} 700"],
                    check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                )
            time.sleep(8)
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
    print(f"Janela oculta sem bloqueio em EGL. Backtrace: {path}")


if __name__ == "__main__":
    if os.environ.get("HEADLESS_SESSION") != "1":
        script = ROOT / "scripts/headless.sh"
        os.execvp("bash", ["bash", str(script), sys.executable, *sys.argv])
    check()
