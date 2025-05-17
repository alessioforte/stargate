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

## Installation

To set up this project locally, you need to have [Rust](https://www.rust-lang.org/) and [Cargo](https://doc.rust-lang.org/cargo/) installed.

1. Build the project:

   ```sh
   cargo build
   ```

2. Run the server:

   ```sh
   cargo run
   ```

## Environment Variables

The following environment variables can be set to configure the application in the .env file:

- `PORT`: The port on which the server will run. Default is `5050`.
- `RUST_LOG`: The log level for the application..
- `SURREALDB_ENDPOINT`: The endpoint for the SurrealDB. By default is a RocksDB database which persists data on the filesystem that is located at `.flows/flows.db`.
- `SURREALDB_USERNAME`: The username for the SurrealDB.
- `SURREALDB_PASSWORD`: The password for the SurrealDB.
- `SURREALDB_NAMESPACE`: The namespace for the SurrealDB. Default is `sensoworks`.
- `SURREALDB_DATABASE`: The database for the SurrealDB. Default is `flows`.
- `API_BASE_PATH`: The base path for the API.
- `COOKIE_BASED_SESSION`: Enable cookie-based session. Default is `false`.
- `JWT_ALGORITHM`: The algorithm for the JWT token. Default is `HS256`.
- `JWT_SECRET`: The secret for the JWT token.
- `JWT_PUBLIC_KEY`: The public key for the JWT token in a PEM format.
- `JWT_PRIVATE_KEY`: The private key for the JWT token in a PEM format.
- `JWT_PRIVATE_KEY_PATH`: The path to the private key for the JWT token.
- `JWT_PUBLIC_KEY_PATH`: The path to the public key for the JWT token.
- `JWT_ACCESS_EXPIRATION_MINUTES`: The expiration time for the access token. Default is `60`.
- `JWT_REFRESH_EXPIRATION_DAYS`: The expiration time for the refresh token. Default is `1`.

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
- [ ] Send SMS
- [ ] Time-based One-Time Password (TOTP) - Authenticator App
- [ ] MFA (Multi-Factor Authentication)
- [x] Cookie-based authentication
- [x] Password Policies
- [ ] Audit Logs
- [ ] Consider to decouple the authentication from the gateway
- [ ] ? Load Balancer - Implement Round Robin, Least Connections, IP Hash, URL Hash
- [ ] Server info and Provider info
- [ ] Realms - Implement multiple realms

- [ ] ? Login - verify access from another device and notify user - Handle sessions
- [ ] ? Single Sign-On (SSO) and Single Log-Out (SLO)
- [ ] ? act as oAuth provider
- [ ] ? QR Code - Generate and Scan
- [ ] ? Generate PDF as Infisical does
- [ ] ? Bio-metric Authentication

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
- [ ] MQTT - try with rumqtt crate
- [ ] gRPC - try with tonic crate
- [ ] ? FTP/FTPS
- [ ] ? TCP/UDP
- [ ] ? AMQP
- [ ] ? SSE
- [ ] ? SOAP

ACCESS CONTROL
- [ ] Implement Role-Based Access Control (RBAC)
- [ ] Implement Attribute-Based Access Control (ABAC)
- [ ] Implement Policy-Based Access Control (PBAC)
- [ ] Implement Rule-Based Access Control (RBAC)

- [ ] API Keys - Generate, Revoke, List
- [ ] Rate Limiting Global and Per User

PRICING PLANS
- [ ] Implement Pricing Plans Configuration

ADMIN FEATURES
- [x] √ Init basic configuration
- [x] √ Save configuration to a file
- [x] √ Download configuration in json or yaml format
- [x] √ Load configuration from a file at runtime

- [ ] Create tenant - Use SurrealDB namespace
- [ ] Create tenant admin
- [ ] Assign permissions to tenant admin
- [ ] Implement Admin Panel
- [ ] Super Admin create access control for tenant

TESTING AND PERFORMANCE
- [x] √ Implement Trie Data Structure for fast path search
- [ ] Unit Testing
- [ ] Integration Testing
- [ ] Performance Testing
- [ ] Load Testing and API Gateway Performance

DOCUMENTATION
- [ ] Documentation Portal
- [ ] Implement Swagger
- [ ] Implement OpenAPI

MONITORING
- [ ] Implement Health Check
- [ ] Implement Metrics
- [ ] Implement Tracing
- [ ] Implement Logging
- [ ] Implement Alerting
