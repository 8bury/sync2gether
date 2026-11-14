# Salas pela internet e relay

Como as salas funcionam fora da rede local, o que o servidor intermediário vê
e como publicá-lo.

[Voltar ao README](../README.md)

## Visão geral

Os dois aplicativos abrem conexões de saída para o relay, então funcionam atrás
de qualquer NAT ou firewall que permita TCP de saída na porta `7843`. O relay
só pareia as conexões e repassa bytes. O vídeo nunca passa por ele: cada PC
continua reproduzindo a própria cópia. O tráfego de uma sala é da ordem de
poucos kB/s.

O relay padrão é `relay.pedrosales.dev:7843`. Outro servidor pode ser
informado em **Configurações → Opções avançadas de rede**, por `--relay` ou
pela variável `SYNC2GETHER_RELAY`.

## Código da sala

O código tem a forma `4821-gato-lua`:

- o **número** é atribuído pelo relay e serve apenas para encontrar a sala;
- as **duas palavras** são sorteadas pelo anfitrião, numa lista de 256 palavras
  sem acentos, e nunca saem do computador dele.

A digitação aceita maiúsculas, acentos e espaços no lugar dos hífens.

## Segurança

- Na primeira entrada, os dois aplicativos executam SPAKE2 (curva Ed25519) com
  o código completo. O resultado é uma chave de 256 bits. Quem observa ou
  controla o caminho, inclusive o próprio relay, não aprende o código e só
  testa um palpite por conexão.
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

O segredo do anfitrião tem 128 bits aleatórios. Sem ele, ninguém assume o
número nem aceita entradas na sala.

Limites padrão: 4096 conexões, 32 por IP, 10 000 salas, 8 salas por IP, 4
entradas pendentes e 2 conexões pareadas por sala, 256 KiB/s por sentido e
30 s de silêncio numa conexão pareada. Os registros mostram apenas contadores
agregados a cada hora, sem números de sala nem endereços.

## Executar localmente

```sh
cargo run --locked -p sync2gether-relay -- --listen 127.0.0.1:7843
cargo run --locked -- --internet --relay 127.0.0.1:7843
cargo run --locked -- --code 4821-gato-lua --relay 127.0.0.1:7843
```

Abrir janelas segue as regras do [guia de desenvolvimento](desenvolvimento.md).

## Publicar com Docker e Coolify

A imagem usa um binário estático em `FROM scratch`, rodando como usuário sem
privilégios:

```sh
docker build -f relay/Dockerfile -t sync2gether-relay .
docker run --rm -p 7843:7843 sync2gether-relay
```

No Coolify, a aplicação usa este repositório com o build pack Dockerfile:

- base directory `/` e Dockerfile `/relay/Dockerfile`;
- porta publicada `7843:7843`, fora do proxy HTTP, porque o tráfego é TCP puro;
- sem domínio HTTP e sem health check HTTP.

O registro DNS do relay precisa apontar direto para o servidor. Com Cloudflare,
use o modo **somente DNS** (nuvem cinza): o proxy da Cloudflare só repassa HTTP.
Libere `7843/tcp` em firewalls do servidor ou do provedor.

Um reinício do relay derruba as conexões por alguns segundos; as salas se
recompõem sozinhas, como descrito em [Reconexão](#reconexão).
