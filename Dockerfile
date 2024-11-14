ARG RUST_VERSION=1.80.1
ARG APP_NAME=stargate

FROM rust:${RUST_VERSION}-slim-bullseye AS build
ARG APP_NAME
WORKDIR /app
RUN apt-get update && apt-get upgrade -y && \
apt-get install -y ca-certificates clang libssl-dev openssl pkg-config && \
apt-get clean
COPY src ./src
COPY Cargo.toml ./Cargo.toml
COPY Cargo.lock ./Cargo.lock
RUN cargo build --release && cp ./target/release/${APP_NAME} /bin/server

################################################################################
FROM debian:bullseye-slim AS final
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
