# Navegador (anfitrião e convidado)

É possível usar o sync2gether pelo navegador sem instalar o app: tanto para
**entrar como convidado** (com link ou código) quanto para **criar uma sala
como anfitrião**. Nos dois casos, o convidado no navegador pode assistir com a
**própria cópia** ou receber o arquivo por **streaming P2P direto (WebRTC)**
sem passar vídeo pelo servidor relay.

## Como usar

### Entrar como convidado

1. O anfitrião cria uma sala pela internet (no app ou no navegador) e copia o
   código ou o link.
2. O link tem o formato `https://sync2gether.pedrosales.dev/#4821-gato-lua`. O
   código fica depois do `#`: o navegador nunca o envia a um servidor.
3. O convidado abre o link e escolhe:
   - **Usar minha cópia**: o navegador lê o arquivo local, calcula a mesma
     identidade BLAKE3 do app e só libera a reprodução se as cópias forem
     iguais;
   - **Assistir por streaming**: disponível quando o anfitrião está no app de
     desktop ou no navegador com arquivo compatível; o navegador conecta direto
     por WebRTC e recebe o arquivo em blocos verificados (ou segmentos fMP4 via
     MSE no caso de MKV).
4. Play, pause e mudanças de posição seguem o anfitrião, como no app.

Também é possível abrir a página sem link e colar o código.

### Criar sala pelo navegador (anfitrião)

1. Abra a página e clique em **Criar sala**. O navegador abre uma conexão de
   controle por WebSocket com o relay, gera o código da sala (com as duas
   palavras sorteadas localmente) e exibe os botões **Copiar código** e
   **Copiar link**.
2. Escolha a sua cópia local do filme. O convidado pode entrar pelo app de
   desktop (com a própria cópia) ou pelo navegador (com a própria cópia ou por
   **streaming P2P direto via WebRTC**).
3. Assim que as duas cópias forem conferidas (ou o streaming estiver conectado)
   e os relógios estiverem prontos, clique em **Começar a assistir** para
   preparar os dois players e agendar o início em conjunto.
4. **Limites do anfitrião no navegador**:
   - A aba do anfitrião precisa permanecer aberta e visível enquanto a sala e o
     streaming estiverem ativos. Navegadores reduzem a atividade de abas em
     segundo plano; o envio é guiado pelos eventos do canal de dados WebRTC
     (`onmessage` e `bufferedamountlow`) e por um Worker (`ticker.js`), mas a
     página avisa se a aba for ocultada durante o streaming.
   - **Formatos no streaming a partir do navegador**: arquivos que o navegador
     toca diretamente (MP4 com H.264/VP9/AV1 e AAC/Opus/MP3, WebM) são enviados
     em blocos verificados pela mesma tabela BLAKE3 do app. Para **MKV** com
     vídeo H.264/HEVC e áudio AAC, Opus ou MP3, um Worker em WebAssembly
     reempacota os trechos sob demanda para fMP4 (MSE) sem recodificar vídeo e
     com escolha de faixa de áudio. Áudio **AC-3, E-AC-3 e DTS** exige
     decodificador e não é suportado quando o anfitrião está no navegador (a
     página avisa claramente para usar o app de desktop como anfitrião ou um
     arquivo compatível).
   - **Convidado no app de desktop**: o streaming P2P do app de desktop usa
     QUIC, não WebRTC, então um convidado no app de desktop assiste com a
     própria cópia quando o anfitrião está no navegador (se tentar streaming P2P
     QUIC, a negociação expira após o limite de 15 s).
   - O servidor relay publicado precisa estar atualizado com suporte a `Host`,
     `Accept` e `Alive` por WebSocket.

## O que funciona no navegador

A reprodução usa o `<video>` do navegador, sem decodificação própria:

- **Com a própria cópia do convidado**, funcionam os formatos que o navegador toca
  nativamente: em geral **MP4 com H.264 e AAC** e **WebM com VP8/VP9/AV1 e
  Opus**. Áudio **AC-3, E-AC-3 e DTS** não toca direto na maioria dos
  navegadores; a página avisa quando percebe que o áudio não está sendo
  decodificado.
