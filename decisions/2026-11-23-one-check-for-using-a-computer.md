# One check for using a computer, and one way to read its files

A workspace member could take control of the workspace's computer and export
its whole volume, but listing, reading, saving or deleting one file came back
"no such computer": those routes checked that the caller may open it, then
called a helper that also required control (a Team Admin).

There is now one check, `computer_as(who, id, level)`, with three levels:

- **Open**: the screen and its person's files. Its person, or any member of
  its workspace. Taking control, exporting, and listing, reading, saving and
  deleting one file all need this, and only this. A member who may take
  control and export everything gains nothing more from one file, so the
  single-file routes follow the takeover and export, not the other way.
- **Control**: changing what the computer is (egress, credentials, apps,
  backups, resets). Its person, or the workspace's Team Admins.
- **Stop**: pausing it. Those who control it, and a server owner or admin.

A computer's files are reached one way, `with_files`: the live guest, or
for a stopped computer one helper VM started once for the whole use (an
export used to start one per file). Every whole-file read goes in 4 MiB
pieces with one ceiling, 128 MiB (`MAX_FILE_BYTES`, what an export already
promised in all): download, a takeover operator's read and export. Each
used to ask for the whole file in one answer, which failed past about 48 MB
and made the guest hold it all. A delivery keeps its own smaller ceiling,
25 MB, which is what one delivery carries.
