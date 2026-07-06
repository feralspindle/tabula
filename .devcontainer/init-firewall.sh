#!/usr/bin/env bash
# Egress allowlist for the agent sandbox. Default-deny outbound; permits only
# the endpoints agentic development here actually needs: Anthropic/OpenAI/
# OpenRouter APIs, GitHub, npm, crates.io, the Playwright CDN, DNS, and the
# host-side supabase ports.
#
# Runs at every container start (rules do not persist) via postStartCommand.
# Known tradeoff: DNS (port 53) is open, so DNS tunneling is not blocked.

set -euo pipefail

# Any failure below must not leave the sandbox with open egress: hard-drop
# everything except loopback and established flows, then surface the error.
fail_closed() {
  iptables -F OUTPUT 2>/dev/null || true
  iptables -A OUTPUT -o lo -j ACCEPT 2>/dev/null || true
  iptables -A OUTPUT -m state --state ESTABLISHED,RELATED -j ACCEPT 2>/dev/null || true
  iptables -P OUTPUT DROP 2>/dev/null || true
  echo "init-firewall: FAILED — egress hard-dropped (fail closed)" >&2
}
trap fail_closed ERR

ALLOWED_DOMAINS=(
  api.anthropic.com
  statsig.anthropic.com
  sentry.io
  api.openai.com
  auth.openai.com
  chatgpt.com
  openrouter.ai
  models.dev
  registry.npmjs.org
  crates.io
  index.crates.io
  static.crates.io
  cdn.playwright.dev
  playwright.azureedge.net
)

# Host-side supabase: API (54321), Postgres (54322), Studio (54323), Mailpit (54324).
HOST_TCP_PORTS="54321,54322,54323,54324"

ipset destroy allowed-egress 2>/dev/null || true
ipset create allowed-egress hash:net

echo "init-firewall: resolving GitHub IP ranges"
github_meta="$(curl -fsSL --max-time 20 https://api.github.com/meta)"
for range in $(echo "$github_meta" | jq -r '.git[], .api[], .web[]' | grep -v ':' | sort -u); do
  ipset add allowed-egress "$range" 2>/dev/null || true
done

echo "init-firewall: resolving allowlisted domains"
for domain in "${ALLOWED_DOMAINS[@]}"; do
  for ip in $(dig +short A "$domain" | grep -E '^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$'); do
    ipset add allowed-egress "$ip" 2>/dev/null || true
  done
done

host_gateway="$(getent ahostsv4 host.docker.internal 2>/dev/null | awk '{print $1}' | head -1 || true)"

iptables -F OUTPUT
iptables -F INPUT
iptables -A OUTPUT -o lo -j ACCEPT
iptables -A OUTPUT -m state --state ESTABLISHED,RELATED -j ACCEPT
iptables -A OUTPUT -p udp --dport 53 -j ACCEPT
iptables -A OUTPUT -p tcp --dport 53 -j ACCEPT
if [ -n "$host_gateway" ]; then
  iptables -A OUTPUT -d "$host_gateway" -p tcp -m multiport --dports "$HOST_TCP_PORTS" -j ACCEPT
fi
# HTTPS/HTTP only, even to allowlisted hosts: git goes over HTTPS with a PAT
# in this sandbox, so outbound SSH stays blocked everywhere (incl. GitHub).
iptables -A OUTPUT -m set --match-set allowed-egress dst -p tcp -m multiport --dports 443,80 -j ACCEPT
iptables -P OUTPUT DROP

iptables -A INPUT -i lo -j ACCEPT
iptables -A INPUT -m state --state ESTABLISHED,RELATED -j ACCEPT
iptables -P INPUT DROP

# IPv6: no allowlist, just close it so nothing bypasses the v4 rules.
if command -v ip6tables >/dev/null 2>&1; then
  ip6tables -F OUTPUT 2>/dev/null || true
  ip6tables -A OUTPUT -o lo -j ACCEPT 2>/dev/null || true
  ip6tables -P OUTPUT DROP 2>/dev/null || true
fi

echo "init-firewall: verifying"
if curl -fsS --max-time 5 https://example.com >/dev/null 2>&1; then
  echo "init-firewall: FAILED — example.com is reachable, egress is not locked down" >&2
  exit 1
fi
if ! curl -fsS --max-time 15 https://api.github.com/zen >/dev/null 2>&1; then
  echo "init-firewall: WARNING — api.github.com not reachable; allowlist may be too strict or network is down" >&2
fi
echo "init-firewall: egress locked to allowlist"
