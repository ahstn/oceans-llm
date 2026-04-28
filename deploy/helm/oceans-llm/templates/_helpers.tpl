{{/*
Expand the chart name.
*/}}
{{- define "oceans-llm.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "oceans-llm.fullname" -}}
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

{{- define "oceans-llm.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "oceans-llm.labels" -}}
helm.sh/chart: {{ include "oceans-llm.chart" . }}
app.kubernetes.io/name: {{ include "oceans-llm.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{- define "oceans-llm.gatewaySelectorLabels" -}}
app.kubernetes.io/name: {{ include "oceans-llm.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: gateway
{{- end -}}

{{- define "oceans-llm.adminUiSelectorLabels" -}}
app.kubernetes.io/name: {{ include "oceans-llm.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: admin-ui
{{- end -}}

{{- define "oceans-llm.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "oceans-llm.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{- define "oceans-llm.gatewayServiceName" -}}
{{- printf "%s-gateway" (include "oceans-llm.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "oceans-llm.adminUiServiceName" -}}
{{- printf "%s-admin-ui" (include "oceans-llm.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "oceans-llm.configMapName" -}}
{{- printf "%s-config" (include "oceans-llm.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "oceans-llm.inlineSecretName" -}}
{{- printf "%s-env" (include "oceans-llm.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "oceans-llm.externalSecretTargetName" -}}
{{- default (printf "%s-external-env" (include "oceans-llm.fullname" .)) .Values.externalSecrets.targetName | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "oceans-llm.cnpgClusterName" -}}
{{- default (printf "%s-postgres" (include "oceans-llm.fullname" .)) .Values.database.cloudnativepg.clusterName | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "oceans-llm.cnpgCredentialsSecretName" -}}
{{- default (printf "%s-app" (include "oceans-llm.cnpgClusterName" .)) .Values.database.cloudnativepg.existingCredentialsSecret | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "oceans-llm.gatewayImage" -}}
{{- printf "%s:%s" .Values.gateway.image.repository (default .Chart.AppVersion .Values.gateway.image.tag) -}}
{{- end -}}

{{- define "oceans-llm.adminUiImage" -}}
{{- printf "%s:%s" .Values.adminUi.image.repository (default .Chart.AppVersion .Values.adminUi.image.tag) -}}
{{- end -}}

{{- define "oceans-llm.migrationHook" -}}
{{- if or (eq .Values.database.mode "cloudnativepg") .Values.externalSecrets.enabled -}}
{{- "post-install,post-upgrade" -}}
{{- else -}}
{{- .Values.migrations.hook -}}
{{- end -}}
{{- end -}}

{{- define "oceans-llm.migrationWaiterEnabled" -}}
{{- $enabled := false -}}
{{- if .Values.migrations.enabled -}}
{{- if eq (toString .Values.gateway.migrationWaiter.enabled) "true" -}}
{{- $enabled = true -}}
{{- else if eq (toString .Values.gateway.migrationWaiter.enabled) "auto" -}}
{{- $migrationHook := include "oceans-llm.migrationHook" . -}}
{{- if or (contains "post-install" $migrationHook) (contains "post-upgrade" $migrationHook) -}}
{{- $enabled = true -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- $enabled -}}
{{- end -}}

{{- define "oceans-llm.jobNeedsHookConfig" -}}
{{- $migrationHook := include "oceans-llm.migrationHook" . -}}
{{- $needsHookConfig := or (and .Values.migrations.enabled (or (contains "pre-install" $migrationHook) (contains "pre-upgrade" $migrationHook))) (and .Values.bootstrapAdminJob.enabled (or (contains "pre-install" .Values.bootstrapAdminJob.hook) (contains "pre-upgrade" .Values.bootstrapAdminJob.hook))) (and .Values.seedConfigJob.enabled (or (contains "pre-install" .Values.seedConfigJob.hook) (contains "pre-upgrade" .Values.seedConfigJob.hook))) -}}
{{- $needsHookConfig -}}
{{- end -}}

{{- define "oceans-llm.jobConfigMapName" -}}
{{- if eq (include "oceans-llm.jobNeedsHookConfig" .) "true" -}}
{{- printf "%s-job-config" (include "oceans-llm.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- include "oceans-llm.configMapName" . -}}
{{- end -}}
{{- end -}}

{{- define "oceans-llm.jobInlineSecretName" -}}
{{- if eq (include "oceans-llm.jobNeedsHookConfig" .) "true" -}}
{{- printf "%s-job-env" (include "oceans-llm.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- include "oceans-llm.inlineSecretName" . -}}
{{- end -}}
{{- end -}}

{{- define "oceans-llm.jobServiceAccountName" -}}
{{- if and .Values.serviceAccount.create (eq (include "oceans-llm.jobNeedsHookConfig" .) "true") -}}
{{- printf "%s-job" ((include "oceans-llm.serviceAccountName" .) | trunc 59 | trimSuffix "-") -}}
{{- else -}}
{{- include "oceans-llm.serviceAccountName" . -}}
{{- end -}}
{{- end -}}

{{- define "oceans-llm.jobPodLabels" -}}
{{- include "oceans-llm.labels" . }}
app.kubernetes.io/component: {{ .component }}
{{- end -}}

{{- define "oceans-llm.validateValues" -}}
{{- $configJson := toJson .Values.gateway.config -}}
{{- if and (not .Values.gateway.allowLiteralSecretsInConfig) (contains "literal." $configJson) -}}
{{- fail "gateway.config contains literal.* references; use env.* references backed by secrets, or set gateway.allowLiteralSecretsInConfig=true to opt in" -}}
{{- end -}}
{{- $hasPostgresExtraEnv := false -}}
{{- range .Values.gateway.extraEnv -}}
{{- if eq .name "POSTGRES_URL" -}}
{{- $hasPostgresExtraEnv = true -}}
{{- end -}}
{{- end -}}
{{- $databaseUrl := dig "database" "url" "" .Values.gateway.config -}}
{{- $hasInlinePostgresUrl := hasKey .Values.secrets.inline "POSTGRES_URL" -}}
{{- $hasPostgresSource := or .Values.database.external.existingSecret.name .Values.secrets.existingSecret.name .Values.externalSecrets.enabled $hasInlinePostgresUrl $hasPostgresExtraEnv -}}
{{- if and (eq .Values.database.mode "external") (eq $databaseUrl "env.POSTGRES_URL") (not $hasPostgresSource) -}}
{{- fail "database.mode=external with gateway.config.database.url=env.POSTGRES_URL requires database.external.existingSecret.name, secrets.inline.POSTGRES_URL, secrets.existingSecret.name, externalSecrets.enabled, or gateway.extraEnv POSTGRES_URL" -}}
{{- end -}}
{{- end -}}

{{- define "oceans-llm.commonEnv" -}}
- name: GATEWAY_CONFIG
  value: {{ .Values.gateway.configMountPath | quote }}
- name: ADMIN_UI_UPSTREAM
  value: {{ printf "http://%s:%v" (include "oceans-llm.adminUiServiceName" .) .Values.service.adminUi.port | quote }}
- name: GATEWAY_RUN_MIGRATIONS
  value: "false"
- name: GATEWAY_BOOTSTRAP_ADMIN
  value: "false"
- name: GATEWAY_SEED_CONFIG
  value: "false"
{{- if eq .Values.database.mode "external" }}
{{- with .Values.database.external.existingSecret.name }}
- name: POSTGRES_URL
  valueFrom:
    secretKeyRef:
      name: {{ . | quote }}
      key: {{ $.Values.database.external.existingSecret.key | quote }}
{{- end }}
{{- else if eq .Values.database.mode "cloudnativepg" }}
- name: POSTGRES_USER
  valueFrom:
    secretKeyRef:
      name: {{ include "oceans-llm.cnpgCredentialsSecretName" . | quote }}
      key: username
- name: POSTGRES_PASSWORD
  valueFrom:
    secretKeyRef:
      name: {{ include "oceans-llm.cnpgCredentialsSecretName" . | quote }}
      key: password
- name: POSTGRES_URL
  value: {{ printf "postgres://$(POSTGRES_USER):$(POSTGRES_PASSWORD)@%s-rw:5432/%s" (include "oceans-llm.cnpgClusterName" .) .Values.database.cloudnativepg.database | quote }}
{{- end }}
{{- with .Values.observability.env }}
{{ toYaml . }}
{{- end }}
{{- with .Values.gateway.extraEnv }}
{{ toYaml . }}
{{- end }}
{{- end -}}

{{- define "oceans-llm.envFrom" -}}
{{- if .Values.secrets.existingSecret.name }}
- secretRef:
    name: {{ .Values.secrets.existingSecret.name | quote }}
{{- end }}
{{- if .Values.secrets.inline }}
- secretRef:
    name: {{ include "oceans-llm.inlineSecretName" . | quote }}
{{- end }}
{{- if .Values.externalSecrets.enabled }}
- secretRef:
    name: {{ include "oceans-llm.externalSecretTargetName" . | quote }}
{{- end }}
{{- with .Values.gateway.extraEnvFrom }}
{{ toYaml . }}
{{- end }}
{{- end -}}

{{- define "oceans-llm.jobEnv" -}}
{{ include "oceans-llm.commonEnv" . }}
{{- end -}}

{{- define "oceans-llm.jobEnvFrom" -}}
{{- if .Values.secrets.existingSecret.name }}
- secretRef:
    name: {{ .Values.secrets.existingSecret.name | quote }}
{{- end }}
{{- if .Values.externalSecrets.enabled }}
- secretRef:
    name: {{ include "oceans-llm.externalSecretTargetName" . | quote }}
{{- end }}
{{- with .Values.gateway.extraEnvFrom }}
{{ toYaml . }}
{{- end }}
{{- end -}}

{{- define "oceans-llm.renderProbe" -}}
{{- $probe := .probe -}}
httpGet:
  path: {{ $probe.path | quote }}
  port: http
initialDelaySeconds: {{ $probe.initialDelaySeconds }}
periodSeconds: {{ $probe.periodSeconds }}
timeoutSeconds: {{ $probe.timeoutSeconds }}
failureThreshold: {{ $probe.failureThreshold }}
{{- end -}}
