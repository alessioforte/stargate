# Deploy Stargate on Kubernetes with Terraform

This guide installs the Stargate Helm chart in `cluster` mode and connects it
to PostgreSQL and Redis that the Terraform architecture already manages.

## Architecture

```text
                         HTTPS
                           |
                    Ingress / Gateway
                           |
                 stargate Service :5050
                           |
                +----------+----------+
                |                     |
           Stargate pod          Stargate pod
                |                     |
                +----------+----------+
                           |
              +------------+-------------+
              |                          |
       existing PostgreSQL          existing Redis
       users, clients, audit        sessions, OAuth state,
       and outbox records           limits, revocation
```

Use an image compiled with `STARGATE_PROFILE=cluster`. A runtime value cannot
turn an edge image into a cluster image.

## Required architecture outputs

Pass these values from the modules that already create PostgreSQL and Redis:

| Input | Expected format | Secret |
| --- | --- | --- |
| PostgreSQL endpoint | `host:port`, without a URI scheme | No |
| PostgreSQL database | Database name | No |
| PostgreSQL username | Login role | Usually |
| PostgreSQL password | Login password | Yes |
| Redis URL | Complete `redis://` or `rediss://` URI | Yes when authenticated |

The PostgreSQL database should already exist. Its role needs `CONNECT` and
enough schema privileges for Stargate's migrations. Network policies, security
groups, and firewalls must allow the pods to reach PostgreSQL and Redis.

Stargate currently constructs its PostgreSQL URI from these values without URI
encoding. Until that changes, keep the username and password URL-safe; `@`,
`/`, `:`, `?`, and `#` can make the generated URI ambiguous.

At the root module, the wiring typically looks like:

```hcl
module "stargate" {
  source = "./modules/stargate"

  postgres_endpoint = "${module.postgres.host}:${module.postgres.port}"
  postgres_database = module.postgres.database_name
  postgres_username = module.postgres.username
  postgres_password = module.postgres.password

  redis_url = module.redis.connection_uri
}
```

## Providers and variables

The example uses the current HashiCorp Helm and Kubernetes provider line.
Configure both providers with the existing cluster credentials in the root
module.

```hcl
terraform {
  required_version = ">= 1.5.0"

  required_providers {
    helm = {
      source  = "hashicorp/helm"
      version = "~> 3.2"
    }
    kubernetes = {
      source  = "hashicorp/kubernetes"
      version = "~> 3.2"
    }
  }
}

variable "namespace" {
  type    = string
  default = "identity"
}

variable "chart_path" {
  description = "Path to the checked-out k8s/stargate chart."
  type        = string
  default     = "./k8s/stargate"
}

variable "image_repository" {
  type = string
}

variable "image_tag" {
  description = "Immutable cluster-profile image tag."
  type        = string
}

variable "public_url" {
  description = "External HTTPS origin without a trailing slash."
  type        = string
}

variable "gateway_config_file" {
  description = "Path to a valid stargate/v1 gateway configuration."
  type        = string
}

variable "postgres_endpoint" {
  description = "PostgreSQL host and port without a URI scheme."
  type        = string
}

variable "postgres_database" {
  type = string
}

variable "postgres_username" {
  type      = string
  sensitive = true
}

variable "postgres_password" {
  type      = string
  sensitive = true
}

variable "redis_url" {
  description = "Complete redis:// or rediss:// URI."
  type        = string
  sensitive   = true
}

variable "jwt_secret" {
  description = "Stable high-entropy HMAC signing key shared by every replica."
  type        = string
  sensitive   = true
}

variable "email_otp_pepper" {
  description = "Stable high-entropy HMAC pepper for OTP and MFA challenges."
  type        = string
  sensitive   = true
}

variable "bootstrap_email" {
  type = string
}

variable "bootstrap_password" {
  type      = string
  sensitive = true
}

variable "console_oauth_client_id" {
  type    = string
  default = "stargate_console"
}
```

