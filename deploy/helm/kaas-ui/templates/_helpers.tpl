{{- define "kaas-ui.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/* The release name, unless it already says kaas-ui. */}}
{{- define "kaas-ui.fullname" -}}
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

{{/* Immutable on a live Deployment, so nothing that can change goes in here. */}}
{{- define "kaas-ui.selectorLabels" -}}
app.kubernetes.io/name: {{ include "kaas-ui.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{/*
For the pods. No `helm.sh/chart`: a chart change that alters nothing about the
pod must not restart it, because a restart signs everyone out.
*/}}
{{- define "kaas-ui.podLabels" -}}
{{ include "kaas-ui.selectorLabels" . }}
app.kubernetes.io/version: {{ .Values.image.tag | default .Chart.AppVersion | quote }}
{{- with .Values.commonLabels }}
{{ toYaml . }}
{{- end }}
{{- end -}}

{{- define "kaas-ui.labels" -}}
{{ include "kaas-ui.podLabels" . }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end -}}

{{- define "kaas-ui.serviceAccountName" -}}
{{- default (include "kaas-ui.fullname" .) .Values.serviceAccount.name -}}
{{- end -}}

{{- define "kaas-ui.configMapName" -}}
{{- .Values.existingConfigMap | default (printf "%s-config" (include "kaas-ui.fullname" .)) -}}
{{- end -}}

{{- define "kaas-ui.image" -}}
{{- if .Values.image.digest -}}
{{- printf "%s@%s" .Values.image.repository .Values.image.digest -}}
{{- else -}}
{{- printf "%s:%s" .Values.image.repository (.Values.image.tag | default .Chart.AppVersion) -}}
{{- end -}}
{{- end -}}

{{/*
The port half of `server.listen`. The file decides what the process binds, so it
also decides what the container declares, and the two cannot disagree. Taking
the last segment is what makes `[::]:8080` work.
*/}}
{{- define "kaas-ui.port" -}}
{{- $listen := dig "server" "listen" "0.0.0.0:8080" (.Values.config | default dict) -}}
{{- splitList ":" (toString $listen) | last -}}
{{- end -}}
