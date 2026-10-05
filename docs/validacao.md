# Histórico de validação

Registro das revisões anteriores. Cada etapa informa o ambiente e os cenários exercitados; consulte também a [validação dos pacotes 0.1.0](releases/v0.1.0.md#validação-e-limites).

[Voltar ao README](../README.md)

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