## Namespace and Secrets

Create the Secrets outside the Helm release. The runtime pods cannot read the
separate bootstrap password because only the hook Job mounts it.

```hcl
resource "kubernetes_namespace_v1" "stargate" {
  metadata {
    name = var.namespace
  }
}

resource "kubernetes_secret_v1" "runtime" {
  metadata {
    name      = "stargate-runtime"
    namespace = kubernetes_namespace_v1.stargate.metadata[0].name
  }

  data = {
    POSTGRES_USERNAME = var.postgres_username
    POSTGRES_PASSWORD = var.postgres_password
    REDIS_URL         = var.redis_url
    JWT_SECRET        = var.jwt_secret
    EMAIL_OTP_PEPPER  = var.email_otp_pepper
  }

  type = "Opaque"
}

resource "kubernetes_secret_v1" "bootstrap" {
  metadata {
    name      = "stargate-bootstrap"
    namespace = kubernetes_namespace_v1.stargate.metadata[0].name
  }

  data = {
    password = var.bootstrap_password
  }

  type = "Opaque"
}
```

`sensitive = true` redacts normal Terraform output but does not remove values
from state. Protect and encrypt the backend, or replace these resources with
the architecture's external-secret mechanism and pass only Secret names to
Helm. Kubernetes Secrets also require encryption at rest and least-privilege
RBAC.

## Install the chart

The chart receives only Secret names; confidential values are not placed in
Helm values or ConfigMaps.

```hcl
locals {
  public_url         = trimsuffix(var.public_url, "/")
  oauth_redirect_uri = "${local.public_url}/stargate/auth/callback"
}

resource "helm_release" "stargate" {
  name      = "stargate"
  namespace = kubernetes_namespace_v1.stargate.metadata[0].name
  chart     = var.chart_path

  atomic          = true
  cleanup_on_fail = true
  wait            = true
  wait_for_jobs   = true
  timeout         = 600

  values = [
    yamlencode({
      deployment = {
        mode = "cluster"
      }

      replicaCount = 2

      image = {
        repository = var.image_repository
        tag        = var.image_tag
      }

      persistence = {
        enabled = false
      }

      runtimeConfig = {
        data = {
          LOG_FILE_ENABLED      = "false"
          JWT_ALGORITHM         = "HS256"
          JWT_ISSUER            = local.public_url
          OAUTH_BASE_URL        = local.public_url
          CONSOLE_PUBLIC_URL    = local.public_url
          CONSOLE_APP_BASE_PATH = "/stargate"
          AUTH_APP_BASE_PATH    = "/auth"
          TRUSTED_ORIGINS       = local.public_url
          CORS_ORIGINS          = local.public_url

          POSTGRES_ENDPOINT = var.postgres_endpoint
          POSTGRES_DATABASE = var.postgres_database
          REDIS_POOL_SIZE   = "10"
        }
      }

      runtimeSecret = {
        existingSecret = kubernetes_secret_v1.runtime.metadata[0].name
        rolloutToken   = kubernetes_secret_v1.runtime.metadata[0].resource_version
      }

      gatewayConfig = {
        config   = file(var.gateway_config_file)
        policies = ""
      }

      podDisruptionBudget = {
        enabled      = true
        minAvailable = 1
      }

      topologySpreadConstraints = [{
        maxSkew           = 1
        topologyKey       = "topology.kubernetes.io/zone"
        whenUnsatisfiable = "ScheduleAnyway"
        labelSelector = {
          matchLabels = {
            "app.kubernetes.io/name" = "stargate"
          }
        }
      }]

      bootstrap = {
        enabled          = true
        email            = var.bootstrap_email
        oauthClientId    = var.console_oauth_client_id
        oauthRedirectUri = local.oauth_redirect_uri
        existingSecret   = kubernetes_secret_v1.bootstrap.metadata[0].name
        rolloutToken     = kubernetes_secret_v1.bootstrap.metadata[0].resource_version
      }
    })
  ]

  depends_on = [
    kubernetes_secret_v1.runtime,
    kubernetes_secret_v1.bootstrap
  ]
}
```

