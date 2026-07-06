#!/usr/bin/env bash
# named volumes are created root-owned on first use; hand them to the agent
# user. this is the only root operation the sandbox user can run besides the
# firewall init (see /etc/sudoers.d/sandbox).

set -euo pipefail

for dir in /workspaces/*/target /workspaces/*/web/node_modules /home/node/.cargo/registry; do
  [ -d "$dir" ] && chown -R node:node "$dir"
done
exit 0
