#!/bin/sh
# Start (or restart) the TheAgentCompany server stack with our overrides
# (README 1.2-1.3). The api-server rebuilds every service on start. Runtime
# state (Plane's generated env and archives) lives in $TAC_HOME/stack.
set -eu
T=$(cd "$(dirname "$0")" && pwd)
RUN=${TAC_HOME:-$HOME/.magnus-tac}/stack
mkdir -p "$RUN"
[ -d "$RUN/plane-app" ] || cp -r "$T/plane/plane-app" "$RUN/plane-app"
docker rm -f api-server >/dev/null 2>&1 || true
docker run -d --name api-server --network host --restart no \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v "$T/docker-compose.yml:/workspace/docker-compose.yml:ro" \
  -v "$T/Makefile:/workspace/Makefile:ro" \
  -v "$T/plane/Makefile:/plane/Makefile:ro" \
  -v "$RUN/plane-app:/plane/plane-app" \
  ghcr.io/theagentcompany/servers-api-server:1.0.0 >/dev/null
echo "api-server started; waiting for the services"
python3 -I - <<'PY'
import time, urllib.request
deadline = time.time() + 1800
pending = ["rocketchat", "owncloud", "gitlab", "plane"]
while pending and time.time() < deadline:
    time.sleep(10)
    for s in list(pending):
        try:
            if urllib.request.urlopen(f"http://localhost:2999/api/healthcheck/{s}", timeout=10).status == 200:
                print(f"{s} healthy", flush=True)
                pending.remove(s)
        except Exception:
            pass
raise SystemExit(f"not healthy after 30 minutes: {', '.join(pending)}" if pending else 0)
PY
