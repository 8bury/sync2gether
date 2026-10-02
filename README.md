# sync2gether

O sync2gether é um aplicativo de desktop instalado no PC para duas pessoas assistirem a um filme juntas, à distância, usando uma cópia do mesmo arquivo em cada PC.

O objetivo é manter a reprodução sincronizada durante toda a sessão. Quando alguém der play, pausar ou mudar a posição do filme, o outro player deve acompanhar. O app também deve detectar e corrigir diferenças que surgirem durante a reprodução.

## Plataformas iniciais

O aplicativo terá suporte inicial a Arch Linux e Ubuntu.

## Como funciona

Os dois PCs se conectam pelo Tailscale. Um deles hospeda a sala e coordena a sessão, enquanto o outro se conecta a essa sala pelo endereço do Tailscale.

Cada PC reproduz seu próprio arquivo local. A conexão transporta comandos e informações de sincronização, sem enviar o vídeo entre os computadores. Essa arquitetura dispensa um servidor externo do aplicativo.

O fluxo previsto é:

1. Uma pessoa cria a sala no seu PC.
2. A outra entra na sala pelo Tailscale.
3. Cada pessoa seleciona sua cópia do filme.
4. O app verifica se os arquivos correspondem e aguarda as duas pessoas ficarem prontas.
5. As duas assistem com os controles e a posição de reprodução sincronizados.

## Escopo inicial

- Sessões com duas pessoas em PCs.
- Criar uma sala e entrar em uma sala existente.
- Selecionar e reproduzir arquivos locais.
- Sincronizar play, pause e mudanças de posição feitas por qualquer participante.
- Compensar o atraso da conexão e corrigir diferenças entre os players.
- Pausar os dois players quando um deles não conseguir continuar a reprodução.
- Exibir o estado da sessão, como aguardando o outro participante, sincronizado ou reconectando.

A primeira versão se concentra na reprodução sincronizada. Chamadas de voz podem acontecer em um aplicativo separado.

## Tecnologias

- Rust para a lógica do aplicativo.
- egui e eframe, com OpenGL, para a interface de desktop em Wayland e X11.
- Tokio para a comunicação entre os PCs.
- Serde para as mensagens de sincronização.
- libmpv para reprodução de vídeo, áudio e legendas.

Tokio, Serde e libmpv serão integrados nas etapas de rede e reprodução. A base
atual contém a interface inicial e os recursos de desenvolvimento.

## Estado atual

O projeto tem uma janela inicial e um modo demo offline com cinco cenários:
sem sessão, aguardando parceiro, sincronizados, pausado e reconectando. Esses
estados são fictícios. Criar salas, abrir filmes e sincronizar PCs ainda não
estão implementados.

A interface usa uma base escura com detalhes em verde, cartões para criar ou
entrar em uma sala e uma área de vídeo ilustrativa nos cenários de sessão.
Em janelas estreitas, os cartões ficam um abaixo do outro e o conteúdo pode
ser rolado. As ações de sala e reprodução ficam desabilitadas até a
implementação desses recursos.

O modo demo permite revisar a interface sem Tailscale ou arquivos de vídeo.
Inclui captura PNG da janela e um teste de layout sem display. O teste de
layout não valida reprodução, aceleração por hardware ou sincronização real.

## Ambiente de desenvolvimento

Instale o [rustup](https://rustup.rs/). O arquivo `rust-toolchain.toml` fixa
Rust 1.98.0, rustfmt e Clippy. Os comandos Cargo usam essa versão ao serem
executados na pasta do projeto. Mantenha `Cargo.lock` no controle de versão.

No Arch Linux:

```sh
sudo pacman -S --needed base-devel rustup pkgconf wayland libxkbcommon mesa mpv
```

No Ubuntu:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libwayland-dev libxkbcommon-dev libgl1-mesa-dev libmpv-dev
```

Os pacotes mpv/libmpv-dev preparam o ambiente para a etapa de reprodução, mas
a interface demo atual não precisa de libmpv para compilar.

Para abrir o aplicativo:

```sh
cargo run --locked
```

Para explorar a demonstração, use os botões de cenário na janela ou escolha
um cenário no comando:

```sh
cargo run --locked --features demo -- --demo --demo-state synchronized
```

Os valores de `--demo-state` são `idle`, `waiting`, `synchronized`, `paused` e
`reconnecting`. Use `--demo-light` para o tema claro.

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

Capturas e arquivos temporários ficam em `.cache/`, ignorada pelo Git.

## Verificações e trabalho com agentes

As instruções do projeto estão em [AGENTS.md](AGENTS.md). A interface retorna
ações que a aplicação processa após desenhar a tela. Rede e player terão
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
projeto. A simulação de dois participantes com atraso e falhas será adicionada
quando a lógica de sincronização existir.
