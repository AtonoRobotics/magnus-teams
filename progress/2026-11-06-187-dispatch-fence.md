# A cancelled task's call does not go out (2026-11-06)

The boundary reads the run again, live, right before an app's call goes out, for remote MCP and brokered connectors alike, so a task cancelled or suspended while its gate waited sends nothing. A resend held back this way is still reported as unknown, since the attempt before it may have reached the app. Gmail's stand-in can now hold a draft read, which is how the brokered test pauses the gate.
