# Arquitetura e sincronização

A implementação do MVP, a integração com libmpv e os limites da sincronização.

[Voltar ao README](../README.md)

## Estado atual

O MVP implementa reprodução local com libmpv, salas de duas pessoas pela
internet (via relay, com código e criptografia de ponta a ponta) ou por TCP direto na rede local,
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

O modo demo permanece offline, com dezesseis cenários fictícios. Ele não inicia
player, lê filmes ou abre sockets. Os testes de layout não validam áudio,
vídeo, GPU ou sincronização entre PCs.

## Arquitetura do core

- `player.rs` encapsula a FFI libmpv e controla reprodução em uma thread própria.
  `player/gl.rs` renderiza no contexto OpenGL do eframe, sem executar comandos
  síncronos de reprodução. `player/tracks.rs` copia a lista de faixas do mpv.
- `player_ui.rs` desenha os controles sobre o vídeo e devolve comandos à aplicação.
- `ui.rs` coordena o estado das telas (`Screen`, `Inputs`) e o fluxo principal
  com os dados do runtime; a aprovação leva o convidado à preparação dos arquivos.
  `ui/theme.rs` reúne paleta, tema e componentes compartilhados; `ui/settings.rs`
  desenha o cabeçalho, o progresso de etapas e o menu de configurações;
  `ui/room.rs` desenha o painel da sala, confirmação de saída e detalhes da
  conexão; `ui/demo.rs` monta a barra e os estados fictícios do modo demo.
  `ui/entry.rs` desenha início, lista de salas e espera de conexão ou aprovação,
  sem I/O.
- `ui/preparation.rs` reúne os arquivos locais e remotos numa lista, com menus
  de nome completo e cópia. Seleção, cancelamento da verificação, aprovação e
  início continuam retornando comandos ao runtime; a UI não lê mídia.
- `media.rs` identifica arquivos sem compartilhar caminhos locais.
- `discovery.rs` anuncia e encontra salas por UDP nas interfaces IPv4 e confirma
  os candidatos por TCP, em segundo plano.
- `network/rooms.rs` implementa descoberta, aprovação limitada a uma vaga e
  tokens internos para reconexão. As tarefas de conexão terminam ao fechar a sala.
- `network/relay.rs` implementa as salas pela internet: transporte TLS via
  `tokio-rustls`/`rustls` (`ring` e `webpki-roots`), esquemas `tls://` e
  `tcp://` (TLS por padrão em nomes públicos, TCP puro em IPs literais e
  `localhost`, sem downgrade silencioso), código da sala, conexão de controle
  do anfitrião, entradas por conexões de dados e reconexão.
- `network/secure.rs` deriva a chave do código com SPAKE2 e abre o canal Noise
  cifrado. As mensagens da sala passam pelos mesmos traços `Inbound`/`Outbound`
  usados pelo TCP aberto, então sessão e sincronização não mudam.
- `relay/` é um crate separado no workspace, com o servidor e o protocolo de
  controle. Depende de Tokio, Serde e `tokio-rustls`/`rustls` (`ring`), aceita
  TLS nativo opcional (`--tls-cert`/`--tls-key` ou
  `RELAY_TLS_CERT`/`RELAY_TLS_KEY`) ou TCP puro atrás de proxy TLS, e não
  conhece as mensagens da sala.
- `network.rs` implementa TCP com mensagens enquadradas, limite de tamanho,
  autenticação da sala, timeout de heartbeat e reconexão.
- `protocol.rs` define as mensagens Serde e a versão do protocolo.
- `session.rs` ordena comandos no anfitrião e controla prontidão e pausa.
- `sync.rs` estima a diferença entre relógios por ping e calcula correções.
- `runtime.rs` define a interface pública (`Runtime`, `Command`, `View`) e o
  loop principal do worker, dividido entre `runtime/rooms.rs` (salas, descoberta
  e eventos de rede), `runtime/files.rs` (seleção e verificação de arquivos),
  `runtime/sync.rs` (controle de reprodução, snapshots e correção de desvio) e
  `runtime/view.rs` (projeção do estado para a UI). A UI só apresenta os dados e
  devolve ações.

### Salas pela internet

As conexões com o relay usam TLS por padrão para nomes públicos (nativo no
`sync2gether-relay` ou terminado em proxy TCP como Traefik/Caddy), protegendo o
canal de controle, o número da sala e o segredo de 128 bits do anfitrião. O
anfitrião mantém uma conexão de controle com o relay. Cada entrada chega
como um aviso nessa conexão; o anfitrião abre uma conexão de dados própria e
o relay junta as duas, passando a repassar bytes. Assim o lado do anfitrião se
comporta como um `accept` de TCP, e cada reconexão do convidado é uma nova
conexão de dados. Na conexão pareada, dentro do transporte TLS, o convidado
envia SPAKE2 (primeira entrada) ou pedido de retomada, e os dois fazem o
handshake Noise antes de qualquer mensagem da sala. Formato, limites e
garantias estão em [Salas pela internet e relay](relay.md).

O relógio filtrado mede o RTT através do relay, então a margem do início
agendado já inclui o desvio geográfico. O teste MVP pelo relay usa
~110 ms por sentido entre convidado e relay, com jitter de até 30 ms.

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
válidas são necessárias antes de iniciar. Uma medição vale por 10 s: relógios
monotônicos divergem menos de 0,1 ms por segundo, então uma retenção curta da
rede não invalida a estimativa nem pausa o filme. A conexão é considerada
perdida após 5 s sem receber nada do parceiro, que envia algo a cada 200 ms;
só então os dois lados pausam e o convidado tenta reconectar. Assimetria de
rede continua sendo uma fonte de incerteza, não eliminada por esse cálculo.

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
