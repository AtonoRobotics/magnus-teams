# Harness pack

Wrap any external agent CLI so it coordinates through the kernel:

    MAGNUS_TEAMS_API=http://100.92.130.94:8080 \
      scripts/harness.sh codex 42 -- codex exec "fix issue 42"

It claims the issue, heartbeats while the command runs, and posts a
handoff with the exit code. Works unchanged for `claude`, `codex`,
or any command. The agent inside still does its own work; the
harness only speaks the five messages around it.