- **Por streaming P2P direto**, formatos nativos de faixa única (como MP4 com
  H.264 e AAC ou WebM com VP8/VP9/AV1 e Opus) continuam usando o envio direto do
  arquivo original em blocos verificados por BLAKE3. Para **MKV** (e outros
  contêineres como AVI, TS e FLV), áudio **AC-3, E-AC-3 ou DTS**, ou arquivos
  com **múltiplas faixas de áudio**, o app do anfitrião reempacota o fluxo sob
  demanda para **MP4 fragmentado (fMP4)** usando as bibliotecas do FFmpeg
  carregadas dinamicamente (`libavformat`, `libavcodec`, `libswresample` e
  `libavutil`), **sem recodificar o vídeo** e convertendo apenas o áudio
  incompatível para AAC estéreo. A página reproduz o fluxo por **Media Source
  Extensions (MSE)**, usa um índice de quadros-chave para buscar qualquer
  posição rapidamente e permite escolher a faixa de áudio no menu de faixas do
  player sem perder a sincronia da sala.
- Se o computador do anfitrião não tiver as bibliotecas do FFmpeg disponíveis
  (por exemplo, no pacote Windows atual, onde o FFmpeg fica estático dentro da
  `libmpv-2.dll`), o anfitrião faz fallback automático para o streaming direto do
  arquivo original com os avisos de formato habituais. Se o navegador não
  suportar o codec de vídeo (por exemplo, HEVC em navegadores sem decodificador),
  a página avisa para usar o app de desktop, já que recodificar vídeo fica fora
  do escopo.

A página também suporta legendas via `<track>` e WebVTT:
- **Legendas embutidas no streaming**: quando o anfitrião tem as bibliotecas do
  FFmpeg disponíveis, faixas de texto embutidas no arquivo (SRT/SubRip, ASS/SSA,
  WebVTT, `text` e `mov_text`) aparecem no menu de faixas do player. Ao escolher
  uma faixa, o anfitrião extrai as falas sob demanda, converte para WebVTT (no
  caso de ASS/SSA, remove tags de estilo e comandos de desenho, mantendo o texto
  puro) e envia pelo canal cifrado da sala usando o mesmo transporte em pedaços
  das legendas externas. Legendas baseadas em imagem (PGS, VobSub, DVB) não têm
  texto e são ignoradas na listagem.
- **Legendas externas (`.srt` e `.vtt`)**: o convidado pode abrir um arquivo
  local (`.srt` é convertido para WebVTT na página) ou receber a legenda que o
  anfitrião compartilhar pelo canal cifrado da sala. Quando existem faixas
  embutidas e arquivo compartilhado ou local ao mesmo tempo, todos aparecem
  juntos no menu de faixas, com controle para ativar, desativar e ajustar o
  atraso.

O streaming guarda até 256 MiB de blocos na memória (128 MiB em celulares e
aparelhos com pouca memória). Não há cache em disco: depois de uma
reconexão longa, os blocos são pedidos de novo.

Não há servidor TURN. Quando as duas redes bloqueiam conexão direta (NAT
simétrico nos dois lados, por exemplo), o streaming falha com uma mensagem
clara e a pessoa pode usar a própria cópia ou o app. O vídeo nunca passa pelo
relay.

Se o navegador bloquear o início com som, a página tenta começar sem som e
mostra **Toque para ativar o som**; se nem assim conseguir, mostra **Toque para
iniciar**.

## Segurança e privacidade

- A página entra no relay por WebSocket e envia exatamente os mesmos bytes que o
  app: `Join` (convidado) ou `Host`/`Accept`/`Alive` (anfitrião), SPAKE2 com o
  código e o canal Noise. O relay continua sem ver o código nem o conteúdo da
  sala.
- No convidado, o código é lido do fragmento do link e removido da barra de
  endereço logo em seguida. Para sobreviver a um recarregamento, o estado da
  sala (código/chave no convidado; número da sala, segredo, palavras e chave de
  retomada no anfitrião) fica apenas no `sessionStorage` da aba, apagado ao sair
  ou encerrar a sala, nunca em logs ou URLs.
- No streaming, a oferta e a resposta WebRTC (incluindo as impressões digitais
  DTLS) passam pelo canal cifrado da sala, então outro computador não consegue
  se passar pelo anfitrião. No envio direto do arquivo original, cada bloco é
  conferido com a tabela de hashes BLAKE3, cujo resumo também veio pelo canal
  cifrado. No modo MSE, o índice de quadros-chave, o segmento de inicialização e
  os fragmentos fMP4 são gerados sob demanda pelo anfitrião e trafegam pelo
  mesmo canal de dados WebRTC (SCTP sobre DTLS autenticado com as impressões
  digitais trocadas no canal Noise), mantendo integridade e autenticidade de
  ponta a ponta sem envolver o relay.
