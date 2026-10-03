#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
command -v gdb >/dev/null || { echo 'Instale gdb para capturar as threads.' >&2; exit 1; }
cargo build --locked
mkdir -p .cache
cat > .cache/debug-hang.gdb <<'GDB'
set pagination off
set debuginfod enabled off
set print thread-events off
set print frame-arguments none
handle SIGPIPE nostop noprint pass
set logging file .cache/sync2gether-backtrace.log
set logging overwrite on
set logging enabled on
define capture-hang
  set logging enabled on
  thread apply all bt 25
  set logging enabled off
end
echo Reproduza o aviso. Volte a este terminal e pressione Ctrl+C.\n
echo Depois digite capture-hang para salvar as pilhas, e continue para retomar.\n
run
GDB
# O GDB é o pai do processo: não precisa liberar ptrace de processos existentes.
# Sem --file/--join: a execução começa sem caminhos ou códigos nos argumentos.
exec gdb -q -x .cache/debug-hang.gdb --args target/debug/sync2gether --diagnostics
