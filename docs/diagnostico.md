# Diagnóstico de travamentos

Captura de checkpoints e pilhas das threads, incluindo o teste de foco em Wayland. Execute os comandos na raiz do projeto.

[Voltar ao README](../README.md)

## Diagnosticar travamentos

Para registrar checkpoints da interface e da renderização, execute:

```sh
cargo run --locked -- --diagnostics
```

A janela se chama `sync2gether [diagnóstico]`. O terminal informa o arquivo
`.cache/sync2gether-diagnostics-PID.log`. Uma thread própria grava uma amostra
por segundo, por até uma hora, mesmo quando a UI para de avançar. O log inclui
a última etapa da UI/OpenGL, o tempo desde o checkpoint, foco, minimização,
ocultação e se há mídia carregada. Não inclui nomes/caminhos de filmes, IPs,
códigos da sala ou argumentos de execução. A UI só atualiza valores atômicos,
sem gravar em disco. Sem `--diagnostics`, essa thread não é iniciada.

Um checkpoint antigo sozinho não prova travamento: uma janela oculta pode
parar de pintar normalmente. Para identificar a função que está bloqueada,
instale GDB e inicie o aplicativo pelo depurador:

```sh
bash scripts/debug-hang.sh
```

Reproduza o aviso, volte ao terminal e pressione Ctrl+C. No prompt do GDB,
digite `capture-hang`. As pilhas de todas as threads ficam em
`.cache/sync2gether-backtrace.log`, sem valores dos argumentos das funções.
Digite `continue` para retomar o aplicativo ou `quit` para sair do depurador.
O arquivo de backtrace é sobrescrito a cada execução do script. O diagnóstico
por PID tem um arquivo próprio para cada processo. Iniciar o processo pelo
GDB evita depender da permissão de anexar o depurador a uma instância já aberta.

Na sessão Wayland com Hyprland 0.55 ou posterior, Python 3 e GDB instalados,
o teste do travamento em EGL pode ser executado com:

```sh
python3 scripts/test-wayland-focus.py
```

Ele compila e abre uma instância sem vídeo pelo GDB, move somente essa janela
para um workspace especial oculto por 12 segundos e captura as threads.
Falha se a thread principal estiver esperando em `SwapBuffers`, em vez de
esperar eventos. O teste encerra sua própria instância. Não modifica regras
do compositor e não faz parte do CI sem sessão gráfica. O backtrace fica em
`.cache/wayland-focus-backtrace.log`.
