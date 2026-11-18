# The desktop app's page loads its files by versioned URLs

## Choice
The release stamps `desktop/ui/index.html` before it builds the desktop app (`release/stamp-ui.sh "$MAGNUS_VERSION"`): every stylesheet and script is loaded as `NAME?v=VERSION`. The repository's page keeps plain names; only the built app carries the stamp. `desktop/tests/stamp-ui-check.mjs` checks the stamp and that the release runs it before `cargo tauri build`.

The right panel's section headings are plain labels: "Approvals", "Apps", "Files", in place of wording carried over from the mockup ("Inline decision cards with real brand SVGs", "Connected integrations & 1-click OAuth", "Workspace & conversation artifacts").

## Reason
Desktop 0.7.1-test.218 held the redesigned right panel (its embedded `app.js`, `layout.css` and `pages.css` match the repository), yet the owner's updated app still showed the old panel. The app's webview keeps what it loaded under a URL and reuses it after an update; the page asked for the same plain names in every release. 53ebc6a versioned the URLs only in the copy the server serves to browsers (`crates/magnus/src/ui/assets.rs`), never in the app's own bundle. The app finds a bundled file by its path, so the query string changes nothing else.

The headings were mockup notes, not product wording, and shipped to everyone on 218.
