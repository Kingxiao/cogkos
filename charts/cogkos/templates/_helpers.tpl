{{/* Helm模板辅助函数 */}}
{{/* cogkos.fullname - 生成完整名称 */}}
{{- define "cogkos.fullname" -}}
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

{{/* cogkos.name - 基础名称 */}}
{{- define "cogkos.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/* cogkos.chart - Chart标签 */}}
{{- define "cogkos.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/* cogkos.labels - 标准标签 */}}
{{- define "cogkos.labels" -}}
helm.sh/chart: {{ include "cogkos.chart" . }}
{{ include "cogkos.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/* cogkos.selectorLabels - 选择器标签 */}}
{{- define "cogkos.selectorLabels" -}}
app.kubernetes.io/name: {{ include "cogkos.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/* cogkos.serviceAccountName - 服务账户名 */}}
{{- define "cogkos.serviceAccountName" -}}
{{- if .Values.mcp.serviceAccount.create }}
{{- default (include "cogkos.fullname" .) .Values.mcp.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.mcp.serviceAccount.name }}
{{- end }}
{{- end }}

{{/* cogkos.databaseUrl - 数据库连接字符串 */}}
{{- define "cogkos.databaseUrl" -}}
{{- if .Values.externalDependencies.postgres.existingSecret }}
{{- printf "$(DATABASE_URL)" }}
{{- else }}
{{- printf "postgres://%s:%s@%s:%d/%s" 
    .Values.externalDependencies.postgres.username
    .Values.externalDependencies.postgres.password
    .Values.externalDependencies.postgres.host
    (.Values.externalDependencies.postgres.port | int)
    .Values.externalDependencies.postgres.database }}
{{- end }}
{{- end }}
