ARG RUST_VERSION=1.80.0
ARG APP_NAME=stargate

FROM --platform=linux/arm64 rust:${RUST_VERSION}-slim-bullseye AS build
ARG APP_NAME
WORKDIR /app
RUN apt-get update && apt-get upgrade -y
RUN apt-get install -y pkg-config openssl libssl-dev ca-certificates clang
RUN \
    apt-get install ca-certificates && \
    apt-get clean
# Build the application.
# Leverage a cache mount to /usr/local/cargo/registry/
# for downloaded dependencies and a cache mount to /app/target/ for
# compiled dependencies which will speed up subsequent builds.
# Leverage a bind mount to the src directory to avoid having to copy the
# source code into the container. Once built, copy the executable to an
# output directory before the cache mounted /app/target is unmounted.
RUN --mount=type=bind,source=src,target=src \
    --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
    # --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=cache,target=/app/target/ \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
    <<EOF
set -e
rustup target add aarch64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
cp ./target/aarch64-unknown-linux-gnu/release/$APP_NAME /bin/server
EOF

################################################################################
# Create a new stage for running the application that contains the minimal
# runtime dependencies for the application. This often uses a different base
# image from the build stage where the necessary files are copied from the build
# stage.
#
# The example below uses the debian bullseye image as the foundation for running the app.
# By specifying the "bullseye-slim" tag, it will also use whatever happens to be the
# most recent version of that tag when you build your Dockerfile. If
# reproducibility is important, consider using a digest
# (e.g., debian@sha256:ac707220fbd7b67fc19b112cee8170b41a9e97f703f588b2cdbbcdcecdd8af57).
FROM --platform=linux/arm64 debian:bullseye-slim AS final

RUN apt-get update && apt-get upgrade -y

# Create a non-privileged user that the app will run under.
# See https://docs.docker.com/develop/develop-images/dockerfile_best-practices/#user
# ARG UID=10001
# RUN adduser \
#     --disabled-password \
#     --gecos "" \
#     --home "/nonexistent" \
#     --shell "/sbin/nologin" \
#     --no-create-home \
#     --uid "${UID}" \
#     appuser
# USER appuser

COPY --from=build /bin/server /bin/
EXPOSE 5050

ENV RUST_LOG=info
ENV PORT=5050

CMD ["/bin/server"]
