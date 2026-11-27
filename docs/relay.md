# Salas pela internet e relay

Como as salas funcionam fora da rede local, o que o servidor intermediário vê
e como publicá-lo.

[Voltar ao README](../README.md)

## Visão geral

Os dois aplicativos abrem conexões de saída para o relay, então funcionam atrás
de qualquer NAT ou firewall que permita TCP/TLS de saída na porta `7843`. O
relay só pareia as conexões e repassa bytes. O vídeo nunca passa por ele: cada
PC continua reproduzindo a própria cópia. O tráfego de uma sala é da ordem de
poucos kB/s.

O relay padrão é `relay.pedrosales.dev:7843`. Outro servidor pode ser
informado em **Configurações → Opções avançadas de rede**, por `--relay` ou
pela variável `SYNC2GETHER_RELAY`.

O endereço aceita os esquemas explícitos `tls://host[:porta]` e
`tcp://host[:porta]`, além de endereços sem esquema:

- **nomes de host públicos** sem esquema (como `relay.pedrosales.dev:7843`)
  usam **TLS por padrão**;
- **endereços IP literais** (`127.0.0.1:7843`, `[::1]:7843`) e `localhost` sem
  esquema usam **TCP puro**, facilitando testes e uso local;
- `tls://` força TLS (inclusive em IP ou `localhost`) e `tcp://` força TCP puro.

Uma conexão configurada para TLS nunca faz downgrade silencioso para TCP puro
em caso de erro de certificado ou handshake.

## Código da sala

O código tem a forma `4821-gato-lua`:

- o **número** é atribuído pelo relay e serve apenas para encontrar a sala;
- as **duas palavras** são sorteadas pelo anfitrião, numa lista de 256 palavras
  sem acentos, e nunca saem do computador dele.

A digitação aceita maiúsculas, acentos e espaços no lugar dos hífens.

## Segurança

- O transporte até o relay usa **TLS** (`rustls` com `ring` e raízes
  `webpki-roots`), autenticando o certificado e o nome do servidor e protegendo
  em trânsito o canal de controle (`Host`, `Accept`, `Join`, `Alive`), o número
  da sala e o segredo de 128 bits do anfitrião. Certificados não confiáveis ou
  com nome incompatível são recusados sem downgrade para TCP puro.
- Por dentro do transporte, na primeira entrada, os dois aplicativos executam
  SPAKE2 (curva Ed25519) com o código completo. O resultado é uma chave de 256
  bits. Quem observa ou controla o caminho, inclusive o próprio relay ou um
  proxy TLS à frente dele, não aprende o código e só testa um palpite por
  conexão.
- Cada conexão faz um handshake Noise `NNpsk0_25519_ChaChaPoly_BLAKE2s` com essa
  chave e chaves efêmeras novas. As mensagens da sala seguem cifradas com
  ChaCha20-Poly1305 e nonces sequenciais: quadros alterados, repetidos ou
  reordenados encerram a conexão.
- O nome do computador é trocado já dentro do canal cifrado.
- Três tentativas com código errado fecham a sala. Quem acerta o código entra
  sem aprovação; há uma vaga de convidado por vez.
- As reconexões repetem apenas o Noise com a chave guardada em memória. Uma
  nova entrada com o código substitui essa chave.
- O relay vê endereços IP, horários, tamanhos de pacote e o número da sala.
  Ele não vê as palavras, as chaves, os nomes dos computadores, os nomes dos
  arquivos nem os comandos. Mesmo comprometido, não consegue ler nem injetar
  mensagens; consegue apenas derrubar ou atrasar a conexão.

## Reconexão

- O convidado reconecta sozinho a cada segundo, sem pedir o código de novo.
- O anfitrião mantém uma conexão de controle com mensagens de vida a cada 15 s.
  Se ela cair, a sala fica reservada por 2 minutos e o anfitrião a retoma com o
  mesmo número.
