#!/usr/bin/env bash
set -euo pipefail

# Hosted runners are disposable. Self-hosted runners must provision their own
# Bubblewrap/AppArmor policy; do not overwrite an administrator's configuration.
if [[ "${RUNNER_ENVIRONMENT:-}" == "github-hosted" ]]; then
  sudo apt-get update
  sudo apt-get install -y bubblewrap apparmor
  if [[ -f /proc/sys/kernel/apparmor_restrict_unprivileged_userns ]] &&
    [[ "$(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns)" == "1" ]]; then
    # Ubuntu ships this restricted profile disabled on some runner images.
    # Enable the distro policy: bwrap gets namespace capabilities, its children
    # do not. Keep the system-wide user-namespace restriction enabled.
    sudo apparmor_parser -r /etc/apparmor.d/bwrap-userns-restrict
  fi
fi

test -x /usr/bin/bwrap || {
  echo 'Install Bubblewrap and permit it in the runner AppArmor policy.' >&2
  exit 1
}
