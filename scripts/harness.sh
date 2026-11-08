#!/bin/sh
# usage: harness.sh ACTOR ISSUE [FILES] -- command...
set -u
ACTOR=$1; ISSUE=$2; FILES=${3:-}; shift 3; [ "$1" = "--" ] && shift
API=${MAGNUS_TEAMS_API:-http://localhost:8080}
post() { curl -s -X POST "$API/events" -d "$1" >/dev/null; }
post "{\"type\":\"claim\",\"actor\":\"$ACTOR\",\"issue\":$ISSUE,\"files\":[]}"
"$@" & pid=$!
while kill -0 $pid 2>/dev/null; do post "{\"type\":\"heartbeat\",\"actor\":\"$ACTOR\",\"issue\":$ISSUE}"; sleep 60 & wait $! 2>/dev/null; kill -0 $pid 2>/dev/null || break; done
wait $pid; code=$?
post "{\"type\":\"handoff\",\"actor\":\"$ACTOR\",\"issue\":$ISSUE,\"text\":\"harness exit $code\"}"
exit $code
