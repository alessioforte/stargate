{{/*
Expand the name of the chart.
*/}}
{{- define "stargate.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
We truncate at 63 chars because some Kubernetes name fields are limited to this (by the DNS naming spec).
If release name contains chart name it will be used as a full name.
*/}}
{{- define "stargate.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "stargate.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "stargate.labels" -}}
helm.sh/chart: {{ include "stargate.chart" . }}
{{ include "stargate.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "stargate.selectorLabels" -}}
app.kubernetes.io/name: {{ include "stargate.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "stargate.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "stargate.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Validate deployment profile combinations.
*/}}
{{- define "stargate.validateProfile" -}}
{{- $mode := default "edge" .Values.deployment.mode -}}
{{- if not (has $mode (list "edge" "cluster")) -}}
{{- fail (printf "unsupported deployment.mode %q: expected \"edge\" or \"cluster\"" $mode) -}}
{{- end -}}

{{- if eq $mode "edge" -}}
{{- if .Values.autoscaling.enabled -}}
{{- fail "deployment.mode=edge requires autoscaling.enabled=false because local sqlite + memory state is single-node only" -}}
{{- end -}}
{{- if ne (int .Values.replicaCount) 1 -}}
{{- fail "deployment.mode=edge requires replicaCount=1 because local sqlite + memory state is single-node only" -}}
{{- end -}}
{{- end -}}

{{- if eq $mode "cluster" -}}
{{- $configData := default (dict) .Values.configmap.data -}}
{{- if not (hasKey $configData "REDIS_URL") -}}
{{- fail "deployment.mode=cluster requires configmap.data.REDIS_URL for shared state" -}}
{{- end -}}
{{- if not (hasKey $configData "POSTGRES_ENDPOINT") -}}
{{- fail "deployment.mode=cluster requires configmap.data.POSTGRES_ENDPOINT for shared database connectivity" -}}
{{- end -}}
{{- end -}}
{{- end }}
