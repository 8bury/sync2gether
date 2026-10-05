# Desenvolvimento e testes

Dependências, modo demo e verificações. Execute os comandos na raiz do projeto.

[Voltar ao README](../README.md)

## Ambiente de desenvolvimento

Instale o [rustup](https://rustup.rs/). O arquivo `rust-toolchain.toml` fixa
Rust 1.98.0, rustfmt e Clippy. Os comandos Cargo usam essa versão ao serem
executados na pasta do projeto. Mantenha `Cargo.lock` no controle de versão.

No Arch Linux:

```sh
sudo pacman -S --needed base-devel rustup pkgconf wayland libxkbcommon libxkbcommon-x11 mesa mpv
```

No Ubuntu:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libwayland-dev libxkbcommon-dev libxkbcommon-x11-0 libgl1-mesa-dev libmpv-dev
```

Os pacotes mpv/libmpv-dev fornecem libmpv para reprodução. A interface demo
continua funcionando sem carregar essa biblioteca.

Para abrir o aplicativo:

```sh
cargo run --locked
```

Para explorar a demonstração, use o menu de cenários ao lado de **DEMO OFFLINE** ou escolha
um cenário no comando:

```sh
cargo run --locked --features demo -- --demo --demo-state synchronized
```

Os valores de `--demo-state` são `idle`, `browsing`, `waiting`, `approval`,
`choosing`, `verifying`, `mismatch`, `stabilizing`, `player-error`, `ready`,
`synchronized`, `paused` e `reconnecting`. Eles usam as mesmas telas
do app, com dados fictícios e sem enviar comandos ao runtime. Use `--demo-light` para o tema claro.

Para salvar uma captura e encerrar:

```sh
mkdir -p .cache
cargo run --locked --features demo -- --demo --demo-state reconnecting --demo-shot .cache/reconnecting.png
```

A captura precisa de um display Wayland ou X11 e de OpenGL. Em um ambiente
sem sessão gráfica, use Xvfb. No Ubuntu, instale `xvfb` e `xauth`; no Arch,
`xorg-server-xvfb` e `xorg-xauth`.

```sh
cargo build --locked --features demo
mkdir -p .cache
timeout 30s xvfb-run -a env LIBGL_ALWAYS_SOFTWARE=1 target/debug/sync2gether --demo --demo-shot .cache/demo.png
```

Capturas de revisão e arquivos temporários ficam em `.cache/`, ignorada pelo Git.
As quatro imagens selecionadas para o README fazem parte da documentação em
`docs/images/`. Foram geradas em CachyOS com Wayland/Hyprland, em 960 × 640,
usando os cenários `idle`, `ready` e `synchronized`, além de `ready` com
`--demo-light`. Todas usam dados fictícios, sem mídia ou conexão real.

## Verificações e trabalho com agentes

As instruções do projeto estão em [AGENTS.md](../AGENTS.md). A interface retorna
ações que a aplicação processa após desenhar a tela. Rede e player ficam em
módulos próprios, sem bloquear a interface.

Execute as mesmas verificações usadas no CI:

```sh
bash scripts/check.sh
```

O script verifica formatação, Clippy, testes e documentação, incluindo a
feature `demo`. O teste de layout exercita a tela inicial e todos os cenários em três dimensões
de janela e nos temas claro e escuro, com e sem a feature `demo`. O CI está configurado para Ubuntu 24.04
e um container Arch Linux. No Ubuntu, também captura a janela demo usando Xvfb.
Executar o CI depende de hospedar o projeto no GitHub.

O guia para agentes, os cenários demo e o fluxo de verificações foram
inspirados no [zapfast](https://github.com/crmne/zapfast), adaptados para este
projeto. A lógica de sessão e relógio tem testes sem player. Os testes TCP usam
localhost e verificam autenticação, ordenação, timeout e reconexão.

## Teste de integração do MVP

Com libmpv, FFmpeg e `pactl` instalados, execute:

```sh
bash scripts/test-mvp.sh
```

O script gera um vídeo sintético com áudio em `.cache/` e executa dois players
reais conectados por localhost. O áudio usa um sink virtual temporário, sem
enviar a fixture aos alto-falantes. Em um desktop, ele usa o servidor PulseAudio
ou PipeWire-Pulse existente e remove somente seu sink ao terminar. Sem servidor,
inicia uma instância isolada de PulseAudio e a encerra depois do teste.
Instale `libpulse` no Arch ou `pulseaudio-utils` no Ubuntu para obter `pactl`.
Em containers sem desktop, instale também `pulseaudio`. Esse teste verifica
o processamento e o relógio do áudio, sem comprovar saída física de som.
O teste usa o perfil `release`, com as mesmas otimizações dos pacotes.
O proxy retém uma confirmação de preparação para testar a pausa antes de
liberá-la; a confirmação atrasada não pode reiniciar o vídeo.
Verifica play solicitado pelo convidado,
seek, pause, desconexão, retorno à sala, recusa de arquivos diferentes e
retomada após fim do vídeo. Também verifica entrada com aprovação, preparação
sem play automático e retorno à preparação quando alguém troca o arquivo. Um segundo cenário usa um proxy TCP com atraso
variável de 40 a 65 ms por mensagem em cada direção, verificando início
agendado, seek durante reprodução e cancelamento com mensagens em trânsito.
Os testes verificam a execução dos comandos de play com diferença estimada
abaixo de 100 ms nesse ambiente; não garantem essa precisão em qualquer rede
ou na apresentação física do áudio/vídeo. Não usam arquivos pessoais nem
precisam de display.
O teste fica ignorado no comando Cargo padrão porque depende de libmpv e FFmpeg;
o CI o executa explicitamente em Arch e Ubuntu.

Para validar o renderizador com uma janela real, libmpv, FFmpeg e o mesmo
ambiente de áudio virtual:

```sh
bash scripts/test-player-gl.sh
```

Em um ambiente sem sessão gráfica:

```sh
env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET xvfb-run -a -s '-screen 0 1280x720x24' env LIBGL_ALWAYS_SOFTWARE=1 bash scripts/test-player-gl.sh
```

Esse script gera outra fixture sintética a 60 fps, com dois áudios e legenda,
e executa `examples/check-player-gl.rs`. Verifica vídeo em movimento por
capturas da área do vídeo, troca de faixas, desativação de legendas, ocultação
e retorno dos controles, tela cheia, redimensionamento e encerramento com um
seek em andamento. Também suspende os callbacks de pintura por pelo menos
quatro segundos e verifica carregamento e seek pausado nesse intervalo, depois
confirma o retorno da imagem. Isso exercita o player sem pintura, mas não
automatiza a perda de foco ou a minimização pelo compositor.
As capturas ficam em `.cache/player-gl-stage-*.png`.
Esse teste usa a thread principal do processo e não faz parte dos testes
Cargo sem janela. Não mede fluidez percebida nem prova aceleração por hardware.

A validação local desta implementação ocorreu em CachyOS, da família Arch,
com libmpv 0.41, dois players por software e janela OpenGL em X11 via Xvfb,
usando Mesa por software. O exemplo `check-player-gl` verificou vídeo em
movimento, troca de faixas, controles, tela cheia, redimensionamento e encerramento.
A demo offline e a mensagem de biblioteca ausente
também foram exercitadas. Não equivale a
validar dois PCs pela rede local ou VPN, saída de áudio em dois dispositivos, filmes
longos, HDR, desempenho com hardware diferente ou execução real em Ubuntu.

A correção do bloqueio em EGL também foi validada em Wayland/Hyprland 0.56.2
no CachyOS, com a janela sem mídia movida para um workspace oculto por 12 s
e captura da thread principal pelo GDB. O teste OpenGL com vídeo sintético
continuou passando em X11/Xvfb após desativar a espera de VSync.
