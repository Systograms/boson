FROM rust:1.94-bookworm AS builder
WORKDIR /workspace
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY apps ./apps
COPY crates ./crates
COPY adapters ./adapters
RUN cargo build --release -p boson-server -p boson-worker -p boson-cli

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /workspace/target/release/boson-server /usr/local/bin/
COPY --from=builder /workspace/target/release/boson-worker /usr/local/bin/
COPY --from=builder /workspace/target/release/boson-cli /usr/local/bin/boson
COPY config ./config
COPY migrations ./migrations
EXPOSE 8080
ENTRYPOINT ["boson-server"]
