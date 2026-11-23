# Held attachments are known by their contents

The third architecture review (candidate 12) found that a fetched
attachment's bytes were held by run and file name, while its record kept
its SHA-256. Mail reuses names (`image0.jpeg`, `invite.ics`): a run that
read two messages with an `image0.jpeg` each kept only the second's bytes,
and opening either gave those bytes, possibly beside the first's recorded
size and digest, with no error.

**The hold is keyed by run and SHA-256.** Two attachments with one name are
two holdings; the same attachment fetched twice is one.

**Each attachment has a reference:** its name and the first 8 hex of its
SHA-256, `image0.jpeg#1a2b3c4d`. The lifted record carries it (`ref`), and
the note the model reads lists attachments by reference.
`open_attachment`, `import_attachment` and `save_attachment` take a name
or a reference (`attachment_ref::pick`): a reference finds its attachment,
and a bare name finds one only when no other attachment of the task shares
it. A shared bare name is refused with the references to choose from
(`save_attachment` answers it as a conflict). The tool specs say so in the
`name` description, a deliberate change to the pinned snapshot.

`save_attachment` saves under the attachment's own name, checked to be a
plain file name, whichever way it was asked for.
