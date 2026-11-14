# Pacotes Linux

Builds para Arch Linux e Ubuntu 24.04, localmente ou pelo GitHub Actions. Execute os comandos na raiz do projeto.

[Voltar ao README](../README.md)

## Gerar os pacotes

O workflow **Pacotes Linux**, acionado manualmente no GitHub Actions, verifica
o código, executa testes com libmpv e vídeo sintético, compila e instala cada
pacote em sua distribuição. Commits comuns não disparam builds de release.

Para lançar uma versão, atualize `version` no `Cargo.toml` e `pkgver` no
`packaging/arch/PKGBUILD`, envie o commit e abra **Actions → Pacotes Linux →
Run workflow**. A opção **rascunho** (padrão) cria a release `vX.Y.Z` como
rascunho, com os dois pacotes, `SHA256SUMS`, instruções de instalação e as
mudanças desde a release anterior. Revise o texto e publique pela página de
releases. **publicada** publica direto, e **nenhuma** só deixa os pacotes como
artifacts da execução. O workflow para antes de compilar se a versão já tiver
release ou tag.

Para repetir localmente com Docker, use checkouts separados para que os
testes não compartilhem fixtures. Todos os builds ficam sob `target/`:

```sh
mkdir -p target/package-work/ubuntu target/package-work/arch
git archive HEAD | tar -x -C target/package-work/ubuntu
git archive HEAD | tar -x -C target/package-work/arch
docker run --rm -v "$PWD/target/package-work/ubuntu:/work" -w /work \
  ubuntu:24.04 bash scripts/release-container.sh deb
docker run --rm -v "$PWD/target/package-work/arch:/work" -w /work \
  archlinux:base-devel bash scripts/release-container.sh arch
```

Os resultados ficam em `target/package-work/{ubuntu,arch}/target/dist/`.
O Docker grava arquivos como root. Para editar ou remover os diretórios depois,
ajuste a propriedade com `sudo chown -R "$(id -u):$(id -g)" target/package-work`.
`packaging/arch/PKGBUILD` empacota o binário produzido no Arch; ele não baixa
nem compila código-fonte sozinho. Ao atualizar a versão no `Cargo.toml`,
atualize também `pkgver` no PKGBUILD e os exemplos de instalação acima.
