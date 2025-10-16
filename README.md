# Stargate

![Actix](https://img.shields.io/badge/actix-web-blue)
![Rust](https://img.shields.io/badge/rust-1.78+-orange)

## Table of Contents

- [Introduction](#introduction)
- [Features](#features)
- [Installation](#installation)
- [Environment Variables](#environment-variables)
- [Usage](#usage)

## Introduction

This project is a api gateway and user management system built using the [Actix Web](https://actix.rs/) framework for Rust.
Actix Web is a powerful, pragmatic, and extremely fast web framework for Rust.

## Features

- Fast and efficient HTTP server
- User authentication and authorization
- JWT token generation and validation
- Role-based access control (RBAC)
- Attribute-based access control (ABAC)
- Policy-based access control (PBAC)
- Password hashing and validation
- Email verification and password reset
- Database integration with PostgreSQL and SQLite

## Installation

To set up this project locally, you need to have [Rust](https://www.rust-lang.org/) and [Cargo](https://doc.rust-lang.org/cargo/) installed.

1. Run the server:

   ```sh
   cargo run -p stargate
   ```

2. Build the project:

   ```sh
   cargo build --release -p stargate
   ```

## Environment Variables

The following environment variables can be set to configure the application in the .env file:

- `PORT`: The port on which the server will run. Default is `5050`.
- `RUST_LOG`: The log level for the application..
- `API_BASE_PATH`: The base path for the API.
- `JWT_ALGORITHM`: The algorithm for the JWT token. Default is `HS256`.
- `JWT_SECRET`: The secret for the JWT token.
- `JWT_ISSUER`: The issuer for the JWT token. Default is `stargate`.

- `JWT_PUBLIC_KEY`: The public key for the JWT token in a PEM format.
- `JWT_PRIVATE_KEY`: The private key for the JWT token in a PEM format.
- `JWT_PRIVATE_KEY_PATH`: The path to the private key for the JWT token.
- `JWT_PUBLIC_KEY_PATH`: The path to the public key for the JWT token.
- `JWT_ACCESS_EXP`: The expiration time for the access token. Default is `1h`.
- `JWT_REFRESH_EXP`: The expiration time for the refresh token. Default is `1d`.

- `POSTGRES_ENDPOINT`: The endpoint for the PostgreSQL database.
- `POSTGRES_USERNAME`: The username for the PostgreSQL database.
- `POSTGRES_PASSWORD`: The password for the PostgreSQL database.
- `POSTGRES_DATABASE`: The database name for the PostgreSQL database.

## Usage

After running the server, it will be available at `http://localhost:5050`. You can test the endpoints using tools like [Postman](https://www.postman.com/) or `curl`.

Example:

```sh
curl http://localhost:5050/health
```

## Docker Build

To build the Docker image, run the following command:

```sh
docker buildx build -t stargate:VERSION .
```

Replace `VERSION` with the version of the image.

## Docker Run

To run the Docker container, run the following command:

```sh
docker run -d -p 5050:5050 --name stargate stargate:VERSION
```

Replace `VERSION` with the version of the image.


## Roadmap

- [x] Send Email
- [x] Cookie-based authentication
- [x] Password Policies
- [x] Load Balancer - Implement Round Robin, Least Connections, IP Hash, URL Hash
- [x] Sqlite - Implement SQLite as a database option
- [x] PostgreSQL - Implement PostgreSQL as a database option
- [x] Redis or Memory - Implement Redis as a cache option
- [x] API Keys - Generate, Revoke, List
- [ ] Rate Limiting Global and Per User
- [ ] Realms - Implement multiple realms
- [ ] Send SMS
- [ ] Audit Logs
- [ ] MFA (Multi-Factor Authentication)

ACCESS CONTROL
- [x] Implement Role-Based Access Control (RBAC)
- [x] Implement Attribute-Based Access Control (ABAC)
- [x] Implement Policy-Based Access Control (PBAC)

OAUTH2 PROVIDERS
- [x] oAuth Github
- [x] oAuth Google
- [ ] oAuth Facebook
- [ ] oAuth Instagram
- [ ] oAuth Apple
- [ ] oAuth Linkedin
- [ ] oAuth Microsoft

PROTOCOLS
- [x] HTTP/HTTPS
- [x] WebSockets
- [ ] TCP/UDP
- [ ] MQTT - try with rumqtt crate
- [ ] gRPC - try with tonic crate
- [ ] FTP/FTPS
- [ ] AMQP
- [ ] SSE (Server-Sent Events)
- [ ] Transforming protocols (e.g., HTTP to gRPC)

ADMIN FEATURES
- [x] Init basic configuration
- [x] Save configuration to a file
- [x] Download configuration in json or yaml format
- [x] Load configuration from a file at runtime

TESTING AND PERFORMANCE
- [ ] Unit Testing
- [ ] Integration Testing
- [ ] Performance Testing
- [ ] Load Testing and API Gateway Performance

DOCUMENTATION
- [ ] Documentation pages
- [ ] Implement Swagger
- [ ] Implement OpenAPI

MONITORING
- [ ] Implement Metrics
- [ ] Implement Tracing
- [ ] Implement Alerting

- [ ] ? Time-based One-Time Password (TOTP) - Authenticator App
- [ ] ? Login - verify access from another device and notify user - Handle sessions
- [ ] ? Single Sign-On (SSO) and Single Log-Out (SLO)
- [ ] ? act as oAuth provider
- [ ] ? QR Code - Generate and Scan
- [ ] ? Generate PDF as Infisical does
- [ ] ? Bio-metric Authentication
