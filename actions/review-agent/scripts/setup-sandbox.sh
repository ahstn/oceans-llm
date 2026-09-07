#!/usr/bin/env bash
set -euo pipefail

# Hosted runners are disposable. Self-hosted runners must provision their own
# Bubblewrap/AppArmor policy; do not overwrite an administrator's configuration.
if [[ "${RUNNER_ENVIRONMENT:-}" == "github-hosted" ]]; then
  sudo apt-get update
  sudo apt-get install -y bubblewrap apparmor
  if [[ -f /proc/sys/kernel/apparmor_restrict_unprivileged_userns ]] &&
    [[ "$(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns)" == "1" ]]; then
    # Prefer the distro policy. Some Noble images omit it, so use the upstream
    # ABI-4 profile with a verified digest on those disposable runners.
    profile=/etc/apparmor.d/bwrap-userns-restrict
    if [[ ! -f "$profile" ]]; then
      profile=$(mktemp)
      trap 'rm -f "$profile"' EXIT
      curl --fail --silent --show-error --location \
        https://gitlab.com/apparmor/apparmor/-/raw/v4.1.0/profiles/apparmor/profiles/extras/bwrap-userns-restrict \
        --output "$profile"
      echo "634d3d3427c483f123cb5ed53b71ea13040187e07d9f67ca74421d42a6170f0e  $profile" | sha256sum --check
    fi
    # The policy strips child capabilities; the global restriction stays on.
    sudo apparmor_parser -r "$profile"
  fi
fi

test -x /usr/bin/bwrap || {
  echo 'Install Bubblewrap and permit it in the runner AppArmor policy.' >&2
  exit 1
}
