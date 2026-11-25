# Guia de uso

Salas pela internet com código, escolha dos arquivos, controles e salas na rede local ou VPN.

[Voltar ao README](../README.md)

## Usar o aplicativo

O fluxo passa por três momentos: **sala**, **filme** e **assistir**. Na tela
inicial ficam **Criar sala**, **Entrar com código** e **Abrir um filme sozinho**.
As salas na rede local ficam nas opções avançadas, descritas em
[Rede local ou VPN](#rede-local-ou-vpn).

1. O anfitrião clica em **Criar sala**. O app abre a sala no relay e mostra o
   código, como `4821-gato-lua`, em destaque, com o botão **Copiar código**, que passa
   a dizer **Copiado**. Enquanto o código não chega, a tela mostra
   **Gerando o código…**; sem contato com o servidor, avisa e tenta de novo.
   Abaixo, **Esperando alguém entrar…** mostra a vaga ainda livre, e
   **Escolher meu filme** permite escolher a cópia enquanto espera.
2. O anfitrião envia o código por qualquer mensageiro. O convidado abre
   **Entrar com código**, digita o código e clica em **Entrar** (ou Enter). Maiúsculas,
   acentos e espaços no lugar dos hífens são aceitos. O botão só fica disponível
   com o número e duas palavras da lista. Enquanto a sala responde, o botão
   mostra **Entrando** e **Cancelar**, no cabeçalho, desiste da tentativa.
   **Voltar** retorna à tela inicial.
3. Quem acerta o código entra direto na etapa **Filme**, sem aprovação. Um
   código errado mostra **Código incorreto** na linha abaixo do campo, mantém o
   texto para correção e avisa o anfitrião; na terceira tentativa errada a sala
   fecha por segurança. Há uma vaga de convidado por vez.
4. Cada pessoa escolhe sua cópia do filme e aguarda a verificação. O botão
   **Começar a assistir** só fica disponível quando a sessão estiver pronta;
   qualquer participante pode iniciar. O app não dá play automaticamente.

Depois da entrada, o código da sala fica discreto no cabeçalho; um clique copia.

Os dois PCs só precisam de TCP de saída para `relay.pedrosales.dev:7843`.
As mensagens são cifradas de ponta a ponta; o relay não vê o código completo,
os nomes dos arquivos nem os comandos. Se a conexão cair, o convidado volta
sozinho, sem digitar o código de novo. Detalhes em [Salas pela internet e relay](relay.md).

A etapa **Filme** mostra os dois arquivos lado a lado, **Você** à esquerda e a
outra pessoa à direita; em janelas estreitas as cópias ficam empilhadas. Cada
cópia mostra o estado, um quadro neutro e o nome do arquivo com extensão; o app
não exibe prévia do vídeo. Durante a verificação, o quadro mostra a porcentagem.
**Escolher arquivo** fica em destaque quando falta sua cópia; depois,
**Trocar arquivo** permanece junto ao nome. A escolha da outra pessoa não pode
ser alterada neste PC. Entre as cópias, dois círculos resumem a comparação:
separados enquanto esperam ou conferem, sobrepostos quando são iguais e com o
contorno de alerta quando são diferentes.

O título acompanha o estado: **Escolham o mesmo filme**, **Conferindo as
cópias**, **As cópias são diferentes**, **Quase pronto** e **Vocês têm o mesmo
filme** (**Tudo pronto** ao assistir sozinho). O
estado é dito uma vez, numa frase acima do botão principal. Enquanto não dá
para começar, o botão fica desativado e diz o que falta, como **Escolha seu
arquivo para começar** ou **Esperando o arquivo de Ana**. Com conteúdos
diferentes, as duas cópias mostram **Conteúdo diferente** e o botão vira
**Trocar meu arquivo**. Quando a sessão está pronta, o botão passa a ser
**Começar a assistir**.

Os dois participantes veem as duas escolhas. O menu **⋯** de cada arquivo reúne
**Nome completo** e **Copiar nome**, com confirmação da cópia. Esses menus têm
nomes acessíveis que distinguem seu arquivo do arquivo da outra pessoa.
Apenas o nome é compartilhado
dentro da sala, após a conexão autorizada, sem o caminho das pastas e sem
incluí-lo nos anúncios de descoberta. Caracteres de controle são substituídos
e o nome compartilhado é limitado a 1024 bytes.

Os nomes ajudam a comparar escolhas; a identidade continua sendo o hash BLAKE3
do conteúdo completo e o tamanho. Nomes diferentes podem resultar em arquivos
iguais; nomes iguais não liberam o play quando o conteúdo é diferente.
A verificação mostra porcentagem nos dois PCs. **Cancelar verificação** interrompe
a leitura entre blocos, restaura o arquivo anterior e mantém a sala aberta,
com reprodução pausada. A janela nativa de seleção tem seu próprio cancelamento.

Em janelas altas, o botão principal fica junto às cópias. O conteúdo pode ser
rolado e, abaixo de 600 pixels de altura, a frase de estado e o botão ficam no
rodapé, inclusive em 480 × 480. Uma solicitação de entrada recebe destaque
antes dos arquivos. Os detalhes da conexão ficam no painel da sala, dentro do
player.

A ação no canto superior esquerdo volta ao início em todas as etapas, inclusive
no player, e **Esc** faz o mesmo fora do player. Nas salas,
voltar é sair: a ação se chama **Encerrar sala** no anfitrião e **Sair da sala**
no convidado. Com a outra pessoa conectada, o app pergunta antes (**Encerrar a
sala?** ou **Sair da sala?**) e oferece **Continuar na sala**; sozinho na sala,
sai na hora. A tela inicial permite continuar um filme que ficou carregado após
sair.
Durante a reprodução, o vídeo ocupa a janela sem rolagem. No canto superior
direito, dois círculos, o nome e o estado mostram a presença: **Ana em
sincronia** com a conexão ativa, ou **reconectando** (no anfitrião,
**desconectado**) quando ela cai. Nesse caso, um aviso no centro do vídeo diz
**Reconectando com Ana…** ou **Esperando Ana voltar…**, e os controles
permanecem visíveis. Na pausa, o vídeo escurece e mostra **Pausado** com a
posição. A linha do tempo marca a posição da outra pessoa enquanto essa
informação estiver atualizada. A ação no canto superior esquerdo sai da sala,
com a mesma confirmação. Um clique na presença abre um painel com
participante, troca do filme, saída e detalhes da conexão; em janelas
estreitas, abre uma janela sobre o vídeo. **Trocar filme** pausa a sessão e
retorna à preparação. Sair da sala retorna ao início e deixa o arquivo
carregado e pausado. Estatísticas não ficam permanentemente no player.

## Rede local ou VPN

Em **Configurações** (ícone de engrenagem no cabeçalho), **Opções avançadas de
rede** mostra na tela inicial **Criar sala na rede local** e **Procurar na rede
local**, além da porta dessas salas e do servidor das salas pela internet. A
escolha vale até fechar o app; os argumentos `--host` e `--join` a ativam.

1. Conecte os PCs à mesma rede local ou VPN. O firewall precisa permitir a
   porta TCP da sala e UDP 7841 para descoberta.
2. O anfitrião clica em **Criar sala na rede local**. O app abre a porta TCP nas
   interfaces IPv4 do PC. A porta padrão é 7842 e pode ser alterada nas opções
   avançadas antes de criar.
3. O convidado escolhe **Procurar na rede local**. A lista acompanha anúncios UDP
   por multicast `239.255.78.42:7841` e broadcast de cada interface IPv4, inclusive
   interfaces de VPN. O anfitrião anuncia a cada 500 ms; a lista é atualizada
   aproximadamente a cada dois segundos. Portas TCP personalizadas também são
   anunciadas. Cada candidato é confirmado por TCP antes de aparecer. Salas
   ocupadas são removidas e anúncios expiram após cinco segundos. Até 128
   endereços são verificados, com 16 consultas simultâneas e prazo de 800 ms.
4. O convidado clica em **Solicitar entrada**. O anfitrião aceita ou recusa na
   etapa **Sala**. A solicitação expira após 60 segundos e pode ser cancelada pelo
   convidado. A tela de conexão distingue **Conectando à sala…** de
   **Aguardando aprovação**; só confirma **Anfitrião encontrado** depois que
   o anfitrião recebe a solicitação. **Cancelar solicitação** sai desse fluxo.
   Após a aprovação, o convidado passa à preparação dos arquivos.
   Há uma vaga de convidado, inclusive durante a aprovação.
5. A preparação segue como nas salas pela internet.


**Conectar por endereço** permite informar `IP:porta` quando a sala não aparece.
O campo valida o IP antes de enviar e aceita Enter para solicitar entrada.
Enquanto ninguém entra numa sala local, a etapa **Sala** do anfitrião mostra os
endereços em destaque, cada um com **Copiar endereço**, que confirma a cópia.
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
Na rede local, o TCP do aplicativo não tem criptografia própria; ela depende da
VPN utilizada. As salas pela internet são sempre cifradas.
Falhas de descoberta mantêm a conexão manual disponível. A demo continua offline.

## Controles

Os controles ficam sobre o vídeo e aparecem ao mover o mouse. Durante a
reprodução, somem após três segundos sem atividade. Permanecem visíveis na
pausa, durante operações pendentes, com um menu aberto, ao arrastar a barra
ou ao manter o mouse sobre os controles. A navegação por Tab também mantém
os controles visíveis. Volume, áudio e legendas afetam somente este PC;
play, pause e mudanças de posição continuam passando pelo coordenador da sala.
O menu de faixas mostra os títulos e idiomas fornecidos pelo arquivo.

O tema padrão é minimalista: grafite neutro, texto quase branco, ações
principais claras e um único tom de alerta, sem gradientes decorativos. O tema
claro inverte os mesmos neutros. A tipografia é Geist, com Geist Mono em códigos,
nomes de arquivo e tempos, incorporada ao executável. Os dois temas
ficam no menu **Configurações**, aberto pelo ícone de engrenagem no cabeçalho,
inclusive durante a conexão; o menu também reúne as opções avançadas de rede e os atalhos.
O cabeçalho permanece visível ao rolar o conteúdo. Os controles
do player usam ícones com dicas e nomes acessíveis para play/pause, saltos de
10 segundos, volume, áudio/legendas e tela cheia. Em janelas com 860 pixels ou
mais, o volume fica numa barra ao lado do ícone, que silencia com um clique; em
janelas menores, o ícone abre o menu de volume. O ícone de legendas abre o menu
de faixas. O título omite a extensão de vídeo. Avisos sobre
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

Com a barra de volume focada pelo teclado, ← e → ajustam o volume em 5%.

A demo usa os mesmos controles visuais, mas as ações de reprodução ficam
desativadas e nenhum player é iniciado. Tela cheia e ocultação dos controles
podem ser exploradas na demo.

Também é possível abrir um arquivo ou conectar por argumentos:

```sh
cargo run --locked -- --file /caminho/filme.mkv
cargo run --locked -- --internet --file /caminho/filme.mkv
cargo run --locked -- --code 4821-gato-lua --file /caminho/filme.mkv
cargo run --locked -- --host --file /caminho/filme.mkv
cargo run --locked -- --host --port 7843
cargo run --locked -- --join 192.168.1.10:7842 --file /caminho/filme.mkv
```

`--relay nome:porta`, ou a variável `SYNC2GETHER_RELAY`, troca o servidor das
salas pela internet. Para testes locais, `--host 127.0.0.1:7842` abre uma sala com aprovação nesse
endereço, sem consultar qualquer VPN. `--join` solicita aprovação, sem código.
Os dois apps precisam usar o protocolo 4.

A biblioteca `libmpv.so.2` é carregada em execução. Se ela faltar, o app exibe
um erro de instalação. Compilar e executar a demo não exige essa biblioteca.