If the namespace is managed elsewhere, replace the namespace resource with an
input. If the chart is published to an OCI or Helm repository, replace
`chart_path` with the repository/chart/version fields required by that source.

The post-install/post-upgrade hook runs the idempotent bootstrap command. It
creates the first administrator and `stargate_console` OAuth client, or verifies
the existing resources. The Console is compiled into the image, so
`console_oauth_client_id` and the callback must match its
`VITE_OAUTH_CLIENT_ID` and `VITE_OAUTH_REDIRECT_URI` build arguments.

## Ingress and NetworkPolicy

Connect the chart's `ClusterIP` Service to the architecture's existing Ingress
or Gateway API resources. If the chart owns the Ingress, add this to the
`yamlencode` value:

```hcl
ingress = {
  enabled   = true
  className = "nginx"
  hosts = [{
    host = "identity.example.com"
    paths = [{
      path     = "/"
      pathType = "Prefix"
    }]
  }]
  tls = [{
    secretName = "stargate-tls"
    hosts      = ["identity.example.com"]
  }]
}
```

`networkPolicy.enabled=true` applies the supplied rules exactly. Allow:

- inbound traffic from the ingress controller or gateway;
- DNS;
- PostgreSQL and Redis;
- SMTP when email OTP/MFA is enabled;
- configured upstream APIs and telemetry collectors.

Avoid generic IP rules when namespace and pod selectors are available.

## Apply and verify

```bash
terraform fmt -check -recursive
terraform validate
terraform plan
terraform apply

kubectl -n identity rollout status deployment/stargate
kubectl -n identity logs job/stargate-bootstrap
kubectl -n identity port-forward service/stargate 5050:5050
curl --fail http://127.0.0.1:5050/readyz
helm test stargate --namespace identity
```

Verify that:

1. every pod reports `STARGATE_RUNTIME_PROFILE=cluster`;
2. PostgreSQL migrations completed;
3. Redis connections succeed;
4. the bootstrap hook completed;
5. the Console returns to `/stargate/auth/callback`;
6. terminating one pod does not invalidate sessions or OAuth state.

The readiness probe `/readyz` checks PostgreSQL and Redis in cluster mode.
Startup and liveness use `/livez`, which has no external dependency checks.
`/health` is a readiness compatibility endpoint. See
[Health and readiness probes](health-probes.md) for semantics and shutdown timing.

## Chart guarantees

Chart `0.2.0` provides:

- release-scoped ConfigMaps, Secrets, PVCs, and workload names;
- ConfigMap rejection for known sensitive keys;
- existing Secret/ConfigMap/PVC integration;
- fixed container port and named Service target port;
- profile-aware persistence and update strategy;
- gateway configuration and policy mounting;
- checksum-triggered rollouts for chart-owned configuration;
- non-root security contexts and disabled service-account token mounting;
- startup, liveness, and readiness probes;
- PDB, HPA, NetworkPolicy, topology, affinity, sidecar, and init-container
  extension points;
- schema validation and idempotent bootstrap hooks.

See the [chart README](../k8s/stargate/README.md) for all operational and
0.1.x migration notes.

## References

- [Terraform Helm provider](https://registry.terraform.io/providers/hashicorp/helm/latest/docs)
- [Terraform Kubernetes provider](https://registry.terraform.io/providers/hashicorp/kubernetes/latest/docs)
- [Terraform sensitive-data guidance](https://developer.hashicorp.com/terraform/language/manage-sensitive-data)
- [Kubernetes Secrets guidance](https://kubernetes.io/docs/concepts/configuration/secret/)
- [Kubernetes encryption at rest](https://kubernetes.io/docs/tasks/administer-cluster/encrypt-data/)
