# A run's sandbox inputs are held until its workspace takes them

**Choice.** The files a run's sandbox starts with (its attachments, the library files it was given, an opened attachment in its own sandbox) are held in memory only until the new workspace takes them. The first boot takes them out of the holder instead of copying them; a run's end (`Sandboxes::destroy`, which an attachment's own sandbox also goes through) lets go of any still held; an erasure lets go of every run of the person's; a run that has ended, or whose workspace is already made, is given none.

The attachment hold itself (`attachment_hold`) already keeps its hour and its 1 GiB cap. No separate hour or cap is added for sandbox inputs: with these releases they live no longer than the run, and a copy waits only until the first boot.

**Reason.** Every set of inputs stayed in memory until the server restarted: most runs never boot a sandbox, an opened attachment's sandbox was destroyed without letting go of its copy, and an erasure did not reach them. An attachment held under the hold's promise of an hour lived on as that copy.