- Se o relay reiniciar, o anfitrião recria a sala com o mesmo número e segredo,
  e o convidado volta a entrar com a chave guardada. Durante essa janela, o
  convidado tolera até 45 s de "sala inexistente" antes de avisar que a sala
  foi encerrada.
- Fechar o app do anfitrião ou clicar em **Encerrar sala** fecha a sala no
  relay imediatamente.
- Quem sai envia `Bye` pelo canal cifrado. O outro lado mostra que a pessoa
  saiu ou que a sala foi encerrada, em vez de "Conexão perdida", e o
  convidado para de reconectar.

A latência acrescentada é o desvio geográfico até o relay. A margem do início
agendado já se ajusta ao RTT medido, que inclui esse desvio.

## Protocolo do relay

Mensagens de controle em JSON com prefixo de tamanho de 32 bits, até 1 KiB,
versionadas por `sync2gether_relay::VERSION` (independente do protocolo da sala):

| Conexão | Pedido | Resposta |
| --- | --- | --- |
| Controle do anfitrião | `Host { room?, secret }` | `Hosting { room }`, depois `Incoming { ticket }` e `Alive` |
| Dados do anfitrião | `Accept { room, secret, ticket }` | `Paired` e bytes do convidado |
| Dados do convidado | `Join { room }` | `Paired` e bytes do anfitrião, ou `Unknown`, `Away`, `Limited` |

O segredo do anfitrião tem 128 bits aleatórios e trafega protegido pelo TLS
entre cliente e relay. Sem ele, ninguém assume o número nem aceita entradas na
sala. Quando o TLS nativo está ativo no relay, o handshake TLS tem timeout de
5 s antes do primeiro quadro de controle para evitar esgotamento de vagas.

Limites padrão: 4096 conexões, 32 por IP, 10 000 salas, 8 salas por IP, 4
entradas pendentes e 2 conexões pareadas por sala, 256 KiB/s por sentido e
30 s de silêncio numa conexão pareada. Os registros mostram apenas contadores
agregados a cada hora, sem números de sala nem endereços.

## Navegadores por WebSocket

A página no [navegador](navegador.md) não abre TCP puro, então o relay pode
escutar também um WebSocket em `/relay`, numa porta separada (`--ws-listen` ou
`RELAY_WS_LISTEN`). Cada mensagem binária do WebSocket carrega um trecho do
mesmo fluxo de bytes da conexão TCP: o navegador envia `Host`, `Accept`, `Alive`
ou `Join`, SPAKE2 e Noise exatamente como o app, com as mesmas validações e
limites por endereço e globais. O outro participante (no app ou no navegador)
não percebe diferença de transporte. Para criar salas pelo navegador em
produção, o relay publicado precisa estar atualizado com essa versão.

O WebSocket não tem TLS próprio: publique-o atrás de um proxy HTTPS (Coolify,
Traefik, Caddy ou Cloudflare). Opções:

| Opção | Variável | Efeito |
| --- | --- | --- |
| `--ws-listen ENDEREÇO:PORTA` | `RELAY_WS_LISTEN` | Ativa o WebSocket |
| `--ws-origin ORIGEM` (repetível) | `RELAY_WS_ORIGINS` (vírgulas) | Só aceita páginas dessas origens; sem a opção, qualquer origem |
| `--ws-client-ip-header NOME` | `RELAY_WS_CLIENT_IP_HEADER` | Lê o IP do cliente nesse cabeçalho |

Os limites por IP usam o endereço da conexão. Atrás de um proxy, todas as
conexões vêm do proxy; com `RELAY_WS_CLIENT_IP_HEADER`, o relay usa o último
endereço do cabeçalho indicado, mas **só quando a conexão vem de um endereço
local ou privado** (o proxy na mesma rede). Escolha um cabeçalho que o proxy
mais próximo sempre sobrescreve: `x-real-ip` no Traefik ou `cf-connecting-ip`
atrás da Cloudflare. Pedidos sem o cabeçalho configurado são recusados.

