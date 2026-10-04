#!/usr/bin/env bash
set -euo pipefail

chart_dir="deploy/helm/oceans-llm"
check_dir="$(mktemp -d)"
trap 'rm -rf "$check_dir"' EXIT

render() {
  local profile="$1"
  shift
  helm template oceans-llm "$chart_dir" \
    --set database.external.existingSecret.name=oceans-llm-postgres \
    --output-dir "$check_dir/$profile" "$@" >/dev/null
}

require_text() {
  if ! grep -Fq -- "$2" "$1"; then
    echo "Expected '$2' in $1" >&2
    exit 1
  fi
}

reject_text() {
  if grep -Fq -- "$2" "$1"; then
    echo "Unexpected '$2' in $1" >&2
    exit 1
  fi
}

expect_rejected() {
  local message="$1"
  shift
  if helm template oceans-llm "$chart_dir" \
    --set database.external.existingSecret.name=oceans-llm-postgres \
    "$@" >"$check_dir/rejected.log" 2>&1; then
    echo "Expected Helm to reject invalid skill storage configuration" >&2
    exit 1
  fi
  require_text "$check_dir/rejected.log" "$message"
}

render disabled
test ! -d "$check_dir/disabled/oceans-llm/charts/rustfs"
require_text "$check_dir/disabled/oceans-llm/templates/configmap.yaml" 'enabled: false'

render s3 --values "$chart_dir/examples/skills-s3-values.yaml"
test ! -d "$check_dir/s3/oceans-llm/charts/rustfs"
require_text "$check_dir/s3/oceans-llm/templates/configmap.yaml" 'bucket: oceans-skills'
require_text "$check_dir/s3/oceans-llm/templates/configmap.yaml" 'force_path_style: false'
require_text "$check_dir/s3/oceans-llm/templates/configmap.yaml" 'secret_access_key: env.OCEANS_SKILLS_S3_SECRET_ACCESS_KEY'
require_text "$check_dir/s3/oceans-llm/templates/gateway-deployment.yaml" 'name: oceans-skills-s3'

render distributed --values "$chart_dir/examples/skills-rustfs-values.yaml"
distributed_dir="$check_dir/distributed/oceans-llm"
statefulset="$distributed_dir/charts/rustfs/templates/statefulset.yaml"
require_text "$statefulset" 'replicas: 4'
require_text "$statefulset" 'image: "rustfs/rustfs:1.0.1"'
require_text "$statefulset" 'storageClassName: skills-csi'
require_text "$statefulset" 'storage: 20Gi'
require_text "$statefulset" 'accessModes: ["ReadWriteOnce"]'
require_text "$statefulset" 'name: oceans-skills-rustfs-root'
require_text "$statefulset" 'name: RUSTFS_LOCAL_ENDPOINT_HOST'
require_text "$distributed_dir/charts/rustfs/templates/service.yaml" 'type: ClusterIP'
require_text "$distributed_dir/charts/rustfs/templates/poddisruptionbudget.yaml" 'maxUnavailable: 1'
require_text "$distributed_dir/templates/configmap.yaml" 'endpoint: http://oceans-skills-rustfs:9000'
require_text "$distributed_dir/templates/configmap.yaml" 'allow_http: true'
require_text "$distributed_dir/templates/gateway-deployment.yaml" 'name: oceans-skills-s3'
reject_text "$distributed_dir/templates/gateway-deployment.yaml" 'oceans-skills-rustfs-root'
test ! -f "$distributed_dir/charts/rustfs/templates/secret.yaml"
test ! -f "$distributed_dir/charts/rustfs/templates/ingress.yaml"
if grep -RqE '^kind: (Ingress|Gateway|HTTPRoute|TLSRoute)$' "$distributed_dir/charts/rustfs/templates"; then
  echo 'RustFS must remain internal in the bundled example' >&2
  exit 1
fi

render standalone --values "$chart_dir/examples/skills-rustfs-standalone-values.yaml"
standalone_dir="$check_dir/standalone/oceans-llm/charts/rustfs/templates"
require_text "$standalone_dir/deployment.yaml" 'replicas: 1'
require_text "$standalone_dir/pvc.yaml" 'helm.sh/resource-policy: keep'
require_text "$standalone_dir/pvc.yaml" 'storageClassName: skills-csi'
require_text "$standalone_dir/pvc.yaml" 'storage: 10Gi'
test ! -f "$standalone_dir/statefulset.yaml"

for field in access_key_id secret_access_key session_token; do
  expect_rejected "skills.storage.$field must use an env.* or file.* secret reference" \
    --set-string "gateway.config.skills.storage.$field=raw-test-credential"
  expect_rejected 'gateway.config contains literal.* references' \
    --set-string "gateway.config.skills.storage.$field=literal.test-credential"
done

render mounted-secret \
  --set gateway.config.skills.enabled=true \
  --set-string gateway.config.skills.storage.bucket=oceans-skills \
  --set bootstrapAdminJob.enabled=true \
  --set seedConfigJob.enabled=true \
  --set-string gateway.config.skills.storage.access_key_id=file./var/run/skills/access-key \
  --set-string gateway.config.skills.storage.secret_access_key=file./var/run/skills/secret-key \
  --set 'gateway.extraVolumes[0].name=skills-s3-files' \
  --set 'gateway.extraVolumes[0].secret.secretName=oceans-skills-s3' \
  --set 'gateway.extraVolumeMounts[0].name=skills-s3-files' \
  --set 'gateway.extraVolumeMounts[0].mountPath=/var/run/skills' \
  --set 'gateway.extraVolumeMounts[0].readOnly=true'
require_text "$check_dir/mounted-secret/oceans-llm/templates/configmap.yaml" 'access_key_id: file./var/run/skills/access-key'
test "$(grep -Fc 'mountPath: /var/run/skills' "$check_dir/mounted-secret/oceans-llm/templates/gateway-deployment.yaml")" -eq 2
for job in migration bootstrap-admin seed-config; do
  job_file="$check_dir/mounted-secret/oceans-llm/templates/jobs/$job-job.yaml"
  require_text "$job_file" 'mountPath: /var/run/skills'
  require_text "$job_file" 'secretName: oceans-skills-s3'
  test "$(grep -Fc 'name: skills-s3-files' "$job_file")" -eq 2
  test "$(grep -Fc 'readOnly: true' "$job_file")" -eq 2
done

expect_rejected 'secret.rustfs.access_key and secret.rustfs.secret_key must be set' \
  --set rustfs.enabled=true
expect_rejected 'boolean' --set-string rustfs.enabled=not-a-boolean

echo 'Skill storage Helm checks passed.'
