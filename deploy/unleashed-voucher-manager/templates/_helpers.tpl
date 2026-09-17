{{- define "unleashed-voucher-manager.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "unleashed-voucher-manager.fullname" -}}
{{- printf "%s-%s" .Release.Name (include "unleashed-voucher-manager.name" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "unleashed-voucher-manager.labels" -}}
app.kubernetes.io/name: {{ include "unleashed-voucher-manager.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ .Chart.Name }}-{{ .Chart.Version }}
{{- end -}}
