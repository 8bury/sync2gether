# Guia para agentes

## Produto

O sync2gether é um aplicativo de desktop para duas pessoas assistirem ao mesmo
filme, com uma cópia local em cada PC. Arch Linux e Ubuntu são as plataformas
iniciais. Os PCs se comunicam por TCP na rede local ou VPN; um deles coordena a sala.
A descoberta usa anúncios UDP por multicast e broadcast nas interfaces IPv4,
sem depender de um provedor de VPN. Entrada manual por IP:porta é alternativa.

A stack escolhida é Rust, egui/eframe, Tokio, Serde e libmpv. O MVP tem player libmpv, salas TCP e sincronização coordenada pelo anfitrião.
O modo demo continua fictício e offline. Não apresente estados simulados como
funcionalidades reais.

## Organização

- `src/main.rs`: argumentos e inicialização da janela.
- `src/app.rs`: estado da aplicação e aplicação das ações após desenhar a tela.
- `src/model.rs`: estados da sessão e ações da interface.
- `src/ui.rs`: desenho das telas. Retorna ações, sem realizar I/O.
- `src/demo.rs`: cenários fictícios e captura de screenshots, apenas com a
  feature `demo`.
- `src/runtime.rs`: comandos/eventos entre UI e trabalho em segundo plano.
- `src/player.rs`: FFI libmpv e thread de controle.
- `src/player/gl.rs`: renderização no contexto OpenGL da janela.
- `src/player_ui.rs`: controles sobre o vídeo, sem I/O.
- `src/network.rs` e `src/protocol.rs`: transporte e mensagens versionadas.
- `src/network/rooms.rs`: descoberta, aprovação de entrada e tokens de reconexão.
- `src/discovery.rs`: anúncios UDP na rede e confirmação TCP de salas em segundo plano.
- `src/session.rs` e `src/sync.rs`: preparação confirmada, início agendado,
  relógio monotônico filtrado e correção de reprodução.
- `src/media.rs`: identidade dos arquivos locais.
- `tests/layout.rs`: teste da interface sem janela ou conexão.
- `tests/network.rs`: comunicação TCP real em localhost.
- `tests/mvp.rs`: dois players reais com fixture sintética, executado por
  `bash scripts/test-mvp.sh`, incluindo proxy com atraso e jitter.

A rede e o player ficam em módulos separados da interface. A comunicação entre a UI e o trabalho em segundo plano deve usar
comandos e eventos. Não bloqueie a UI com acesso a disco, rede ou decodificação.
Use libmpv para reprodução e Tokio para rede; não implemente codecs próprios.

## Desenvolvimento

Leia o README para dependências e comandos. Use o Rust fixado em
`rust-toolchain.toml` e mantenha `Cargo.lock` versionado. Não copie versões,
forks ou patches do zapfast sem uma necessidade verificada neste projeto.

Para revisar a interface, use os cenários do modo demo. Nunca use filmes ou
arquivos pessoais como fixtures. Capturas vão em `.cache/`, fora do Git.
O teste de layout não verifica áudio, vídeo, GPU ou sincronização real.

Antes de concluir mudanças de código, execute `bash scripts/check.sh`. Acrescente
testes para comportamentos que podem regredir, especialmente sincronização,
reconexão e comandos simultâneos quando esses recursos existirem. Não enfraqueça
lints ou testes apenas para passar as verificações.

Atualize a documentação quando mudar o comportamento, a arquitetura ou os
comandos. Relate quais plataformas e cenários foram de fato testados. Compilar
em Arch não demonstra que o aplicativo foi executado em Ubuntu.

Mantenha as mudanças dentro da tarefa solicitada. Não publique releases nem
envie commits para um remoto sem autorização. Este guia não define uma política
de branches; siga as instruções do usuário.

## Builds e dados

Use o `target/` deste projeto. Se houver builds paralelos no mesmo checkout,
cada um deve ter seu próprio `CARGO_TARGET_DIR` sob `target/`. Não coloque builds
ou arquivos grandes em `/tmp`.

O modo demo deve funcionar sem Tailscale, abrir sockets ou ler arquivos pessoais.
Logs de diagnóstico não devem incluir caminhos completos dos filmes, tokens
ou outros dados pessoais.

## Referências

Este guia e o fluxo de demo foram inspirados no
[zapfast](https://github.com/crmne/zapfast), com instruções escritas para o
sync2gether. Os detalhes específicos de WhatsApp, publicação e atualização do
zapfast não se aplicam aqui.
