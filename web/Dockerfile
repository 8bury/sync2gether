# Página do convidado no navegador. Contexto de build: a raiz do repositório.
#   docker build -f web/Dockerfile -t sync2gether-web .
# Serve os arquivos estáticos e repassa /relay para RELAY_UPSTREAM (host:porta do
# WebSocket do relay, por exemplo sync2gether-relay:8080).
FROM rust:1.98.0-slim AS build
ENV RUSTUP_TOOLCHAIN=1.98.0 CARGO_TERM_COLOR=never
RUN rustup target add wasm32-unknown-unknown \
    && cargo install --locked wasm-bindgen-cli --version 0.2.129 --root /tools
WORKDIR /src
COPY . .
RUN mkdir -p target/tools/bin && ln -s /tools/bin/wasm-bindgen target/tools/bin/wasm-bindgen \
    && bash scripts/build-web.sh /out

FROM caddy:2.10.0-alpine
COPY web/Caddyfile /etc/caddy/Caddyfile
COPY --from=build /out /srv
USER 65534:65534
EXPOSE 8080/tcp
ENV RELAY_UPSTREAM=sync2gether-relay:8080 XDG_DATA_HOME=/tmp XDG_CONFIG_HOME=/tmp
