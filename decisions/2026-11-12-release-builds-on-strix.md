# The release is built on Strix Beta, and the production server only publishes it

**Choice.** The release workflow has two jobs.

`build` runs on Strix Beta (runner `strix-beta-teams`, label `strix-build`). It builds:
- the programs;
- the desktop app, signed with a throwaway key from `release/signing-key.sh`;
- the sandbox image;
- the connectors' server images.

It leaves them in `~/magnus-teams-release/<run id>/` (`release/hand-over.sh`).

`publish` runs on the production server. It fetches that build over the LAN (`release/fetch-build.sh`) and copies the sandbox image only when its own store lacks it. It then signs and publishes the desktop app with the release key (`release/publish.sh`, as before), and publishes and installs its own programs. `release/to-appliance.sh` takes the built images as `MAGNUS_IMAGE` and `MAGNUS_BROKERED_IMAGES`, and builds them itself only when they are not given.

**Reason.** The production server builds nothing (`2026-11-12-no-builds-on-the-production-server.md`). The release built the whole workspace, two Tauri bundles, a sandbox image of several gigabytes and seven static connector builds on it. On 2026-11-11 that load, together with the agents' local tests, starved the server until it had to be power-cycled.

The release key stays on the production server, as before. Strix never has it, because `publish.sh` signs every package with the key file at publish time, so the throwaway signature from the build is never published. The build is fetched over the LAN, not passed through GitHub artifacts, because the sandbox image is several gigabytes and rarely changes.
