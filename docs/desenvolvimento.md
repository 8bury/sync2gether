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

No Windows (x86_64 com MSVC):

- Instale as **Ferramentas de Build do Visual Studio** (C++ com MSVC) e o `rustup`.
- Baixe `libmpv-2.dll` (build `mpv-dev-x86_64` de `shinchiro/mpv-winbuild-cmake`)
  e coloque-a na mesma pasta do executável (por exemplo `target/debug/` ou
  `target/release/`), ou aponte o caminho em `SYNC2GETHER_LIBMPV`.
- Opcionalmente, instale o **NSIS** (`makensis`) para gerar o instalador local
  com `pwsh -File scripts/package-windows.ps1 -LibMpvPath caminho\para\libmpv-2.dll`.

No macOS (Apple Silicon ou Intel):

```sh
brew install mpv
```

O app procura `libmpv.2.dylib` dentro do `.app`, na pasta do executável e nos
diretórios padrão do Homebrew (`/opt/homebrew/lib` e `/usr/local/lib`), ou no
caminho definido em `SYNC2GETHER_LIBMPV`. Para gerar o `.app` e o `.dmg`
localmente após `cargo build --release --locked`:

```sh
bash scripts/package-macos.sh arm64
```

(Troque `arm64` por `x86_64` em Macs Intel.)

Os pacotes mpv/libmpv fornecem libmpv para reprodução. A interface demo
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

Os valores de `--demo-state` são `idle`, `browsing`, `connecting`,
`pending-approval`, `waiting`, `approval`,
`choosing`, `verifying`, `mismatch`, `stabilizing`, `player-error`, `ready`,
`synchronized`, `paused`, `reconnecting`, `peer-left` e `room-closed`. Eles usam as mesmas telas
do app, com dados fictícios e sem enviar comandos ao runtime. Use `--demo-light` para o tema claro
e `--lang en` ou `--lang pt-BR` para escolher o idioma; sem `--lang`, a demo segue o idioma do sistema.

Para revisar o fluxo inicial, use `idle` para a tela de boas-vindas,
`browsing` para a lista e conexão manual, `connecting` para a tentativa de
conexão e `pending-approval` para a espera do convidado. `approval` continua
mostrando a solicitação na tela do anfitrião. `join-code` mostra a entrada com
código. As fontes Geist (regular e medium) e Geist Mono (regular) estão
incorporadas em `assets/fonts/`, com a licença SIL OFL 1.1; a interface não lê
fontes do disco durante o desenho.

Os cenários `choosing`, `verifying`, `ready`, `mismatch` e `player-error`
permitem revisar a lista de arquivos, progresso, liberação do play e falhas.
Use os menus **⋯** para consultar e copiar os nomes fictícios. `approval` e
`waiting` exercitam a solicitação e o convite na preparação do anfitrião.
`peer-left` mostra o anfitrião depois que o convidado saiu da sala, e
`room-closed` mostra o convidado de volta ao início depois que o anfitrião
encerrou a sala.

Para salvar uma captura e encerrar:

```sh
mkdir -p .cache
cargo run --locked --features demo -- --demo --demo-state reconnecting --demo-shot .cache/reconnecting.png
```

Esse comando abre a janela na área de trabalho. Para capturar sem exibir
nada nem roubar o foco, instale `sway` e use a sessão headless:

```sh
cargo build --locked --features demo
mkdir -p .cache
timeout 60s bash scripts/headless.sh target/debug/sync2gether --demo --lang pt-BR --demo-state reconnecting --demo-shot .cache/reconnecting.png
```

Em um ambiente sem sessão gráfica, também é possível usar Xvfb. No Ubuntu, instale `xvfb` e `xauth`; no Arch,
`xorg-server-xvfb` e `xorg-xauth`.

```sh
cargo build --locked --features demo
mkdir -p .cache
timeout 30s xvfb-run -a env LIBGL_ALWAYS_SOFTWARE=1 target/debug/sync2gether --demo --demo-shot .cache/demo.png
```

