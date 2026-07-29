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
Labels. Bootstrap and test pods intentionally use different `name` labels so
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
{{- range $key := list "STARGATE_RUNTIME_PROFILE" "PORT" "CONFIG_PATH" "CONFIG_FILENAME" -}}
  {{- if hasKey $runtimeData $key -}}
    {{- fail (printf "runtimeConfig.data.%s is chart-managed and must be removed" $key) -}}
  {{- end -}}
{{- end -}}
{{- range $key := list "POSTGRES_PASSWORD" "REDIS_URL" "JWT_SECRET" "EMAIL_OTP_PEPPER" "PASSWORD_PEPPER" "SMTP_PASSWORD" -}}
  {{- if hasKey $runtimeData $key -}}
    {{- fail (printf "runtimeConfig.data.%s is sensitive; use runtimeSecret instead" $key) -}}
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
  {{- $algorithm := upper (default "HS256" (get $runtimeData "JWT_ALGORITHM")) -}}
  {{- if and (hasPrefix "HS" $algorithm) (empty (get $secretData "JWT_SECRET")) -}}
    {{- fail "cluster mode with an HMAC JWT algorithm requires runtimeSecret.stringData.JWT_SECRET" -}}
  {{- end -}}
{{- end -}}

{{- if and .Values.gatewayConfig.create (not (empty .Values.gatewayConfig.existingConfigMap)) -}}
  {{- fail "gatewayConfig.create and gatewayConfig.existingConfigMap are mutually exclusive" -}}
{{- end -}}
{{- if and (not .Values.gatewayConfig.create) (empty .Values.gatewayConfig.existingConfigMap) -}}
  {{- fail "set gatewayConfig.create=true or provide gatewayConfig.existingConfigMap" -}}
{{- end -}}
{{- if and .Values.gatewayConfig.create (empty .Values.gatewayConfig.config) -}}
  {{- fail "gatewayConfig.config must contain a stargate/v2alpha1 configuration" -}}
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