- **Limite importante:** quem publica a página controla o JavaScript que lê o
  fragmento. A garantia de "o servidor não vê o código" vale enquanto a página
  servida for a deste repositório. O app de desktop não depende disso.
- A página publicada usa uma política de conteúdo estrita (`script-src 'self'`,
  `connect-src 'self'`, `frame-ancestors 'none'`) e não carrega nada de
  terceiros. Os servidores STUN públicos da Cloudflare e do Google são usados só
  para descobrir o endereço externo no WebRTC.

## Publicação

A página e o relay são dois containers:

- `relay/Dockerfile`: o relay de sempre. Com `RELAY_WS_LISTEN=0.0.0.0:8080` ele
  também aceita navegadores por WebSocket em `/relay`. Veja
  [Salas pela internet e relay](relay.md#navegadores-por-websocket).
- `web/Dockerfile`: compila o WebAssembly e serve a página com Caddy na porta
  `8080`. O caminho `/relay` é repassado para `RELAY_UPSTREAM` (por padrão
  `sync2gether-relay:8080`), então a página e o WebSocket ficam na mesma origem.

No Coolify, publique o `web` com um domínio HTTPS (por exemplo
`assistir.exemplo.com`) e o relay na mesma rede Docker. No relay, configure:

```sh
RELAY_WS_LISTEN=0.0.0.0:8080
RELAY_WS_ORIGINS=https://assistir.exemplo.com
RELAY_WS_CLIENT_IP_HEADER=x-real-ip
```

Use `cf-connecting-ip` em vez de `x-real-ip` quando a página estiver atrás do
proxy da Cloudflare.

A página publicada é `https://sync2gether.pedrosales.dev`, usada por padrão
quando a sala está no relay padrão. Com outro relay, informe a página dele com
`--web-url URL` (ou `SYNC2GETHER_WEB_URL`); sem isso o app não mostra o link,
porque a página só conversa com o relay da própria origem. `--web-url ""`
desliga o link.

## Desenvolvimento

```sh
bash scripts/build-web.sh               # gera target/web
cargo run --locked -p sync2gether-relay -- \
  --listen 127.0.0.1:7843 --ws-listen 127.0.0.1:7844 \
  --ws-origin http://127.0.0.1:8000
SYNC2GETHER_WEB_RELAY_URL=ws://127.0.0.1:7844/relay bash scripts/build-web.sh
python3 -m http.server -d target/web 8000
cargo run --locked -- --internet --relay 127.0.0.1:7843 --web-url http://127.0.0.1:8000
```

`scripts/build-web.sh` instala o `wasm-bindgen-cli` da versão do `Cargo.lock` em
`target/tools` na primeira vez. Abrir o app com janela segue as regras do
[guia de desenvolvimento](desenvolvimento.md).

Para revisar ou capturar telas do navegador com estados fictícios, sem conectar
a um relay, gere o build de desenvolvimento com `--preview`:

```sh
bash scripts/build-web.sh --preview
python3 -m http.server -d target/web 8000
firefox --headless --screenshot="$PWD/.cache/web-paused.png" --window-size=1440,900 \
  "http://127.0.0.1:8000/?preview=paused"
```

Os cenários disponíveis em `web/preview/` são `entry`, `connecting`, `ended`,
`prepare`, `mkv-prepare`, `mkv-prepare-fallback`, `mkv-streaming`, `audio-menu`,
`player`, `paused`, `reconnecting`, `buffering`, `subtitles`,
`subtitles-menu`, `host-waiting`, `host-matched`, `host-mismatch`,
`host-playing`, `host-streaming`, `host-streaming-playing`, `host-mkv-remux`,
`host-mkv-unsupported` e `guest-browser-host-stream`, com `&theme=light` ou
`&theme=dark` opcionais. O build padrão
(`bash scripts/build-web.sh`) e o `web/Dockerfile` nunca incluem `web/preview/`
e ignoram `?preview=`.

O código fica em:

- `core/`: mensagens, relógio, código da sala, handshake SPAKE2/Noise sem I/O,
  hashes dos blocos, formato do canal de dados, conversão/deslocamento de
  legendas (`subtitle.rs`) e tokens de tema (`theme.rs`), compilados para o app,
  o relay e o WebAssembly;
- `web/src/`: o convidado sem I/O (`guest.rs`, `link.rs`), o anfitrião sem I/O
  (`host.rs`, `host_link.rs`, usando `sync2gether_core::host::Host`), o
  servidor do canal de dados do anfitrião (`host_stream.rs`), o remuxer
  MKV para fMP4 em WebAssembly (`remux.rs` e `remux/parse.rs`) e o cliente
  de blocos do streaming (`stream.rs`), exportados para JavaScript em
  `bindings.rs`;
- `web/static/`: a página, os tokens CSS gerados a partir de `core/src/theme.rs`
  (`tokens.css`, verificados por teste em `tests/web.rs`), o controlador do
  anfitrião (`host.js`), o streaming WebRTC e MSE local do anfitrião
  (`host-stream.js`), o Worker de remux MKV (`remux-worker.js`), os controles e
  avisos sobre o vídeo no mesmo visual do app (`ui.js`, `subtitles.js`,
  `style.css`), o adaptador do `<video>`, o Worker de hash e o Service Worker
  que responde aos pedidos de trecho do vídeo;
- `web/preview/`: fixtures e ponto de entrada usados apenas com
  `scripts/build-web.sh --preview` para revisão visual e capturas em `.cache/`;
- `src/stream/webrtc.rs`, `src/stream/ffmpeg.rs`, `src/stream/remux.rs` e
  `src/runtime/web_stream.rs`: o lado do anfitrião, com `str0m`, carregamento
  dinâmico das bibliotecas FFmpeg e reempacotamento incremental em fMP4.

Testes:

- `tests/web.rs`: relay real com WebSocket, anfitrião real e o convidado do
  navegador compilado nativamente (código errado, origem recusada, relógio,
  preparação, início agendado, reconexão, compatibilidade de protocolo e saída);
- `tests/web_host.rs`: relay real com WebSocket (`Host`/`Accept`/`Alive` e
  limites), anfitrião do navegador compilado nativamente contra convidado de
  desktop e contra convidado do navegador (criação de sala, código errado,
  início em sincronia, pausa, seek, retomada com `INTRO_RESUME` e reconexão do
  controle do relay);
- `tests/web_stream.rs`: WebRTC do anfitrião contra um cliente `str0m` em
  localhost, com tabela de hashes, blocos verificados, recusa e cancelamento;
- `tests/remux.rs`: reempacotamento incremental de MKV sintético com H.264 e
  duas faixas AC-3, índice de quadros-chave, timestamps `tfdt`, troca de faixa
  de áudio, fallback sem bibliotecas FFmpeg e entrega pelo canal de dados WebRTC;
- `bash scripts/test-browser.sh`: Firefox headless (WebDriver BiDi) e Chrome
  ou Chromium headless (DevTools) contra o runtime real do anfitrião com um
  MP4 sintético (H.264 e AAC) nos dois modos e um MKV sintético (H.264 com duas
  faixas AC-3) via MSE com busca para trecho não carregado e troca de faixa de
  áudio. Requer Firefox, ffmpeg com libx264 e libmpv; sem Chrome (ou
  `SYNC2GETHER_TEST_CHROME`), os casos do Chrome são pulados. Não faz parte do
  `check.sh`. Com `SYNC2GETHER_TEST_PUBLIC_URL=https://sync2gether.pedrosales.dev`,
  os mesmos casos usam a página publicada e o relay padrão, para conferir a
  publicação.

## Validação

Em CachyOS (Arch) e Ubuntu 24.04, com relay e anfitrião em localhost, os casos
de `scripts/test-browser.sh` passaram no Firefox e no Chrome headless: a página
entra pelo link, tira o código da barra de endereço, confere a cópia local ou
recebe o filme por WebRTC (tanto MP4 direto quanto MKV com duas faixas AC-3 via
MSE, incluindo busca para posição não carregada e troca de faixa de áudio),
começa junto com o anfitrião e pausa quando ele pausa. A diferença medida pelo
anfitrião ficou abaixo de 80 ms. Os testes usam reprodução sem som e autoplay
liberado nas preferências do navegador de teste. Os quatro casos originais
também passaram contra a página publicada em `https://sync2gether.pedrosales.dev`
e o relay padrão (TLS na porta 7843 e WebSocket atrás de Traefik e Caddy), com
anfitrião e navegador no mesmo computador.

Ainda não foram testados: Safari (desktop e iOS), navegadores de celular,
redes reais com NAT entre os dois lados, filmes longos em MSE e a página atrás
do proxy da Cloudflare.
