# The server's own update channel

Spec rev 141, Administration › Operations: "Updates (channel, install, roll
back)". Operations offered a channel only for the desktop apps; the server
always updated from production.

Choice: an admin picks the channel the server itself updates from, among the
desktop apps' three (production, test, development), in Admin › Operations ›
Updates. `release/publish.sh appliance` takes the channel from
`MAGNUS_CHANNEL` (its program arguments are a list, so not a positional
argument) and writes test to `updates/appliance/test/` and development to
`updates/appliance/development/`; production stays in `updates/appliance/`,
so every server and release job that names no channel behaves as before.

A server on a channel that has published nothing takes production's release,
as a desktop app does. It still installs only a release that verifies with
the release key, is newer than what runs and was not rolled back from, so
a channel never moves the server backwards.

The setting is a new variant of the existing `admin_setting` event
(`ServerChannel`), so no event kind is added or edited. Setting it takes the
admin step-up and is written to the admin audit (`server_channel_set`, with
the channel before and after) before it is made. Any owner or admin may set
it, as any may install or roll back.
