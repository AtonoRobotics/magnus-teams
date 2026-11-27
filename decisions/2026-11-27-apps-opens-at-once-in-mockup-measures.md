# Apps opens at once, in the mockup's measures

The owner, 2026-11-27: "Apps opens with a very long delay. Fonts, font
size, padding / space and layouts don't match mockup."

**The delay.** There were two causes:

- For someone who manages shared apps, `/v1/connectors` asked each signed-in
  app for its tools one after another. It now asks them all at once.
- Apps waited for that list before drawing anything. It now draws from the
  list the app already holds (`App.connectors`, read at sign-in and kept
  fresh), then refreshes in place.

**The look (Nova only).** Apps follows `docs/nova-greenfield-mockup.html`
§6 ("Apps & Skills"):

- **Page:** a centred 896px column, with its parts 32px apart.
- **Title row:** a 24px title in medium weight, with the Yours/Discover
  segmented pill on the same row as the title, styled as the mockup's
  `.seg` (12px, 3px padding).
- **Spotlight:** half art, half copy, with 24px corners, as in the mockup.
  - It shows a real app from the catalog: Notion first, as in the mockup,
    and otherwise one that is not yet connected.
  - The mockup's "+ Add" starts that app's sign-in, and "Details" opens its
    page.
- **Catalog:** a 14px label per section ("Popular ›"). Under it, two
  columns of the mockup's `.cat-row` rows:
  - 12px by 10px padding, no border, 14px corners;
  - a 40px tile;
  - a 14px name over a one-line 12px summary;
  - a 32px round action.
  - The action shows the status: +, a check, or sign in again. The status
    line and "Details →" are gone, and the name opens the app's page.
- **Beyond the mockup, kept below the catalog:**
  - search, under the spotlight;
  - programs on this computer;
  - outside agents;
  - "Ask Magnus to find an app".
  The category chips are hidden in Nova, since search covers them.

**Tests:**

- `connectors::routes::tests::an_owners_apps_asks_every_app_at_once` checks
  the server change.
- `desktop/tests/nova-screens.mjs` checks the measures, in dark and light.
