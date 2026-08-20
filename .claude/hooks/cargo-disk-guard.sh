#!/usr/bin/env bash
# Keep Cargo build artifacts from filling the disk.
#
# Requested by the repository owner, whose machine has twice been made unusable by
# `target/` reaching 23 GB. Runs before every Bash tool call and does nothing at
# all unless the command mentions `cargo`. Then, by free space:
#
#   any        remove `target/*/incremental` if it holds anything — the dev
#              profile sets `incremental = false`, so content there is stale waste
#   < 25 GB    say so, and continue
#   < 10 GB    remove `target/` outright, because a build starting from here can
#              make the machine unusable before it finishes
#
# It never touches source, and everything it removes is rebuildable. Thresholds
# are the two constants below.
set -uo pipefail

WARN_GB=25
EMERGENCY_GB=10
STALE_CACHE_MB=256

payload=$(cat)

# Fast path: this runs before every Bash call, so reject the common case without
# spawning an interpreter. The word cannot appear in the payload at all if the
# command does not mention it.
case "$payload" in
  *cargo*) ;;
  *) exit 0 ;;
esac

json_get() {
  # $1: python expression over `d`, the parsed hook payload
  /usr/bin/python3 -c "
import json, sys
try:
    d = json.load(sys.stdin)
except Exception:
    sys.exit(0)
print($1)
" 2>/dev/null
}

cmd=$(printf '%s' "$payload" | json_get 'd.get("tool_input", {}).get("command", "")')
case "$cmd" in
  *cargo*) ;;
  *) exit 0 ;;
esac

repo="${CLAUDE_PROJECT_DIR:-$PWD}"
target="$repo/target"
[ -d "$target" ] || exit 0

# `df -g` reports whole gigabytes; field 4 is available space.
avail_gb() { df -g "$1" 2>/dev/null | awk 'NR==2 {print $4}'; }

# Surface a message to the user. Anything on stdout that is not JSON is only
# visible in transcript mode, so use the documented envelope.
say() {
  /usr/bin/python3 -c 'import json,sys; print(json.dumps({"systemMessage": sys.argv[1]}))' "$1"
}

# Whole MB below a gigabyte, one decimal above it. Integer division alone reports
# a freed 900 MB cache as "0 GB".
human_mb() {
  if [ "$1" -lt 1024 ]; then
    printf '%s MB' "$1"
  else
    printf '%s.%s GB' "$(($1 / 1024))" "$((($1 % 1024) * 10 / 1024))"
  fi
}

freed_mb=0
for dir in "$target"/*/incremental; do
  [ -d "$dir" ] || continue
  kb=$(du -sk "$dir" 2>/dev/null | cut -f1)
  [ -n "${kb:-}" ] || continue
  mb=$((kb / 1024))
  [ "$mb" -ge "$STALE_CACHE_MB" ] || continue
  rm -rf "$dir" && freed_mb=$((freed_mb + mb))
done

avail=$(avail_gb "$target")
[ -n "${avail:-}" ] || exit 0

if [ "$avail" -lt "$EMERGENCY_GB" ]; then
  kb=$(du -sk "$target" 2>/dev/null | cut -f1)
  rm -rf "$target"
  say "Disk guard: ${avail} GB free, so target/ ($(human_mb $(( ${kb:-0} / 1024 )))) was removed before this command. The next build is cold."
  exit 0
fi

if [ "$freed_mb" -gt 0 ]; then
  say "Disk guard: removed $(human_mb "$freed_mb") of stale incremental cache; ${avail} GB free."
  exit 0
fi

if [ "$avail" -lt "$WARN_GB" ]; then
  say "Disk guard: ${avail} GB free. A full build of every feature set needs about 20 GB. Below ${EMERGENCY_GB} GB this hook removes target/ on its own."
fi

exit 0