Capturas de revisão e arquivos temporários ficam em `.cache/`, ignorada pelo Git.
Para capturar telas da página do navegador com estados fictícios (somente em
desenvolvimento, com `bash scripts/build-web.sh --preview`), veja
[Convidado no navegador](navegador.md#desenvolvimento).

O fluxo das telas foi decidido num protótipo React em `prototype/`, com o kit
Stageshow. Ele roda com `npm install` e `npm run dev` dentro da pasta e serve
de referência visual; veja `prototype/README.md`.
As quatro imagens selecionadas para o README fazem parte da documentação em
`docs/images/`. Foram geradas em CachyOS pela sessão sway headless de
`scripts/headless.sh`, em 960 × 640, usando os cenários `idle`, `ready` e
`synchronized`, além de `ready` com `--demo-light`, todos com `--lang pt-BR`. Todas usam dados fictícios, sem mídia ou conexão real.

## Idiomas

Todo texto visível fica em `src/i18n/`: `strings.rs` declara os campos e
`pt_br.rs` e `en.rs` preenchem um por idioma. Textos com valores usam funções,
como `room_of: |name| format!("{name}'s room")`. Para acrescentar um idioma,
crie outro arquivo com todos os campos, inclua a variante em `Language` e em
`Language::ALL` e mapeie o código de locale em `Language::from_locale`; o
compilador aponta campos faltando. O teste de layout percorre todos os idiomas
de `Language::ALL` nos tamanhos de janela suportados, e
`english_interface_has_no_untranslated_portuguese_in_any_demo_scenario` procura
texto em português que tenha escapado da tradução. Os testes que verificam textos
em português chamam `ui::configure_language(&ctx, Language::PtBr)`.

## Verificações e trabalho com agentes

As instruções do projeto estão em [AGENTS.md](../AGENTS.md). A interface retorna
ações que a aplicação processa após desenhar a tela. Rede e player ficam em
módulos próprios, sem bloquear a interface.

Execute as mesmas verificações usadas no CI:

```sh
bash scripts/check.sh
```

O script verifica formatação, comentários, limite de tamanho dos arquivos de
código (`scripts/check-file-size.py`), Clippy, testes e documentação, incluindo a
feature `demo`. O teste de layout exercita a tela inicial e todos os cenários em três dimensões
de janela e nos temas claro e escuro, com e sem a feature `demo`. O CI está configurado para Ubuntu 24.04,
Arch Linux (container), Windows (`windows-latest`) e macOS (`macos-15`). No Ubuntu, também captura a janela demo usando Xvfb.
Executar o CI depende de hospedar o projeto no GitHub.

O guia para agentes, os cenários demo e o fluxo de verificações foram
inspirados no [zapfast](https://github.com/crmne/zapfast), adaptados para este
projeto. A lógica de sessão e relógio tem testes sem player. Os testes TCP usam
localhost e verificam autenticação, ordenação, timeout e reconexão.

O repositório é um workspace Cargo com o app e o crate `relay/`; o
`check.sh` verifica os dois. `tests/relay.rs` sobe o relay real em localhost,
num runtime próprio, e verifica código certo e errado, fechamento após três
erros, sala inexistente, versão incompatível, segredo do anfitrião, queda do
enlace do convidado e reinício do relay com retomada do mesmo número. Os testes
do canal cifrado também rejeitam quadros alterados e repetidos.

Para testar contra um relay publicado, sem players:

```sh
SYNC2GETHER_RELAY_E2E=relay.pedrosales.dev:7843 cargo test --locked --test relay -- --ignored
```

A imagem do relay é construída com `docker build -f relay/Dockerfile .`;
veja [Salas pela internet e relay](relay.md#publicar-com-docker-e-coolify).

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
Um terceiro cenário simula uma rota intercontinental, com 90 a 150 ms por
direção: retém todo o tráfego por 2 s durante a reprodução, que deve continuar
sem pausa, e depois além do limite de silêncio, quando os dois lados pausam,
reconectam e voltam a iniciar juntos.
Um quarto cenário cria a sala pela internet com o relay em localhost e coloca
o convidado atrás de um proxy de bytes com 110 a 140 ms por sentido: entrada
pelo código, início agendado, desvio entre players e pausa passam pelo canal
cifrado.
Para executar um cenário, passe parte do nome dele:
`bash scripts/test-mvp.sh intercontinental`.
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
validar saída de áudio em dois dispositivos, filmes
longos, HDR, desempenho com hardware diferente ou execução real em Ubuntu.

A correção do bloqueio em EGL também foi validada em Wayland/Hyprland 0.56.2
no CachyOS, com a janela sem mídia movida para um workspace oculto por 12 s
e captura da thread principal pelo GDB. O teste OpenGL com vídeo sintético
continuou passando em X11/Xvfb após desativar a espera de VSync.
