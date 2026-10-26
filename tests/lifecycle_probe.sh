#!/bin/sh
set -eu
probe_dir="$1"
mkdir -p "$probe_dir"
rm -f "$probe_dir/child-exit"
printf '%s\n' "$$" > "$probe_dir/child-pid"
trap 'printf "hangup\n" > "$probe_dir/child-exit"; exit 0' HUP TERM
printf 'ready\n' > "$probe_dir/ready"
probe_ticks=0
while [ "$probe_ticks" -lt 300 ]; do
    sleep 1
    probe_ticks=$((probe_ticks + 1))
done
printf 'timed-out\n' > "$probe_dir/child-exit"
