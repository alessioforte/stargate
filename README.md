# Stargate User Management System and API Gateway

## Deployment Profiles

Stargate supports different deployment profiles with different operational limits:

- `edge`: single-node deployments where local `sqlite + memory` state is acceptable. This fits Raspberry Pi, appliance, and other edge installs. Use `replicaCount=1` and keep autoscaling disabled.
- `cluster`: multi-replica deployments that require shared backends such as Redis and Postgres. Do not use local-only state assumptions in this mode.

The Helm chart defaults to `deployment.mode=edge` and validates incompatible combinations at template time.

## Build Profiles

The container image builds with an explicit backend profile:

- `edge`: compiles `sqlite + memory`
- `cluster`: compiles `postgres + redis`

```bash
docker build --build-arg STARGATE_PROFILE=edge -t stargate:edge .
docker build --build-arg STARGATE_PROFILE=cluster -t stargate:cluster .
```

At runtime, Stargate validates `STARGATE_RUNTIME_PROFILE` against the compiled backend set and fails fast if they do not match.
