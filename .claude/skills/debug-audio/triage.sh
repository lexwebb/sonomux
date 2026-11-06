#!/bin/bash
# One-shot macOS audio triage for Sonomux. Read-only.
# Usage: triage.sh [since]   since = "YYYY-MM-DD HH:MM:SS" (default: last 2 hours)
set -u
SINCE="${1:-$(date -v-2H '+%Y-%m-%d %H:%M:%S')}"
LOG=~/Library/Logs/sonomuxd.log
SINCE_UTC=$(date -u -j -f '%Y-%m-%d %H:%M:%S' "$SINCE" '+%Y-%m-%dT%H:%M:%S')
strip() { sed 's/\x1b\[[0-9;]*m//g'; }
ulog() { /usr/bin/log show --start "$SINCE" --style compact --predicate "$1" 2>/dev/null | awk 'NR>1'; }

echo "== Daemon"
launchctl print "gui/$(id -u)/org.sonomux.sonomuxd" 2>/dev/null | grep -E '^\s*(state|pid|last exit reason) =' | head -3

echo; echo "== Daemon events since $SINCE (starts, routes, warnings)"
strip <"$LOG" | awk -v t="$SINCE_UTC" '$1>t' | grep -E 'Starting|Routing|Could not|WARN|Default (in|out)put set|Removed stale|Created CoreAudio' | cut -c1-200 | tail -30

echo; echo "== Route rebuilds per minute (more than 3 in a minute = flapping)"
strip <"$LOG" | awk -v t="$SINCE_UTC" '$1>t && /Routing/ {print substr($1,1,16)}' | uniq -c | awk '$1>3' | tail -10

echo; echo "== coreaudiod IO overloads per minute (dropouts, garbled audio)"
ulog 'process == "coreaudiod" AND eventMessage CONTAINS "Audio IO Overload thread"' | awk '{print substr($2,1,5)}' | uniq -c | tr '\n' ' '; echo

echo; echo "== Mic/audio indicator changes (count, then the last 10)"
ATTR=$(ulog 'process == "ControlCenter" AND eventMessage CONTAINS "Active activity attributions changed"')
echo "$ATTR" | grep -c .
echo "$ATTR" | sed 's/ Df ControlCenter.*changed to//' | cut -c12-200 | tail -10

echo; echo "== Apps muted by Sonomux taps (last 5)"
ulog 'process == "coreaudiod" AND eventMessage CONTAINS "muted by sonomux"' | sed 's/.*AddMuter: //' | cut -c1-120 | tail -5

echo; echo "== Sleep/wake"
pmset -g log 2>/dev/null | awk -v d="${SINCE%% *}" '($4=="Sleep" || $4=="Wake") && $1>=d' | cut -c1-110 | tail -6
