# Relay do sync2gether. Contexto de build: a raiz do repositório.
#   docker build -f relay/Dockerfile -t sync2gether-relay .
# O binário é estático (musl) e roda sem shell, como usuário sem privilégios.
FROM rust:1.98.0-alpine AS build
RUN apk add --no-cache musl-dev
# Usa o compilador da imagem, sem instalar os componentes do rust-toolchain.toml.
ENV RUSTUP_TOOLCHAIN=1.98.0 CARGO_TERM_COLOR=never
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p sync2gether-relay \
    && install -Dm755 target/release/sync2gether-relay /out/sync2gether-relay

FROM scratch
COPY --from=build /out/sync2gether-relay /sync2gether-relay
USER 65534:65534
EXPOSE 7843/tcp
ENV RELAY_LISTEN=0.0.0.0:7843
ENTRYPOINT ["/sync2gether-relay"]
