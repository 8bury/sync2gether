# Guia de uso

Salas, escolha dos arquivos, controles e conexão por rede local ou VPN.

[Voltar ao README](../README.md)

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
