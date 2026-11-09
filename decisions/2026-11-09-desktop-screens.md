# The desktop app's computer and team screens

The app now has screens for computers and teams, and it can connect the computer it runs on.

- **Agent-written pages load from their own origin.** A page with a script is served once from `magnus-frame:` in the app and from `/v1/ui/frames/{id}` on the web, under its own CSP with `sandbox allow-scripts`. A `srcdoc` frame inherits the app's CSP, which stops the page's script, and loosening that CSP would loosen it for the app too.
- **Takeover speaks RFB in plain JavaScript through a relay in the app.** The app holds the socket because only it has the pinned certificate and the device key. `desktop/ui/rfb.js` needs no build step.
- **`magnus-local` ships beside the app and is started by it.** It gets the server's pin and the device certificate so it trusts exactly what the app trusts. It runs as a plain OS process because the Tauri commands that start it run on the interface thread, where there is no async runtime and a tokio process panics.
- **The installed app is tested with a seeded sign-in** through tauri-driver and WebKit, because the browser sign-in cannot be driven in that harness. macOS paths are covered by unit tests only.
