# Empacotamento e Release

Builds para Linux (Arch Linux e Ubuntu 24.04), Windows (x86_64) e macOS (Apple Silicon e Intel), localmente ou pelo GitHub Actions. Execute os comandos na raiz do projeto.

[Voltar ao README](../README.md)

## Gerar os pacotes pelo GitHub Actions

O workflow **Pacotes e Release**, acionado manualmente no GitHub Actions, compila
e empacota todas as plataformas em paralelo:

- **Linux:** containers Arch Linux (`*.pkg.tar.zst`) e Ubuntu 24.04 (`*.deb`),
  incluindo testes com libmpv, OpenGL e instalação real do pacote.
- **Windows (x86_64 MSVC):** `.zip` portátil e instalador NSIS (`*-setup.exe`)
  com `libmpv-2.dll` incluída.
- **macOS (arm64 e x86_64):** `.dmg` com `sync2gether.app`, `libmpv.2.dylib` e
  dependências reescritas em `Contents/Frameworks/` com assinatura ad-hoc.

Commits comuns não disparam builds de release.

Para lançar uma versão, atualize `version` no `Cargo.toml` e `pkgver` no
`packaging/arch/PKGBUILD`, envie o commit e abra **Actions → Pacotes e Release →
Run workflow**. A opção **rascunho** (padrão) cria a release `vX.Y.Z` como
rascunho, com todos os pacotes, `SHA256SUMS`, instruções de instalação e as
mudanças desde a release anterior. Revise o texto e publique pela página de
releases. **publicada** publica direto, e **nenhuma** só deixa os pacotes como
artifacts da execução. O workflow para antes de compilar se a versão já tiver
release ou tag.

## Builds locais

### Linux (Docker)

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
atualize também `pkgver` no PKGBUILD e os exemplos de instalação no README.

### Windows (PowerShell)

Após `cargo build --release --locked`:

```powershell
pwsh -File scripts/package-windows.ps1 -LibMpvPath caminho\para\libmpv-2.dll
```

Gera `target/dist/sync2gether-<versão>-windows-x86_64.zip` e
`target/dist/sync2gether-<versão>-windows-x86_64-setup.exe` (requer NSIS).

### macOS

Com `mpv` instalado via Homebrew (`brew install mpv`), após
`cargo build --release --locked`:

```sh
bash scripts/package-macos.sh arm64
```

(Ou `x86_64` em Mac Intel.) Gera `target/dist/sync2gether-<versão>-macos-<arch>.dmg`.
