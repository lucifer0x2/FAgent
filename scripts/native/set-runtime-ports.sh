#!/bin/bash
# Update only numeric listener settings in the private generated configuration.
set -euo pipefail
umask 077
config=${1:?usage: set-runtime-ports.sh SERVER_ENV LOCAL_PORT PUBLIC_PORT}
local_port=${2:?missing local port}
public_port=${3:?missing public port}
for port in "$local_port" "$public_port"; do
  [[ "$port" =~ ^[1-9][0-9]{0,4}$ ]] && ((port<=65535))
done
[[ "$local_port" != "$public_port" ]]
[[ $(awk '/^FAGENT_PORT=/ {n++} END {print n+0}' "$config") == 1 ]]
[[ $(awk '/^FAGENT_PUBLIC_PORT=/ {n++} END {print n+0}' "$config") == 1 ]]
temporary=$(mktemp "$(dirname "$config")/.server-ports.XXXXXX")
trap 'rm -f "$temporary"' EXIT
awk -v local_port="$local_port" -v public_port="$public_port" '
  /^FAGENT_PORT=/ {print "FAGENT_PORT=" local_port; next}
  /^FAGENT_PUBLIC_PORT=/ {print "FAGENT_PUBLIC_PORT=" public_port; next}
  {print}
' "$config" > "$temporary"
chmod 600 "$temporary"
mv "$temporary" "$config"
