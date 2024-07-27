FROM rust:1.79.0 AS builder
# FROM rust:1.79.0-alpine AS builder
# FROM rust:1-slim-bookworm AS builder
WORKDIR /app
RUN apt-get update && apt-get -y install ca-certificates cmake musl-tools libssl-dev && rm -rf /var/lib/apt/lists/*
COPY . .
RUN rustup default nightly && rustup update
RUN rustup target add x86_64-unknown-linux-musl
ENV PKG_CONFIG_ALLOW_CROSS=1


# COPY Cargo.toml Cargo.toml
# RUN cargo fetch
# RUN mkdir src && echo "fn main() {println!(\"Hello, world!\");}" > src/main.rs
# RUN mkdir src && echo "fn main() {}" > src/main.rs
# RUN echo "fn main() {}" > src/main.rs
# RUN cargo build --release
# RUN rm src/main.rs
RUN cargo fetch
# RUN cargo build --release
RUN cargo build --target x86_64-unknown-linux-musl --release

FROM debian:buster-slim

RUN apt-get update && apt-get install -y ca-certificates libssl1.1 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/stargate /app/stargate

EXPOSE 5050
ENTRYPOINT ["/app/stargate"]
