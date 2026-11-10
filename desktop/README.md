# Magnus desktop app

A client of the appliance for Linux and Windows. It signs in as the person, keeps only their session (in the OS keyring), and is where people make every decision the agent needs: approvals never pass through the agent's context. It holds no provider or tool credentials. Keys typed in the Models view go straight to the appliance's vault.

The appliance serves the same frontend (`ui/`) to browsers.

## Layout

- `ui/`: the frontend, plain JavaScript with no build step.
  - Operator view (`app.js`): Home, Inbox, New task, Tasks, Task detail, Models.
  - Developer view (`dev.js`): Runs and the run inspector, Experiments, System, Policy, Playbook. It needs the `developer` role.
  - `compose.js` draws composed outcomes inside a frame that cannot run scripts.
  - `bridge.js` is the only part that differs between the app and a browser.
- `src-tauri/`: the Rust side.
  - `pin.rs`: certificate pinning.
  - `store.rs`: settings and the session.
  - `speech.rs`: read-aloud in the system's voice, used only when the appliance has no voice of its own.
  - `lib.rs`: the commands the page may call.
- `tests/flow.mjs`: the operator flow in a browser, signing in with a password or through an identity provider.
- `tests/app.mjs`: the same flow in the built app, through tauri-driver.

## Build

You need `cargo-tauri` (`cargo install tauri-cli --version ^2`). Run these in `desktop/src-tauri`.

Linux (needs the WebKitGTK 4.1 development packages):

```
cargo tauri build --bundles deb,appimage
```

The `.deb` also carries `magnus-local`, the agent that works on this computer once the person connects it, so bundling builds it first.

This writes `target/release/bundle/deb/Magnus_<version>_amd64.deb` and `target/release/bundle/appimage/Magnus_<version>_amd64.AppImage`.

Windows, cross-built on Linux. It needs `rustup target add x86_64-pc-windows-gnu`, mingw-w64 and NSIS:

```
cargo tauri build --runner cargo --target x86_64-pc-windows-gnu --bundles nsis
```

This writes `target/x86_64-pc-windows-gnu/release/bundle/nsis/Magnus_<version>_x64-setup.exe`. The installer installs for the current user, and brings the WebView2 runtime if the machine lacks it.

## Before the first sign-in (owner, on the appliance)

1. Turn on HTTPS: set `appliance.tls_listen` (for example `"0.0.0.0:7700"`) and restart. The appliance prints its certificate's SHA-256 fingerprint at start. People compare it once, when the app first connects.
2. To sign in through Keycloak (or another OpenID Connect provider), register the appliance as a client and add `[sign_in.oidc]` to `magnus.toml`:

   ```
   sudo python3 -I config/keycloak-client.py
   ```

   It creates a public client with only the device-code grant, and prints the lines for `magnus.toml` (`issuer`, `client_id`). Each person's Keycloak username must be their `id` in `[[people]]`.
3. Optionally, or without a provider, give each person a password. With a provider it is the backup way in:

   ```
   magnus password --config magnus.toml --person <id>
   ```

   The password is read from standard input. Passwords are refused over plain HTTP except from the appliance itself.
4. Optional: add the `developer` role to anyone who should see the developer view.

## The recorded run

On each platform:

1. Install the app and open it.
2. Type the appliance's address (for example `https://192.168.0.50:7700`). Check that the fingerprint matches the one the appliance printed, then choose **They match**.
3. Choose **Sign in with identity.pyrallon.ai**. Your browser opens the Keycloak page with a code; check that it matches the code in the app, sign in there and approve. The app finishes by itself. (**Use a password instead** is the backup.)
4. Go to **Models**. On the Claude adapter choose **Connect**, paste the API key, then **Save to the vault**. Pick the model and choose **Use this model**.
5. In **New task**, type the task in plain language and choose **Start**.
6. In **Inbox**, the card under **Needs you now** shows exactly what will happen. Choose **Approve once**. If a review card follows, accept it once you have checked the work.
7. Open the task. **Outcome** shows the result, the delivered files and the evidence for each criterion. **Full trace** shows every step.

