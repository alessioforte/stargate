ARG RUST_VERSION=1.86.0
ARG APP_NAME=stargate

FROM rust:${RUST_VERSION}-slim-bullseye AS chef
ARG APP_NAME
WORKDIR /app
RUN apt-get update && apt-get upgrade -y && \
    apt-get install -y ca-certificates clang libssl-dev openssl pkg-config && \
    apt-get clean && \
    cargo install cargo-chef --locked

RUN mkdir -p .stargate/certificate/localhost
RUN openssl req -x509 -nodes -days 365 -newkey rsa:2048 -keyout .stargate/certificate/localhost/key.pem -out .stargate/certificate/localhost/cert.pem -subj "/C=FR/ST=IDF/L=Paris/O=Global Security/OU=IT Department/CN=localhost"

FROM chef AS planner
ARG APP_NAME
WORKDIR /app
COPY crates ./crates
COPY Cargo.toml ./Cargo.toml
COPY Cargo.lock ./Cargo.lock
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
ARG APP_NAME
WORKDIR /app
COPY --from=planner /app/recipe.json ./recipe.json
RUN cargo chef cook --release --recipe-path recipe.json -p $APP_NAME
COPY crates ./crates
COPY Cargo.toml ./Cargo.toml
COPY Cargo.lock ./Cargo.lock
RUN cargo build --release -p $APP_NAME && cp ./target/release/$APP_NAME /bin/server

FROM debian:bullseye-slim AS final
# RUN apt-get update && apt-get upgrade -y
RUN apt-get update && apt-get upgrade -y && \
    apt-get install -y ca-certificates && \
    apt-get clean
COPY --from=build /bin/server /bin/
COPY --from=chef /app/.stargate /.stargate
EXPOSE 5050

ENV RUST_LOG=info
ENV PORT=5050

CMD ["/bin/server"]
