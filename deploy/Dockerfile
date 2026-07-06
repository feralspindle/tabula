# Kernel image. Plugins are NOT built here — they're wasm components mounted
# at runtime (see docker-compose.yml) or baked by CI into /app/plugins/dist.
FROM rust:1-slim AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml ./
COPY crates ./crates
COPY kernel ./kernel
COPY vendor ./vendor
COPY wit ./wit
RUN cargo build --release -p tabula-kernel

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /src/target/release/tabula-kernel /usr/local/bin/tabula-kernel
ENV PLUGIN_DIR=/app/plugins/dist
EXPOSE 8080
CMD ["tabula-kernel"]