## Tests

- `cargo test -p magnus-scratch --test desktop_flow`: the frontend in Chromium against a real appliance, once with a password and once through a fake identity provider.
- `cargo test -p magnus-scratch --test desktop_app -- --ignored`: the installed Linux app (the full flow, and a download from the library). Every feature that crosses into the desktop shell (downloads, uploads, sign-in links, notifications, file access) has at least one test here.
  - It needs `tauri-driver` (`cargo install tauri-driver --locked`), `WebKitWebDriver` (package `webkit2gtk-driver`, or set `MAGNUS_WEBKIT_DRIVER`) and `xvfb-run`.
  - `MAGNUS_DESKTOP_APP=<binary>` tests a given build, for example one unpacked from a released `.deb` with `dpkg-deb -x`.
  - Set `MAGNUS_SHOTS=<dir>` to keep screenshots.
- `cargo test` in `src-tauri`: what the page may call, and address handling.

## Release checks

`.github/workflows/desktop.yml` builds the installers, runs the installed-app tests on Linux, and, for a `desktop-v<version>` tag, publishes a pre-release with those installers only when every test passed. The Windows build is the same app and frontend; it is not tested separately. Installers are no longer published by hand.

To release: bump the version in `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, commit, then push an annotated tag whose message is the release notes: `git tag -a desktop-v0.5.0 -m "What changed, in plain words." && git push origin desktop-v0.5.0`.

The jobs run on a self-hosted runner on the build box (the machine with KVM, the guest image and the test Postgres). One-time setup:

1. `sudo apt install webkit2gtk-driver`, and `cargo install tauri-driver --locked`.
2. Register a runner with the label `magnus-linux`, running as your own user. In GitHub: Settings, Actions, Runners, New self-hosted runner (Linux); follow its download and `./config.sh` steps, adding `--labels magnus-linux`. Install it as a service: `sudo ./svc.sh install samuel && sudo ./svc.sh start`. Put `PATH=/home/samuel/.cargo/bin:/home/samuel/.local/bin:/usr/local/bin:/usr/bin:/bin` in the runner's `.env` file so it finds `cargo` and `node`. (Done on the build box on 2026-10-29: runner `build-box` in `~/actions-runner`, running as a systemd service.) The workflow is one job: its steps share the build box's disk, because GitHub's artifact storage for this private repository is full.
3. Repository variables (Settings, Secrets and variables, Actions, Variables): `MAGNUS_UPDATES_DIR`, the appliance's updates directory (set: `~/.magnus-appliance/state/updates`); `MAGNUS_APPLIANCE_CONFIG`, the build box appliance's `magnus.toml`, which master builds are put on (set: `~/.magnus-appliance/magnus.toml`). The sandbox image is built per commit (`sandbox/image.sh`), and Postgres and Keycloak are started by `dev/services.sh`.
4. The release key, `~/.magnus-teams-release/update.key` (mode 600, on the build box only; its public half is `release/update.pub` and `plugins.updater.pubkey`). Keep an offline copy: without it no installed app or appliance accepts another update.

## Updates

A build that passes its tests is also put, signed, in the appliance's updates directory (`release/publish.sh`), in its release channel: a pull request's in development, master's in test, a tag in production. The appliance decides which channel each app gets: the owner's apps development, testers' test, everyone else's production (DECISIONS.md, "Release channels"); an app only moves forward. Installed apps ask the appliance for their release when a session begins and every hour, download it in the background, and offer "Restart to update"; after the restart they show the tag's message as "What's new", so write it for the person. On Windows the installer updates the app in place; on Linux the AppImage updates itself, and a .deb is updated with the package manager. Version 0.5.0 is the first with the updater: 0.4.5 and earlier are replaced by hand once.
