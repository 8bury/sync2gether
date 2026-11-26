# Convidado no navegador

O convidado pode entrar numa sala pela internet sem instalar o app: basta abrir
um link no navegador. O anfitrião continua usando o app de desktop. No
navegador, a pessoa assiste com a **própria cópia** do filme ou recebe o
arquivo por **streaming P2P direto** do computador do anfitrião.

## Como usar

1. O anfitrião cria uma sala pela internet. A tela de espera e os detalhes da
   conexão mostram **Copiar link para navegador**.
2. O link tem o formato `https://sync2gether.pedrosales.dev/#4821-gato-lua`. O
   código fica depois do `#`: o navegador nunca o envia a um servidor.
3. O convidado abre o link e escolhe:
   - **Usar minha cópia**: o navegador lê o arquivo local, calcula a mesma
     identidade BLAKE3 do app e só libera a reprodução se as cópias forem
     iguais;
   - **Assistir por streaming**: o navegador conecta direto ao app do
     anfitrião por WebRTC e recebe o arquivo em blocos verificados.
4. Play, pause e mudanças de posição seguem o anfitrião, como no app.

Também é possível abrir a página sem link e colar o código.

## O que funciona no navegador

A reprodução usa o `<video>` do navegador, sem decodificação própria. Funcionam
os formatos que o navegador toca: em geral **MP4 com H.264 e AAC** e **WebM com
VP8/VP9/AV1 e Opus**. A página avisa quando o arquivo do anfitrião tem uma
extensão que costuma falhar (MKV, AVI, TS e outras) e quando o vídeo abre sem
uma faixa reproduzível. Áudio **AC-3, E-AC-3 e DTS**, comum em arquivos MKV,
não toca na maioria dos navegadores; a página avisa quando percebe que o áudio
não está sendo decodificado. Legendas e troca de faixas ficam para o app.

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
  app: `Join`, SPAKE2 com o código e o canal Noise. O relay continua sem ver o
  código nem o conteúdo da sala.
- O código é lido do fragmento do link e removido da barra de endereço logo em
  seguida. Para sobreviver a um recarregamento, ele e a chave de reconexão
  ficam no `sessionStorage` da aba, apagado ao sair da sala.
- No streaming, a oferta e a resposta WebRTC (incluindo as impressões digitais
  DTLS) passam pelo canal cifrado da sala, então outro computador não consegue
  se passar pelo anfitrião. Cada bloco é conferido com a tabela de hashes, cujo
  resumo também veio pelo canal cifrado.
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

O código fica em:

- `core/`: mensagens, relógio, código da sala, handshake SPAKE2/Noise sem I/O,
  hashes dos blocos e formato do canal de dados, compilados para o app, o relay
  e o WebAssembly;
- `web/src/`: o convidado sem I/O (`guest.rs`, `link.rs`) e o cliente de blocos
  do streaming (`stream.rs`), exportados para JavaScript em `bindings.rs`;
- `web/static/`: a página, o adaptador do `<video>`, o Worker de hash e o
  Service Worker que responde aos pedidos de trecho do vídeo;
- `src/stream/webrtc.rs` e `src/runtime/web_stream.rs`: o lado do anfitrião,
  com `str0m` e criptografia em Rust puro.

Testes:

- `tests/web.rs`: relay real com WebSocket, anfitrião real e o convidado do
  navegador compilado nativamente (código errado, origem recusada, relógio,
  preparação, início agendado, reconexão e saída);
- `tests/web_stream.rs`: WebRTC do anfitrião contra um cliente `str0m` em
  localhost, com tabela de hashes, blocos verificados, recusa e cancelamento;
- `bash scripts/test-browser.sh`: Firefox headless (WebDriver BiDi) e Chrome
  ou Chromium headless (DevTools) contra o runtime real do anfitrião com um
  MP4 sintético (H.264 e AAC), nos dois modos. Requer Firefox, ffmpeg com
  libx264 e libmpv; sem Chrome (ou `SYNC2GETHER_TEST_CHROME`), os casos do
  Chrome são pulados. Não faz parte do `check.sh`. Com
  `SYNC2GETHER_TEST_PUBLIC_URL=https://sync2gether.pedrosales.dev`, os mesmos
  casos usam a página publicada e o relay padrão, para conferir a publicação.

## Validação

Em CachyOS (Arch), com relay e anfitrião em localhost, os quatro casos de
`scripts/test-browser.sh` passaram no Firefox 160 e no Chrome 157 headless: a
página entra pelo link, tira o código da barra de endereço, confere a cópia
local ou recebe o filme por WebRTC, começa junto com o anfitrião e pausa
quando ele pausa. A diferença medida pelo anfitrião ficou abaixo de 80 ms.
Os testes usam reprodução sem som e autoplay liberado nas preferências do
navegador de teste. Os mesmos quatro casos também passaram contra a página
publicada em `https://sync2gether.pedrosales.dev` e o relay padrão (TLS na
porta 7843 e WebSocket atrás de Traefik e Caddy), com anfitrião e navegador no
mesmo computador.

Ainda não foram testados: Safari (desktop e iOS), navegadores de celular,
redes reais com NAT entre os dois lados, arquivos MKV ou com áudio AC-3/DTS,
filmes longos e a página atrás do proxy da Cloudflare.
