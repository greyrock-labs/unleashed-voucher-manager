{{- define "unleashed-voucher-manager.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- /*
  Standard Helm scaffold fullname, including the guard that an earlier
  version of this chart dropped.

  The guard matters because the common way to deploy this -- a Flux
  HelmRelease -- takes the release name from the HelmRelease's
  metadata.name, which is naturally "unleashed-voucher-manager". Without
  the `contains` check that renders every object as
  "unleashed-voucher-manager-unleashed-voucher-manager", and every
  consumer (an HTTPRoute backendRef, say) has to hardcode the doubled
  name.
*/ -}}
{{- define "unleashed-voucher-manager.fullname" -}}
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

{{- define "unleashed-voucher-manager.labels" -}}
app.kubernetes.io/name: {{ include "unleashed-voucher-manager.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ .Chart.Name }}-{{ .Chart.Version }}
{{- end -}}
