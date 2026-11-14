# Protótipo das telas

Protótipo React do fluxo do sync2gether, usado para decidir a interface antes
de implementá-la em egui. As cenas são ilustrações animadas: o cursor, os
arquivos e a outra pessoa são fictícios, e nada abre sockets, lê arquivos ou
reproduz vídeo. A implementação real fica em `src/ui/`.

```sh
cd prototype
npm install
npm run dev
```

Abra o endereço indicado pelo Vite. Com `?capture=host` (ou `guest`, `prep`,
`mismatch`, `player`) a página mostra uma cena só.

| Cena | Pasta | O que mostra |
|---|---|---|
| Criar uma sala | `examples/s2g-host` | Início, código em destaque, a outra pessoa entrando |
| Entrar com código | `examples/s2g-guest` | Código no início, erro no campo, progresso no botão |
| Escolher o filme | `examples/s2g-prep` | Cópias lado a lado, comparação, botão principal |
| Arquivos diferentes | `examples/s2g-mismatch` | O dado que difere e o que fazer |
| Assistir | `examples/s2g-player` | Selo da sala, avisos de pausa e reconexão |

`examples/_s2g` tem a paleta do app (`src/ui.rs`) mapeada para os tokens do
kit e as peças comuns. A fonte Inter vem de `assets/fonts`.

`kit/` é uma cópia do kit do [Stageshow](https://github.com/benjitaylor/skills)
(licença MIT, ver `kit/LICENSE` e `kit/NOTICE.md`). As marcas em `kit/brands`
vêm com o kit e não são usadas pelas cenas.
