# Guia para agentes

## Produto

O sync2gether é um aplicativo de desktop para duas pessoas assistirem ao mesmo
filme, com uma cópia local em cada PC. Arch Linux e Ubuntu são as plataformas
iniciais. Os PCs se comunicam pelo Tailscale; um deles coordena a sala.

A stack escolhida é Rust, egui/eframe, Tokio, Serde e libmpv. O projeto está na
fase de estrutura inicial: a interface demo funciona, mas ainda não há player
nem conexão de rede. Não apresente estados simulados como funcionalidades reais.

## Organização

- `src/main.rs`: argumentos e inicialização da janela.
- `src/app.rs`: estado da aplicação e aplicação das ações após desenhar a tela.
- `src/model.rs`: estados da sessão e ações da interface.
- `src/ui.rs`: desenho das telas. Retorna ações, sem realizar I/O.
- `src/demo.rs`: cenários fictícios e captura de screenshots, apenas com a
  feature `demo`.
- `tests/layout.rs`: teste da interface sem janela ou conexão.

Quando implementados, a rede e o player devem ficar em módulos separados da
interface. A comunicação entre a UI e o trabalho em segundo plano deve usar
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
