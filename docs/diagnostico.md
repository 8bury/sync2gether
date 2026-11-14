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

Com Python 3, GDB e sway instalados, o teste do travamento em EGL pode ser
executado com:

```sh
python3 scripts/test-wayland-focus.py
```

Ele compila e abre uma instância sem vídeo pelo GDB numa sessão sway
headless (`scripts/headless.sh`), sem mostrar nada na área de trabalho. Move a
janela para o scratchpad, onde o compositor deixa de enviar callbacks de frame,
redimensiona a janela oculta para forçar novos frames e captura as threads após
12 segundos. Falha se a thread principal estiver esperando em `SwapBuffers`, em
vez de esperar eventos. Com a espera de VSync ativada, o teste reproduz o
travamento. Não altera a configuração do compositor da sessão e não faz parte
do CI. O backtrace fica em `.cache/wayland-focus-backtrace.log`.
