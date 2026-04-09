ARG RUST_VERSION=1.86.0
ARG APP_NAME=stargate
ARG STARGATE_PROFILE=edge

FROM rust:${RUST_VERSION}-slim-bullseye AS chef
ARG APP_NAME
WORKDIR /app
RUN apt-get update && apt-get upgrade -y && \
    apt-get install -y ca-certificates clang libssl-dev openssl pkg-config && \
    apt-get clean && \
    cargo install cargo-chef --locked

FROM chef AS planner
ARG APP_NAME
ARG STARGATE_PROFILE
WORKDIR /app
COPY crates ./crates
COPY Cargo.toml ./Cargo.toml
COPY Cargo.lock ./Cargo.lock
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
ARG APP_NAME
ARG STARGATE_PROFILE
WORKDIR /app
COPY --from=planner /app/recipe.json ./recipe.json
RUN case "$STARGATE_PROFILE" in edge|cluster) ;; *) echo "Unsupported STARGATE_PROFILE: $STARGATE_PROFILE" >&2; exit 1;; esac && \
    cargo chef cook --release --recipe-path recipe.json -p $APP_NAME --no-default-features --features "$STARGATE_PROFILE"
COPY crates ./crates
COPY Cargo.toml ./Cargo.toml
COPY Cargo.lock ./Cargo.lock
RUN case "$STARGATE_PROFILE" in edge|cluster) ;; *) echo "Unsupported STARGATE_PROFILE: $STARGATE_PROFILE" >&2; exit 1;; esac && \
    cargo build --release -p $APP_NAME --no-default-features --features "$STARGATE_PROFILE" && \
    cp ./target/release/$APP_NAME /bin/server

FROM debian:bullseye-slim AS final
ARG STARGATE_PROFILE
# RUN apt-get update && apt-get upgrade -y
RUN apt-get update && apt-get upgrade -y && \
    apt-get install -y ca-certificates && \
    apt-get clean
COPY --from=build /bin/server /bin/
EXPOSE 5050

ENV RUST_LOG=info
ENV PORT=5050
ENV STARGATE_RUNTIME_PROFILE=${STARGATE_PROFILE}

CMD ["/bin/server"]
