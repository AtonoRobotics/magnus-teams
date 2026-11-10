# Files leave in bulk, and the ones Magnus made on the person's own computer are listed

The spec (Observability, audit, export and deletion › Export): "Every file Magnus generates, in the computer or on the user's desktop, can be downloaded and deleted from the client, one at a time or in bulk", and "the user volume and the app manifest export as ordinary files."

## A cloud computer's files

- `GET /v1/computers/{id}/export?paths=` returns one ordinary tar file. `paths` is one path per line under `/workspace/`, a folder ending in `/`, walked through the guest's own `list` and `read`. No paths is the whole user volume, under `files/`, with the app manifest beside it as `apps.json`.
- It is held in memory while packed, so it stops at 128 MB or 10,000 files and says to pick fewer. Streaming it would lift that; nothing asks for more yet.
- The file explorer gains "Download selected" and "Download everything". Bulk delete was already there.
- The desktop app fetched only task outputs, library files and chat exports as files, so the explorer's existing Download button was refused in the installed app. The list of file calls moved into `desktop/src-tauri/src/files.rs` and now takes a computer's file, its export, and a file Magnus made on the person's own computer.

## The person's own computer

- When `local_write_file` succeeds, the log records a `local_file` event (`written`), and `local_delete` records `deleted`. A new kind, so no existing kind changed. Only a write counts as a file Magnus made: an edit changes the person's own file, and what a command writes cannot be told apart from what was there.
- `GET /v1/local/computers` carries each computer's `files`. The app lists them under that computer with Download and Delete for each and for those picked.
- `GET /v1/local/computers/{id}/files/raw?path=` and `DELETE /v1/local/computers/{id}/files?path=` reach only a file on that list, so the person's session cannot be used to read or delete anything else on the machine. They run as jobs on the machine, marked as the person's own: those go while the person is at the machine, where the agent's jobs wait. A file already gone from the machine leaves the list too.
- A run is told of a file the person deleted, as for a cloud computer, for 30 days.
- Downloads come through the server, so they work from any device the person signs in on. The machine's read limit still applies (4 MB, as text).
