#!/usr/bin/env python3
"""Abre o pacote instalado com uma fixture e fecha pela mensagem nativa do X11."""
import subprocess
import time
from pathlib import Path

from Xlib import X, display, protocol


fixture = Path(".cache/player-gl-fixture.mkv").resolve()
assert fixture.is_file(), "Execute scripts/test-player-gl.sh antes deste teste"
process = subprocess.Popen(["/usr/bin/sync2gether", "--file", str(fixture)])
connection = display.Display()
try:
    window = None
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        assert process.poll() is None, "O pacote instalado encerrou antes de abrir a janela"
        for candidate in connection.screen().root.query_tree().children:
            if candidate.get_wm_name() == "sync2gether":
                window = candidate
                break
        if window is not None:
            break
        time.sleep(0.1)
    assert window is not None, "O pacote instalado não abriu a janela"
    time.sleep(2)
    window.send_event(
        protocol.event.ClientMessage(
            window=window,
            client_type=connection.intern_atom("WM_PROTOCOLS"),
            data=(32, [connection.intern_atom("WM_DELETE_WINDOW"), X.CurrentTime, 0, 0, 0]),
        ),
        event_mask=0,
    )
    connection.flush()
    assert process.wait(timeout=10) == 0, "Erro ao fechar o pacote instalado"
    print("Pacote instalado: janela com fixture e encerramento normal verificados.")
finally:
    if process.poll() is None:
        process.kill()
        process.wait()
    connection.close()