O upgrade e o primeiro quadro têm 5 s de prazo, no máximo 256 upgrades ficam
pendentes ao mesmo tempo e cada mensagem tem até 24 KiB. Os registros continuam
só com contadores agregados.

## Executar localmente

Sem certificados (TCP puro, padrão para `127.0.0.1` e `localhost`):

```sh
cargo run --locked -p sync2gether-relay -- --listen 127.0.0.1:7843
cargo run --locked -- --internet --relay 127.0.0.1:7843
cargo run --locked -- --code 4821-gato-lua --relay 127.0.0.1:7843
```

Com TLS nativo no relay (`--tls-cert` e `--tls-key`, ou `RELAY_TLS_CERT` e
`RELAY_TLS_KEY`; informar apenas um dos dois encerra com erro):

```sh
cargo run --locked -p sync2gether-relay -- \
  --listen 0.0.0.0:7843 \
  --tls-cert /caminho/para/fullchain.pem \
  --tls-key /caminho/para/privkey.pem
```

Para forçar TLS ou TCP explicitamente no cliente, use `tls://host:7843` ou
`tcp://host:7843`.

Abrir janelas segue as regras do [guia de desenvolvimento](desenvolvimento.md).

## Publicar com Docker e Coolify

A imagem usa um binário estático em `FROM scratch`, rodando como usuário sem
privilégios (`65534:65534`):

```sh
docker build -f relay/Dockerfile -t sync2gether-relay .
```

Há duas formas suportadas de terminar TLS em produção:

1. **TLS nativo no `sync2gether-relay`**: monte os arquivos PEM do certificado
   e da chave privada no container (legíveis pelo UID `65534`) e configure
   `RELAY_TLS_CERT` e `RELAY_TLS_KEY` (ou `--tls-cert` / `--tls-key`),
   publicando a porta `7843:7843`:

   ```sh
   docker run --rm -p 7843:7843 \
     -v /etc/certs:/certs:ro \
     -e RELAY_TLS_CERT=/certs/fullchain.pem \
     -e RELAY_TLS_KEY=/certs/privkey.pem \
     sync2gether-relay
   ```

2. **Proxy com terminação TLS TCP (Coolify / Traefik / Caddy)**: omita
   `RELAY_TLS_CERT` e `RELAY_TLS_KEY` para que o `sync2gether-relay` escute em
   TCP puro na rede interna, enquanto o proxy termina TLS na porta `7843`
   (por exemplo, um router TCP com TLS/ACME no Traefik ou o módulo `layer4`
   do Caddy) e encaminha o fluxo TCP para o container do relay:

   ```sh
   docker run --rm -p 127.0.0.1:7843:7843 sync2gether-relay
   ```

Para receber navegadores, defina também `RELAY_WS_LISTEN=0.0.0.0:8080` e
publique essa porta atrás do proxy HTTPS (ou pelo container `web/Dockerfile`,
que repassa `/relay`), como descrito em [Convidado no navegador](navegador.md#publicação).

No Coolify, a aplicação usa este repositório com o build pack Dockerfile:

- base directory `/` e Dockerfile `/relay/Dockerfile`;
- sem proxy HTTP e sem health check HTTP (o protocolo não é HTTP);
- ou exponha `7843:7843` diretamente com TLS nativo (`RELAY_TLS_CERT` e
  `RELAY_TLS_KEY`), ou configure um router TCP com terminação TLS (Traefik/Caddy)
  na porta `7843` apontando para o serviço interno em TCP puro.

O registro DNS do relay precisa apontar direto para o servidor. Com Cloudflare,
use o modo **somente DNS** (nuvem cinza): o proxy da Cloudflare só repassa HTTP.
Libere `7843/tcp` em firewalls do servidor ou do provedor.

Um reinício do relay derruba as conexões por alguns segundos; as salas se
recompõem sozinhas, como descrito em [Reconexão](#reconexão).
