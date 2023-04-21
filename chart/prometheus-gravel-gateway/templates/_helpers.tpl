{{/*
Expand the name of the chart.
*/}}
{{- define "prometheus-gravel-gateway.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
We truncate at 63 chars because some Kubernetes name fields are limited to this (by the DNS naming spec).
If release name contains chart name it will be used as a full name.
*/}}
{{- define "prometheus-gravel-gateway.fullname" -}}
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
{{- define "prometheus-gravel-gateway.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "prometheus-gravel-gateway.labels" -}}
helm.sh/chart: {{ include "prometheus-gravel-gateway.chart" . }}
{{ include "prometheus-gravel-gateway.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "prometheus-gravel-gateway.selectorLabels" -}}
app.kubernetes.io/name: {{ include "prometheus-gravel-gateway.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "prometheus-gravel-gateway.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "prometheus-gravel-gateway.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}


{{/*
Create the flags for the application
*/}}
{{- define "prometheus-gravel-gateway.args" -}}
    {{- $flags := list }}
    {{- if .Values.clustering.enabled }}
        {{- $flags = append $flags (printf "--cluster-enabled=true" ) }}
    {{- end }}
    {{- $flags = append $flags (printf "-%s=%s:%d" "l" "0.0.0.0" (.Values.service.port | int)) }}
    {{- if .Values.clustering.enabled }}
    {{- $flags = append $flags (include "prometheus-gravel-gateway.peers" .) }}
    {{- end }}
    {{- join "," ($flags) }}
{{- end }}

{{/*
Create the peers for the application based on the replica count
*/}}
{{- define "prometheus-gravel-gateway.peers" -}}
{{- $root := . }}
{{- $peers := list }}
{{- range $i, $e := until (.Values.replicaCount | int) }}
{{- $peers = append $peers (printf "--peer=%s-%d.%s-headless.%s.svc.cluster.local:%d" (include "prometheus-gravel-gateway.fullname" $) $i (include "prometheus-gravel-gateway.fullname" $) ($root.Release.Namespace) ($root.Values.service.port | int))}}
{{- end }}
{{- join "," ($peers) }}
{{- end }}
