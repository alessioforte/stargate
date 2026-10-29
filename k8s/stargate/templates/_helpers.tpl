{{/*
Chart and resource names.
*/}}
{{- define "stargate.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "stargate.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "stargate.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "stargate.image" -}}
{{- if .Values.image.digest -}}
{{- printf "%s@%s" .Values.image.repository .Values.image.digest -}}
{{- else -}}
{{- printf "%s:%s" .Values.image.repository (.Values.image.tag | default .Chart.AppVersion) -}}
{{- end -}}
{{- end -}}

{{/*
Labels. Hook and test pods intentionally use different `name` labels so
the Service selector can never route traffic to them.
*/}}
{{- define "stargate.commonLabels" -}}
{{- $labels := deepCopy (default (dict) .Values.commonLabels) -}}
{{- $_ := set $labels "helm.sh/chart" (include "stargate.chart" .) -}}
{{- $_ := set $labels "app.kubernetes.io/managed-by" .Release.Service -}}
{{- if .Chart.AppVersion -}}
{{- $_ := set $labels "app.kubernetes.io/version" .Chart.AppVersion -}}
{{- end -}}
{{ toYaml $labels }}
{{- end -}}

{{- define "stargate.selectorLabels" -}}
{{- $labels := dict
  "app.kubernetes.io/name" (include "stargate.name" .)
  "app.kubernetes.io/instance" .Release.Name
-}}
{{ toYaml $labels }}
{{- end -}}

{{- define "stargate.labels" -}}
{{- $labels := mergeOverwrite
  (include "stargate.commonLabels" . | fromYaml)
  (include "stargate.selectorLabels" . | fromYaml)
  (dict "app.kubernetes.io/component" "server")
-}}
{{ toYaml $labels }}
{{- end -}}

{{- define "stargate.bootstrapLabels" -}}
{{- $labels := mergeOverwrite
  (include "stargate.commonLabels" . | fromYaml)
  (dict
    "app.kubernetes.io/name" (printf "%s-bootstrap" (include "stargate.name" .) | trunc 63 | trimSuffix "-")
    "app.kubernetes.io/instance" .Release.Name
    "app.kubernetes.io/component" "bootstrap"
  )
-}}
{{ toYaml $labels }}
{{- end -}}

{{- define "stargate.configImportLabels" -}}
{{- $labels := mergeOverwrite
  (include "stargate.commonLabels" . | fromYaml)
  (dict
    "app.kubernetes.io/name" (printf "%s-config-import" (include "stargate.name" . | trunc 49 | trimSuffix "-"))
    "app.kubernetes.io/instance" .Release.Name
    "app.kubernetes.io/component" "config-import"
  )
-}}
{{ toYaml $labels }}
{{- end -}}

{{- define "stargate.testLabels" -}}
{{- $labels := mergeOverwrite
  (include "stargate.commonLabels" . | fromYaml)
  (dict
    "app.kubernetes.io/name" (printf "%s-test" (include "stargate.name" .) | trunc 63 | trimSuffix "-")
    "app.kubernetes.io/instance" .Release.Name
    "app.kubernetes.io/component" "test"
  )
-}}
{{ toYaml $labels }}
{{- end -}}

{{/*
Referenced resource names.
*/}}
{{- define "stargate.configImportName" -}}
{{- printf "%s-config-import" (include "stargate.fullname" . | trunc 49 | trimSuffix "-") -}}
{{- end -}}

{{/* Keep protected and ambiguous paths out of public identity routes. */}}
{{- define "stargate.adminPathRule" -}}
(PathPrefix(`/admin`) || PathPrefix(`/stargate`) || PathRegexp(`(?i)(//|%|(^|/)[.]{1,2}(/|$)|^/(admin|stargate))`))
{{- end -}}

{{- define "stargate.adminAllowListName" -}}
{{- printf "%s-admin-allowlist" (include "stargate.fullname" . | trunc 47 | trimSuffix "-") -}}
{{- end -}}

{{- define "stargate.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "stargate.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{- define "stargate.runtimeConfigName" -}}
{{- if .Values.runtimeConfig.existingConfigMap -}}
{{- .Values.runtimeConfig.existingConfigMap -}}
{{- else -}}
{{- printf "%s-runtime" (include "stargate.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "stargate.runtimeSecretName" -}}
{{- if .Values.runtimeSecret.existingSecret -}}
{{- .Values.runtimeSecret.existingSecret -}}
{{- else -}}
{{- printf "%s-runtime" (include "stargate.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "stargate.gatewayConfigName" -}}
{{- if .Values.gatewayConfig.existingConfigMap -}}
{{- .Values.gatewayConfig.existingConfigMap -}}
{{- else -}}
{{- printf "%s-gateway" (include "stargate.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "stargate.persistenceClaimName" -}}
{{- if .Values.persistence.existingClaim -}}
{{- .Values.persistence.existingClaim -}}
{{- else -}}
{{- printf "%s-data" (include "stargate.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{/*
JWT_ALGORITHM from the chart's runtime ConfigMap, upper-cased; empty when unset
(the server then uses the active key's algorithm).
*/}}
{{- define "stargate.jwtAlgorithm" -}}
{{- upper (toString (default "" (get (default (dict) .Values.runtimeConfig.data) "JWT_ALGORITHM"))) -}}
{{- end -}}

{{/*
Mount point of the Secret holding JWT key `<kid>`; the server reads its keyring
from .stargate/jwks/keys/ under the image's /app working directory.
*/}}
{{- define "stargate.jwtKeyPath" -}}
{{- printf "/app/.stargate/jwks/keys/%s" . -}}
{{- end -}}

{{/*
Cross-field validation that JSON Schema cannot express.
*/}}
{{- define "stargate.validateValues" -}}
{{- $mode := .Values.deployment.mode -}}
{{- if eq $mode "edge" -}}
  {{- if .Values.autoscaling.enabled -}}
    {{- fail "deployment.mode=edge requires autoscaling.enabled=false" -}}
  {{- end -}}
  {{- if ne (int .Values.replicaCount) 1 -}}
    {{- fail "deployment.mode=edge requires replicaCount=1" -}}
  {{- end -}}
  {{- if not .Values.persistence.enabled -}}
    {{- fail "deployment.mode=edge requires persistence.enabled=true for SQLite, local state, and signing keys" -}}
  {{- end -}}
{{- else if eq $mode "cluster" -}}
  {{- if .Values.persistence.enabled -}}
    {{- fail "deployment.mode=cluster requires persistence.enabled=false; shared state belongs in PostgreSQL and Redis" -}}
  {{- end -}}
  {{- if and (not .Values.runtimeSecret.create) (empty .Values.runtimeSecret.existingSecret) -}}
    {{- fail "deployment.mode=cluster requires runtimeSecret.existingSecret or runtimeSecret.create=true" -}}
  {{- end -}}
{{- else -}}
  {{- fail (printf "unsupported deployment.mode %q: expected \"edge\" or \"cluster\"" $mode) -}}
{{- end -}}

{{- if and .Values.runtimeConfig.create (not (empty .Values.runtimeConfig.existingConfigMap)) -}}
  {{- fail "runtimeConfig.create and runtimeConfig.existingConfigMap are mutually exclusive" -}}
{{- end -}}
{{- if and (not .Values.runtimeConfig.create) (empty .Values.runtimeConfig.existingConfigMap) -}}
  {{- fail "set runtimeConfig.create=true or provide runtimeConfig.existingConfigMap" -}}
{{- end -}}

{{- if and .Values.runtimeSecret.create (not (empty .Values.runtimeSecret.existingSecret)) -}}
  {{- fail "runtimeSecret.create and runtimeSecret.existingSecret are mutually exclusive" -}}
{{- end -}}

{{- $runtimeData := default (dict) .Values.runtimeConfig.data -}}
{{- $managed := dict -}}
{{- if .Values.traefik.adminAllowList.enabled -}}
  {{- if not .Values.traefik.enabled -}}
    {{- fail "traefik.adminAllowList.enabled=true requires traefik.enabled=true" -}}
  {{- end -}}
  {{- if empty .Values.traefik.adminAllowList.sourceRange -}}
    {{- fail "traefik.adminAllowList.enabled=true requires a nonempty sourceRange" -}}
  {{- end -}}
{{- end -}}
{{- if .Values.traefik.enabled -}}
  {{- if .Values.ingress.enabled -}}
    {{- fail "traefik.enabled and ingress.enabled are mutually exclusive" -}}
  {{- end -}}
  {{- $identity := required "traefik.enabled=true requires traefik.identityHost" .Values.traefik.identityHost -}}
  {{- $hosts := list (lower $identity) -}}
  {{- range $host := .Values.traefik.apiHosts -}}
    {{- if has (lower $host) $hosts -}}
      {{- fail "traefik.identityHost and traefik.apiHosts must contain distinct hostnames (case-insensitive)" -}}
    {{- end -}}
    {{- $hosts = append $hosts (lower $host) -}}
  {{- end -}}
  {{- if and .Values.traefik.tls.secretName .Values.traefik.tls.certResolver -}}
    {{- fail "set only one of traefik.tls.secretName and traefik.tls.certResolver" -}}
  {{- end -}}
  {{- $_ := set $managed "IAM_HOSTS" "traefik.identityHost" -}}
{{- end -}}
{{- if .Values.trustedProxies -}}
  {{- $_ := set $managed "TRUSTED_PROXIES" "trustedProxies" -}}
{{- end -}}
{{- if .Values.internalContext.enabled -}}
  {{- range $key := list "existingSecret" "mountPath" "issuer" "kid" -}}
    {{- if empty (get $.Values.internalContext $key) -}}
      {{- fail (printf "internalContext.enabled=true requires internalContext.%s" $key) -}}
    {{- end -}}
  {{- end -}}
  {{- $_ := set $managed "INTERNAL_CONTEXT_PRIVATE_KEY_PATH" "internalContext.mountPath" -}}
  {{- $_ := set $managed "INTERNAL_CONTEXT_JWKS_PATH" "internalContext.mountPath" -}}
  {{- $_ := set $managed "INTERNAL_CONTEXT_ISSUER" "internalContext.issuer" -}}
  {{- $_ := set $managed "INTERNAL_CONTEXT_KID" "internalContext.kid" -}}
{{- end -}}
{{- range $key, $setting := $managed -}}
  {{- if hasKey $runtimeData $key -}}
    {{- fail (printf "runtimeConfig.data.%s is chart-managed; set %s instead" $key $setting) -}}
  {{- end -}}
  {{- range $env := $.Values.extraEnv -}}
    {{- if eq (get $env "name") $key -}}
      {{- fail (printf "extraEnv.%s is chart-managed; set %s instead" $key $setting) -}}
    {{- end -}}
  {{- end -}}
{{- end -}}
{{- range $key := list "STARGATE_RUNTIME_PROFILE" "PORT" "CONFIG_PATH" "CONFIG_FILENAME" -}}
  {{- if hasKey $runtimeData $key -}}
    {{- fail (printf "runtimeConfig.data.%s is chart-managed and must be removed" $key) -}}
  {{- end -}}
{{- end -}}
{{- range $key, $value := dict "JWT_KID" "jwt.activeKid" "JWT_ID_TOKEN_KID" "jwt.idTokenKid" -}}
  {{- if hasKey $runtimeData $key -}}
    {{- fail (printf "runtimeConfig.data.%s is chart-managed; set %s instead" $key $value) -}}
  {{- end -}}
{{- end -}}
{{- range $key := list "POSTGRES_PASSWORD" "REDIS_URL" "JWT_SECRET" "EMAIL_OTP_PEPPER" "PASSWORD_PEPPER" "SMTP_PASSWORD" "GOOGLE_OAUTH_CLIENT_SECRET" "GITHUB_OAUTH_CLIENT_SECRET" -}}
  {{- if hasKey $runtimeData $key -}}
    {{- fail (printf "runtimeConfig.data.%s is sensitive; use runtimeSecret instead" $key) -}}
  {{- end -}}
{{- end -}}

{{- $jwt := .Values.jwt -}}
{{- $jwtAlgorithm := include "stargate.jwtAlgorithm" . -}}
{{- $hmac := hasPrefix "HS" $jwtAlgorithm -}}
{{- $kids := list -}}
{{- range $key := $jwt.keys -}}
  {{- if or (eq $key.kid ".") (eq $key.kid "..") -}}
    {{- fail (printf "jwt.keys: %q is not a valid kid" $key.kid) -}}
  {{- end -}}
  {{- if has $key.kid $kids -}}
    {{- fail (printf "jwt.keys: kid %q is listed twice" $key.kid) -}}
  {{- end -}}
  {{- $kids = append $kids $key.kid -}}
{{- end -}}
{{- if eq $mode "edge" -}}
  {{- if $kids -}}
    {{- fail "jwt.keys is cluster-only: edge keeps its JWT keys on the PVC (rotate them with `stargate jwt generate-key` in the pod)" -}}
  {{- end -}}
{{- else if eq $mode "cluster" -}}
  {{- if $hmac -}}
    {{- if $jwt.activeKid -}}
      {{- fail (printf "jwt.activeKid names a key pair, but JWT_ALGORITHM=%s signs with JWT_SECRET; use jwt.idTokenKid for the RS256 ID token key" $jwtAlgorithm) -}}
    {{- end -}}
    {{- if and $kids (not $jwt.idTokenKid) -}}
      {{- fail "jwt.keys is unused with an HMAC JWT_ALGORITHM unless jwt.idTokenKid names one of them" -}}
    {{- end -}}
  {{- else if and (or .Values.runtimeConfig.create $kids) (not $jwt.activeKid) -}}
    {{- fail "cluster replicas must share their JWT signing keys: set jwt.activeKid and jwt.keys (one Secret per key), or JWT_ALGORITHM=HS256 with JWT_SECRET" -}}
  {{- end -}}
  {{- range $name, $kid := dict "jwt.activeKid" $jwt.activeKid "jwt.idTokenKid" $jwt.idTokenKid -}}
    {{- if and $kid (not (has $kid $kids)) -}}
      {{- fail (printf "%s %q is not in jwt.keys" $name $kid) -}}
    {{- end -}}
  {{- end -}}
{{- end -}}

{{- if and (eq $mode "cluster") .Values.runtimeSecret.create -}}
  {{- $secretData := default (dict) .Values.runtimeSecret.stringData -}}
  {{- range $key := list "POSTGRES_ENDPOINT" "POSTGRES_DATABASE" "POSTGRES_USERNAME" "POSTGRES_PASSWORD" "REDIS_URL" -}}
    {{- $configValue := get $runtimeData $key -}}
    {{- $secretValue := get $secretData $key -}}
    {{- if and (empty $configValue) (empty $secretValue) -}}
      {{- fail (printf "cluster mode with runtimeSecret.create=true requires %s in runtimeConfig.data or runtimeSecret.stringData" $key) -}}
    {{- end -}}
  {{- end -}}
  {{- if and $hmac (empty (get $secretData "JWT_SECRET")) -}}
    {{- fail "cluster mode with an HMAC JWT algorithm requires runtimeSecret.stringData.JWT_SECRET" -}}
  {{- end -}}
{{- end -}}

{{- if and (eq $mode "edge") .Values.gatewayConfig.importOnUpgrade -}}
  {{- fail "gatewayConfig.importOnUpgrade is cluster-only: use kubectl exec in the existing edge pod to run /bin/server config import; a second pod must not mount the SQLite ReadWriteOnce PVC" -}}
{{- end -}}

{{- if and .Values.gatewayConfig.create (not (empty .Values.gatewayConfig.existingConfigMap)) -}}
  {{- fail "gatewayConfig.create and gatewayConfig.existingConfigMap are mutually exclusive" -}}
{{- end -}}
{{- if and (not .Values.gatewayConfig.create) (empty .Values.gatewayConfig.existingConfigMap) -}}
  {{- fail "set gatewayConfig.create=true or provide gatewayConfig.existingConfigMap" -}}
{{- end -}}
{{- if and .Values.gatewayConfig.create (empty .Values.gatewayConfig.config) -}}
  {{- fail "gatewayConfig.config must contain a stargate/v1 configuration" -}}
{{- end -}}
{{- if .Values.gatewayConfig.create -}}
  {{- $gatewayConfig := tpl .Values.gatewayConfig.config . | fromYaml -}}
  {{- if ne (toString (get $gatewayConfig "schema")) "stargate/v1" -}}
    {{- fail "gatewayConfig.config requires an explicit schema of stargate/v1" -}}
  {{- end -}}
{{- end -}}

{{- if and (eq $mode "edge") .Values.persistence.enabled (ne .Values.persistence.mountPath "/app/.stargate") -}}
  {{- fail "edge persistence.mountPath must be /app/.stargate because SQLite and local state use that path" -}}
{{- end -}}

{{- if .Values.bootstrap.enabled -}}
  {{- if empty .Values.bootstrap.email -}}
    {{- fail "bootstrap.enabled=true requires bootstrap.email" -}}
  {{- end -}}
  {{- if empty .Values.bootstrap.oauthRedirectUri -}}
    {{- fail "bootstrap.enabled=true requires bootstrap.oauthRedirectUri" -}}
  {{- end -}}
  {{- if empty .Values.bootstrap.existingSecret -}}
    {{- fail "bootstrap.enabled=true requires bootstrap.existingSecret" -}}
  {{- end -}}
{{- end -}}

{{- if .Values.podDisruptionBudget.enabled -}}
  {{- if and .Values.podDisruptionBudget.minAvailable .Values.podDisruptionBudget.maxUnavailable -}}
    {{- fail "podDisruptionBudget.minAvailable and maxUnavailable are mutually exclusive" -}}
  {{- end -}}
  {{- if and (not .Values.podDisruptionBudget.minAvailable) (not .Values.podDisruptionBudget.maxUnavailable) -}}
    {{- fail "podDisruptionBudget.enabled=true requires minAvailable or maxUnavailable" -}}
  {{- end -}}
{{- end -}}
{{- if and .Values.autoscaling.enabled (not .Values.autoscaling.targetCPUUtilizationPercentage) (not .Values.autoscaling.targetMemoryUtilizationPercentage) -}}
  {{- fail "autoscaling.enabled=true requires a CPU or memory utilization target" -}}
{{- end -}}
{{- if and .Values.autoscaling.enabled (gt (int .Values.autoscaling.minReplicas) (int .Values.autoscaling.maxReplicas)) -}}
  {{- fail "autoscaling.minReplicas must not exceed autoscaling.maxReplicas" -}}
{{- end -}}
{{- end -}}
