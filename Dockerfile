# syntax=docker/dockerfile:1.7

ARG RUST_VERSION=1.96.0
ARG NODE_VERSION=22.18.0
ARG CARGO_CHEF_VERSION=0.1.77
ARG APP_NAME=stargate
ARG STARGATE_PROFILE=edge

FROM node:${NODE_VERSION}-bookworm-slim AS console-build
WORKDIR /app/apps/console
COPY apps/console/package.json apps/console/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci
COPY apps/console ./
ARG CONSOLE_APP_BASE_PATH=/stargate
ARG VITE_API_URL
ARG VITE_AUTH_URL
ARG VITE_OAUTH_CLIENT_ID=stargate_console
ARG VITE_OAUTH_REDIRECT_URI
RUN VITE_BASE_PATH="${CONSOLE_APP_BASE_PATH}" \
    VITE_API_URL="${VITE_API_URL}" \
    VITE_AUTH_URL="${VITE_AUTH_URL}" \
    VITE_OAUTH_CLIENT_ID="${VITE_OAUTH_CLIENT_ID}" \
    VITE_OAUTH_REDIRECT_URI="${VITE_OAUTH_REDIRECT_URI}" \
    npm run build

FROM node:${NODE_VERSION}-bookworm-slim AS auth-build
WORKDIR /app/apps/auth
COPY apps/auth/package.json apps/auth/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci
COPY apps/auth ./
ARG AUTH_APP_BASE_PATH=/auth
ARG VITE_API_URL
RUN VITE_BASE_PATH="${AUTH_APP_BASE_PATH}" \
    VITE_API_URL="${VITE_API_URL}" \
    npm run build

FROM node:${NODE_VERSION}-bookworm-slim AS mail-build
WORKDIR /app/apps/mail
COPY apps/mail/package.json apps/mail/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci
COPY apps/mail ./
RUN npm run export

FROM rust:${RUST_VERSION}-slim-bookworm AS chef
ARG CARGO_CHEF_VERSION
WORKDIR /app
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        clang \
        curl \
        libssl-dev \
        pkg-config && \
    rm -rf /var/lib/apt/lists/* && \
    cargo install cargo-chef \
        --locked \
        --version "${CARGO_CHEF_VERSION}"

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS server-build
ARG APP_NAME
ARG STARGATE_PROFILE
COPY --from=planner /app/recipe.json ./recipe.json
RUN case "${STARGATE_PROFILE}" in \
        edge|cluster) ;; \
        *) echo "Unsupported STARGATE_PROFILE: ${STARGATE_PROFILE}" >&2; exit 1 ;; \
    esac && \
    cargo chef cook --release --recipe-path recipe.json \
        -p "${APP_NAME}" \
        --no-default-features \
        --features "${STARGATE_PROFILE}"
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations
COPY src ./src
RUN cargo build --release \
        -p "${APP_NAME}" \
        --no-default-features \
        --features "${STARGATE_PROFILE}" && \
    cp "./target/release/${APP_NAME}" /bin/server

FROM debian:bookworm-slim AS final
ARG STARGATE_PROFILE
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 10001 stargate && \
    useradd --uid 10001 --gid 10001 --no-create-home \
        --home-dir /app --shell /usr/sbin/nologin stargate

WORKDIR /app
RUN mkdir -p .stargate && chown stargate:stargate .stargate
COPY --from=server-build /bin/server /bin/server
COPY --from=console-build --chown=stargate:stargate \
    /app/.stargate/apps/console ./.stargate/apps/console
COPY --from=auth-build --chown=stargate:stargate \
    /app/.stargate/apps/auth ./.stargate/apps/auth
COPY --from=mail-build --chown=stargate:stargate \
    /app/.stargate/transactional ./.stargate/transactional

USER 10001:10001
EXPOSE 5050

ENV RUST_LOG=info
ENV PORT=5050
ENV STARGATE_RUNTIME_PROFILE=${STARGATE_PROFILE}
ARG CONSOLE_APP_BASE_PATH=/stargate
ARG AUTH_APP_BASE_PATH=/auth
ARG VITE_OAUTH_CLIENT_ID=stargate_console
ARG VITE_OAUTH_REDIRECT_URI
ENV CONSOLE_APP_BASE_PATH=${CONSOLE_APP_BASE_PATH}
ENV AUTH_APP_BASE_PATH=${AUTH_APP_BASE_PATH}
ENV CONSOLE_OAUTH_CLIENT_ID=${VITE_OAUTH_CLIENT_ID}
ENV CONSOLE_OAUTH_REDIRECT_URI=${VITE_OAUTH_REDIRECT_URI}

CMD ["/bin/server"]
