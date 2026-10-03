# sync2gether

O sync2gether é um aplicativo de desktop instalado no PC para duas pessoas assistirem a um filme juntas, à distância, usando uma cópia do mesmo arquivo em cada PC.

O objetivo é manter a reprodução sincronizada durante toda a sessão. Quando alguém der play, pausar ou mudar a posição do filme, o outro player deve acompanhar. O app também deve detectar e corrigir diferenças que surgirem durante a reprodução.

## Plataformas iniciais

Os pacotes da versão 0.1.0 são para Arch Linux e Ubuntu 24.04, em x86_64.

## Instalar a release

Baixe o pacote da sua distribuição em
[Releases](https://github.com/8bury/sync2gether/releases).

No Arch Linux atualizado:

```sh
sudo pacman -U ./sync2gether-0.1.0-1-x86_64.pkg.tar.zst
```

No Ubuntu 24.04:

```sh
sudo apt install ./sync2gether_0.1.0_amd64.deb
```

Os gerenciadores instalam libmpv e as outras dependências. Abra **sync2gether**
pelo menu de aplicativos ou execute `sync2gether` no terminal. O seletor de
arquivos usa o portal do desktop; em ambientes mínimos, instale e configure
o backend correspondente, como `xdg-desktop-portal-gtk` ou
`xdg-desktop-portal-kde`.

Confira os arquivos baixados com `sha256sum -c SHA256SUMS` na pasta dos pacotes.
Se baixou apenas um pacote, use `sha256sum --ignore-missing -c SHA256SUMS`.
O `.deb` é compilado em Ubuntu 24.04; Ubuntu 22.04 não é compatível.
O pacote Arch usa as bibliotecas da distribuição rolling release e requer
um sistema atualizado. Os binários não incluem o modo demo.

## Gerar os pacotes

O workflow **Pacotes Linux**, acionado manualmente no GitHub Actions, verifica
o código, executa testes com libmpv e vídeo sintético, compila e instala cada
pacote em sua distribuição. Os pacotes ficam disponíveis como artifacts.

Para repetir localmente com Docker, use checkouts separados para que os
testes não compartilhem fixtures. Todos os builds ficam sob `target/`:

```sh
mkdir -p target/package-work/ubuntu target/package-work/arch
git archive HEAD | tar -x -C target/package-work/ubuntu
git archive HEAD | tar -x -C target/package-work/arch
docker run --rm -v "$PWD/target/package-work/ubuntu:/work" -w /work \
  ubuntu:24.04 bash scripts/release-container.sh deb
docker run --rm -v "$PWD/target/package-work/arch:/work" -w /work \
  archlinux:base-devel bash scripts/release-container.sh arch
```

Os resultados ficam em `target/package-work/{ubuntu,arch}/target/dist/`.
O Docker grava arquivos como root. Para editar ou remover os diretórios depois,
ajuste a propriedade com `sudo chown -R "$(id -u):$(id -g)" target/package-work`.
`packaging/arch/PKGBUILD` empacota o binário produzido no Arch; ele não baixa
nem compila código-fonte sozinho. Ao atualizar a versão no `Cargo.toml`,
atualize também `pkgver` no PKGBUILD e os exemplos de instalação acima.

## Como funciona

Os dois PCs se conectam pela rede local ou por uma VPN que permita comunicação entre eles. Um deles hospeda a sala e coordena a sessão. O app não depende do Tailscale nem de um serviço externo de descoberta.

Cada PC reproduz seu próprio arquivo local. A conexão transporta comandos e informações de sincronização, sem enviar o vídeo entre os computadores. Essa arquitetura dispensa um servidor externo do aplicativo.

O fluxo é:

1. Uma pessoa cria a sala no seu PC.
2. A outra encontra a sala na lista da rede e solicita entrada. O anfitrião aceita ou recusa.
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

## Estado atual

O MVP implementa reprodução local com libmpv, salas de duas pessoas por TCP,
verificação do conteúdo dos arquivos, play/pause/seek coordenados pelo anfitrião,
correção de desvio e reconexão automática do convidado. Volume e seleção de
faixas de áudio e legendas são locais.

A sessão só permite play quando ambos carregaram arquivos com o mesmo hash
BLAKE3 e nenhum player está bloqueado. O hash cobre o arquivo inteiro e é
calculado em segundo plano. Arquivos grandes podem levar algum tempo para
ficarem prontos. Cópias com remux ou metadados diferentes são consideradas
arquivos diferentes, mesmo quando o conteúdo audiovisual parece igual.

A perda de conexão, um erro de reprodução, carregamento interrompido ou fim do
filme pausa a sessão. Após a reconexão, os players recuperam a posição do
anfitrião e aguardam um novo play. Fechar o anfitrião encerra a sala; não há
transferência de coordenação. O convidado continua tentando reconectar até
sair da sala.

A janela usa a API OpenGL do libmpv. O vídeo é desenhado em um framebuffer
com o tamanho da área do player em pixels e permanece na GPU entre frames,
sem converter cada frame para uma imagem RGBA na interface. Novos frames
solicitam o redesenho da janela pelo callback do mpv. Os comandos, a
decodificação e o áudio continuam separados da thread da interface.

A inicialização GL tem um callback próprio, sem desenhar vídeo. Isso permite
carregar o filme na tela de preparação sem depender de um player visível.

O contexto OpenGL usa o controle padrão do libmpv, sem
`MPV_RENDER_PARAM_ADVANCED_CONTROL`. A janela pode ficar oculta ou minimizada
sem atender callbacks de pintura; o controle avançado exigiria processar
atualizações GL mesmo nesse intervalo e poderia bloquear o player.

A janela também desativa a espera de VSync do EGL. Em Wayland/Hyprland, foi
capturado um travamento em `eglSwapBuffers` com a janela sem foco e sem vídeo
carregado. Esperar a apresentação na thread da janela impedia processar os
eventos e responder aos pings do compositor. Essa opção não inicia um loop de pintura contínuo. A apresentação pode ter tearing em ambientes sem
composição que não sincronizem a saída.

A janela configura `hwdec=auto-copy`, que tenta decodificar por hardware e
retorna os frames à memória antes da renderização OpenGL. Esse modo evita
exigir compartilhamento de superfícies do decoder com o contexto gráfico da
janela. Quando não há decoder compatível, libmpv usa software. Não é uma
integração de decodificação sem cópias. Os testes sem janela continuam usando
o renderizador de software em uma thread própria, com imagem de 960 × 540.
Filmes pesados, HDR e desempenho em GPUs físicas precisam de validação
adicional. A integração segue [render.h](https://github.com/mpv-player/mpv/blob/master/include/mpv/render.h)
e [render_gl.h](https://github.com/mpv-player/mpv/blob/master/include/mpv/render_gl.h).

Com a API libmpv anterior a 2.3, o renderizador usa `fbo-format=rgba16`.
No Ubuntu 24.04, libmpv 0.37 com Mesa/llvmpipe exibiu legendas sobre vídeo
preto ao escolher `rgba16f`; RGBA16 inteiro passou pelos testes de vídeo em
movimento e faixas. Essa configuração muda a representação intermediária
de cor nas bibliotecas antigas. HDR e fidelidade de cor nesse caminho ainda
precisam de validação. Bibliotecas novas mantêm a escolha automática do mpv.
Os testes com renderizador por software limitam cada decoder a duas threads;
a janela mantém o paralelismo automático do libmpv.

O modo demo permanece offline, com treze cenários fictícios. Ele não inicia
player, lê filmes ou abre sockets. Os testes de layout não validam áudio,
vídeo, GPU ou sincronização entre PCs.

## Usar o aplicativo

A interface separa início, preparação e reprodução. Na tela inicial, escolha
**Criar sala**, **Entrar em uma sala** ou **Abrir filme sozinho**.

1. Instale as dependências abaixo e conecte os PCs à mesma rede local ou VPN.
   O firewall precisa permitir a porta TCP da sala e UDP 7841 para descoberta.
2. O anfitrião clica em **Criar sala**. O app abre a porta TCP nas interfaces
   IPv4 do PC. Não pede IP nem código. A porta padrão é 7842 e pode ser alterada
   em **Configurações** antes de criar.
3. O convidado escolhe **Entrar em uma sala**. A lista acompanha anúncios UDP
   por multicast `239.255.78.42:7841` e broadcast de cada interface IPv4, inclusive
   interfaces de VPN. O anfitrião anuncia a cada 500 ms; a lista é atualizada
   aproximadamente a cada dois segundos. Portas TCP personalizadas também são
   anunciadas. Cada candidato é confirmado por TCP antes de aparecer. Salas
   ocupadas são removidas e anúncios expiram após cinco segundos. Até 128
   endereços são verificados, com 16 consultas simultâneas e prazo de 800 ms.
4. O convidado clica em **Solicitar entrada**. O anfitrião aceita ou recusa na
   preparação. A solicitação expira após 60 segundos e pode ser cancelada pelo
   convidado. Há uma vaga de convidado, inclusive durante a aprovação.
5. Cada pessoa escolhe sua cópia do filme e aguarda a verificação. O botão
   **Começar a assistir** só fica disponível quando a sessão estiver pronta;
   qualquer participante pode iniciar. O app não dá play automaticamente.

A preparação mostra dois cartões, **Você** e **Outra pessoa**, com os nomes
dos arquivos incluindo a extensão, o estado e o progresso de verificação.
Os dois participantes veem as duas escolhas. **Nome completo** permite consultar
nomes longos e **Copiar nome** confirma a cópia. Apenas o nome é compartilhado
dentro da sala, após a conexão autorizada, sem o caminho das pastas e sem
incluí-lo nos anúncios de descoberta. Caracteres de controle são substituídos
e o nome compartilhado é limitado a 1024 bytes.

Os nomes ajudam a comparar escolhas; a identidade continua sendo o hash BLAKE3
do conteúdo completo e o tamanho. Nomes diferentes podem resultar em arquivos
iguais; nomes iguais não liberam o play quando o conteúdo é diferente.
A verificação mostra porcentagem nos dois PCs. **Cancelar verificação** interrompe
a leitura entre blocos, restaura o arquivo anterior e mantém a sala aberta,
com reprodução pausada. A janela nativa de seleção tem seu próprio cancelamento.

A comparação e o motivo de espera aparecem junto à ação de começar, incluindo
arquivo ausente, conteúdo diferente, player bloqueado e conexão ainda não pronta.
Em janelas altas, a ação fica junto à comparação. Em janelas menores, os cartões
se empilham, o conteúdo pode ser rolado e o botão fica no rodapé, inclusive em
480 × 480. Uma solicitação de entrada recebe destaque antes dos arquivos.
**Encerrar sala** identifica a saída do anfitrião e avisa que desconecta a outra
pessoa. O convidado usa **Sair da sala**. A tela inicial permite continuar um
filme que ficou carregado após sair.
Durante a reprodução, o vídeo ocupa a janela sem rolagem. O botão
**Sala** abre um painel com participante, troca do filme, saída e detalhes da
conexão; em janelas estreitas, abre uma janela sobre o vídeo. Trocar o arquivo
pausa a sessão e retorna à preparação. Sair da sala retorna ao início e deixa
o arquivo carregado e pausado. Estatísticas não ficam permanentemente no player.

**Conectar por endereço** permite informar `IP:porta` quando a sala não aparece.
O campo valida o IP antes de enviar e aceita Enter para solicitar entrada.
Na preparação do anfitrião, **Convidar por endereço** mostra os endereços e confirma a cópia.
IP sem porta usa 7842; IPv6 com porta usa `[IP]:porta`. A conexão manual aceita
IPs numéricos de destino IPv4 e IPv6, sem resolver nomes. Os detalhes da sala
mostram endereços IPv4 locais que podem ser copiados, além da porta.

A descoberta precisa de multicast ou broadcast entre os computadores. Uma VPN
como Radmin pode oferecer essa rede virtual, mas a descoberta depende das suas
configurações e do firewall. VPNs que só roteiam conexões unicast, como o uso
habitual do Tailscale, podem exigir entrada manual. O app não instala nem
configura VPNs. A descoberta automática pela internet fora dessa rede não é
suportada; a conexão manual exige que o endereço e a porta sejam alcançáveis.

O anfitrião identifica a solicitação pelo IP de origem, sem confiar no nome
fornecido pelo cliente. Os anúncios incluem apenas identificação do aplicativo e da sala,
versão, nome do computador, porta e disponibilidade. Não incluem filmes, hashes
nem tokens. A aprovação emite um token aleatório mantido em memória, associado
ao IP do convidado, para reconexão sem nova aprovação. Uma nova aprovação
substitui o token anterior. O token não aparece na interface nem é persistido.
O TCP do aplicativo não tem criptografia própria; ela depende da VPN utilizada.
Falhas de descoberta mantêm a conexão manual disponível. A demo continua offline.

Os controles ficam sobre o vídeo e aparecem ao mover o mouse. Durante a
reprodução, somem após três segundos sem atividade. Permanecem visíveis na
pausa, durante operações pendentes, com um menu aberto, ao arrastar a barra
ou ao manter o mouse sobre os controles. A navegação por Tab também mantém
os controles visíveis. Volume, áudio e legendas afetam somente este PC;
play, pause e mudanças de posição continuam passando pelo coordenador da sala.
O menu de faixas mostra os títulos e idiomas fornecidos pelo arquivo.

O tema padrão usa fundo escuro, detalhes em azul e uma ação principal clara
por etapa. O tema claro permanece disponível em **Configurações**. Os controles
do player usam ícones com dicas e nomes acessíveis para play/pause, saltos de
10 segundos, volume, áudio/legendas e tela cheia. Os menus de volume e faixas
abrem ao clicar nos ícones. O título omite a extensão de vídeo. Avisos sobre
reconexão, preparação e falhas aparecem sobre o vídeo quando necessários;
o estado normal da sessão fica nos detalhes da sala.

Clique na imagem para reproduzir ou pausar. Os atalhos funcionam com o mouse
sobre o vídeo, com o vídeo focado ou em tela cheia, sem interceptar a digitação
nos campos da sala:

| Atalho | Ação |
| --- | --- |
| Espaço | Reproduzir, pausar ou cancelar uma operação pendente |
| ← / → | Voltar ou avançar 10 segundos |
| F | Alternar tela cheia |
| Esc | Sair da tela cheia, depois de fechar menus abertos |
| M | Silenciar ou restaurar o volume local |

A demo usa os mesmos controles visuais, mas as ações de reprodução ficam
desativadas e nenhum player é iniciado. Tela cheia e ocultação dos controles
podem ser exploradas na demo.

Também é possível abrir um arquivo ou conectar por argumentos:

```sh
cargo run --locked -- --file /caminho/filme.mkv
cargo run --locked -- --host --file /caminho/filme.mkv
cargo run --locked -- --host --port 7843
cargo run --locked -- --join 192.168.1.10:7842 --file /caminho/filme.mkv
```

Para testes locais, `--host 127.0.0.1:7842` abre uma sala com aprovação nesse
endereço, sem consultar qualquer VPN. `--join` solicita aprovação, sem código.
Os dois apps precisam usar o protocolo 4.

A biblioteca `libmpv.so.2` é carregada em execução. Se ela faltar, o app exibe
um erro de instalação. Compilar e executar a demo não exige essa biblioteca.

## Arquitetura do core

- `player.rs` encapsula a FFI libmpv e controla reprodução em uma thread própria.
  `player/gl.rs` renderiza no contexto OpenGL do eframe, sem executar comandos
  síncronos de reprodução. `player/tracks.rs` copia a lista de faixas do mpv.
- `player_ui.rs` desenha os controles sobre o vídeo e devolve comandos à aplicação.
- `media.rs` identifica arquivos sem compartilhar caminhos locais.
- `discovery.rs` anuncia e encontra salas por UDP nas interfaces IPv4 e confirma
  os candidatos por TCP, em segundo plano.
- `network/rooms.rs` implementa descoberta, aprovação limitada a uma vaga e
  tokens internos para reconexão. As tarefas de conexão terminam ao fechar a sala.
- `network.rs` implementa TCP com mensagens enquadradas, limite de tamanho,
  autenticação da sala, timeout de heartbeat e reconexão.
- `protocol.rs` define as mensagens Serde e a versão do protocolo.
- `session.rs` ordena comandos no anfitrião e controla prontidão e pausa.
- `sync.rs` estima a diferença entre relógios por ping e calcula correções.
- `runtime.rs` conecta esses módulos por comandos e eventos. A UI só apresenta
  os dados e devolve ações.

Ao fechar a janela, o aplicativo destrói o renderizador no contexto OpenGL
e aguarda o encerramento do runtime, do player e das threads nativas do libmpv.
Essa espera ocorre somente no fechamento. Os testes de integração também
aguardam esse encerramento antes de iniciar o próximo cenário.

A sincronização usa preparação confirmada e início agendado:

1. O anfitrião ordena cada play ou seek e atribui um identificador à operação.
2. Os dois players pausam, ajustam a posição e confirmam a preparação após
   o evento `playback-restart` do libmpv, com posição verificada.
3. Para reproduzir, o anfitrião escolhe um instante futuro comum. A margem é
   calculada a partir do atraso e da incerteza da conexão, entre 350 ms e 5 s.
4. O convidado confirma que recebeu e armou o agendamento. A thread de controle
   de cada player executa play no prazo local equivalente, sem esperar o
   heartbeat da rede ou um frame da interface.

Seek durante reprodução passa pela mesma preparação e retoma no horário
comum; seek em uma sessão pausada permanece pausado. Pause é imediato e
cancela preparações e agendamentos. A interface mostra essas fases e permite
cancelar a operação. Um novo seek substitui a operação anterior; confirmações
antigas não iniciam reprodução. A preparação expira após 10 s. Um agendamento
recebido tarde, não confirmado ou executado com atraso local superior a 80 ms
é cancelado e a sessão volta a ficar pausada.

Os relógios são monotônicos. Ping/pong usa quatro timestamps, medidos na
recepção e no envio da rede, para descontar o processamento remoto. O filtro
usa uma janela de até 16 amostras, priorizando as de menor atraso; três amostras
válidas são necessárias antes de iniciar. Medições de relógio sem atualização
por 1,5 s deixam de permitir play agendado. Assimetria de rede continua sendo
uma fonte de incerteza, não eliminada por esse cálculo.

O player informa posição, instante da medição e velocidade efetiva a cada
50 ms. O controlador extrapola a posição para o instante atual e monitora a
sessão a cada 20 ms; posições com mais de 350 ms não provocam correções.
Desvios filtrados acima de 80 ms iniciam correção proporcional de velocidade,
limitada a ±2%; abaixo de 35 ms ela termina. Desvios acima de 600 ms que
persistem por 500 ms usam seek. Pausado, o limite é 100 ms por 200 ms.
Há intervalo mínimo de dois segundos entre seeks periódicos e um período
para o player estabilizar após operações. Play e pause não provocam seek
apenas por mudarem a revisão do estado.

Heartbeats e posições dos participantes são enviados a cada 200 ms; mudanças
de operação são enviadas imediatamente. A interface distingue o desvio contra
a linha do tempo da sala da diferença estimada entre os players, quando
existem medições recentes. Esses valores não medem o instante físico em que
cada monitor ou dispositivo de áudio apresentou o conteúdo.

O protocolo agora é versão 4, incluindo nomes e progresso de verificação dentro da sala. Os dois aplicativos precisam usar a mesma versão;
clientes da versão anterior são recusados no handshake.

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

## Validação do novo fluxo

Os testes de TCP em localhost cobrem descoberta sem entrada na sala, vaga
reservada durante a aprovação, recusa, cancelamento, encerramento com solicitação
pendente, rejeição de tokens inválidos e reconexão de um convidado aprovado.
O teste de layout cobre as etapas e o painel nos temas claro e escuro, sem I/O.
Os testes UDP usam multicast real em loopback, incluindo porta personalizada,
sala ocupada, anúncios inválidos, expiração e deduplicação entre interfaces. Não comprovam descoberta entre
dois PCs nem transporte de multicast ou broadcast por uma VPN.

A validação local do fluxo ocorreu em CachyOS, da família Arch: testes de
layout, TCP e três cenários com dois players reais, incluindo atraso e jitter.
Um quarto cenário verifica criação automática, busca contínua, aprovação e
remoção de uma sala encerrada entre duas instâncias, sem VPN instalada.
As capturas da demo foram revisadas em Wayland/Hyprland e salvas em `.cache/`.
O exemplo OpenGL também completou vídeo, faixas, controles, tela cheia,
redimensionamento, comandos sem pintura e encerramento em Wayland/Hyprland
0.56.2. Para essa execução, a janela criada pelo teste foi tornada flutuante e
redimensionada pelo compositor após sair da tela cheia; o compositor ignorou
o pedido de tamanho do aplicativo. Não foram alteradas regras permanentes nem
outras janelas.
Não foi validada execução em Ubuntu, Windows ou descoberta entre dois PCs por
Radmin, Tailscale ou outras VPNs.

## Validação do polimento visual

O polimento foi validado localmente em CachyOS, com X11 via Xvfb e Mesa por
software. Foram geradas 32 capturas de demo: os oito cenários em 960 × 640,
nos dois temas, e início, preparação pronta, reprodução e reconexão também
em 720 × 540 e 480 × 480. Todas usam dados fictícios e ficam em `.cache/`.

Os testes de layout cobrem os dois temas e os três tamanhos, incluindo a
visibilidade das ações na janela mínima, o botão fixo de começar, contraste
do título sobre o vídeo, avisos de reconexão e comandos locais dos menus de
áudio, legendas e volume. `scripts/check.sh` passou. O teste OpenGL com vídeo
sintético também passou, cobrindo faixas, ocultação dos controles, tela cheia,
redimensionamento e comandos sem pintura. Esta validação visual não incluiu
execução em Ubuntu nem testes entre dois PCs.

## Escolha compartilhada dos arquivos

A revisão da preparação foi validada em CachyOS, na sessão Wayland/Hyprland.
`bash scripts/check.sh` passou, com testes de nomes nos dois papéis, comparação
por conteúdo, progresso, cancelamento, descarte de eventos antigos, aprovação
visível em 480 × 480 e layout com nomes longos nos dois temas. O transporte dos
metadados também foi testado por TCP real após aprovação da entrada.

`bash scripts/test-mvp.sh` passou nos quatro cenários com vídeo sintético e dois
players libmpv por software. O teste confirma que nomes diferentes com conteúdo
igual permitem assistir e que os nomes reaparecem corretamente após reconectar.
Os cenários de incompatibilidade, comandos coordenados, descoberta e rede com
atraso e jitter continuam passando. Esta revisão não validou execução em Ubuntu
nem comunicação entre dois PCs físicos.

As capturas de revisão ficam em `.cache/ui-redesign/`. Incluem preparação pronta,
escolha inicial, verificação, arquivos diferentes, aprovação, falha e reconexão,
com revisão em 960 × 640, 480 × 480 e uma janela alta de 1000 × 1000.
O modo demo aguarda 900 ms antes de solicitar a captura para permitir que o
compositor conclua o posicionamento da janela.

As referências consultadas para a organização dos controles e o fluxo de convite
foram [Netflix](https://help.netflix.com/en/node/372),
[Disney+](https://help.disneyplus.com/article/disneyplus-player-controls) e
[Rave](https://rave.io/faq). A preparação com duas cópias locais e comparação de
conteúdo é específica deste aplicativo.
