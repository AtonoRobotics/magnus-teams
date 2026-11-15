# Files from outside reach the agent's work, rebuilt clean, with no person asked

**What happened.** A run asked to make a flier from six JPEGs in an email. Gmail's tools gave only the file names, `open_attachment` answered "this task has no attachment from outside called image0.jpeg", and the run stopped. The kernel lifts an attachment out of a mail result only when the result carries its bytes as `data_b64`. No mail connector ever sent them, so the path from 2026-11-08 ("Attachments from outside") had never been reachable from mail. Even once reachable, a picture opened only as its file type, and the file could reach the work only through the person's save.

**The owner's rule (2026-11-14).** "The attachments and emails must be handled securely and autonomously. This is pointless if HIL has to get involved with the most basic functions." And: "All file types need a secure and safe solution for being downloaded and used by an agent."

**Choices.**
- Gmail gets `get_attachment` (message id and file name, class `read`). It returns the bytes as `data_b64`, so the existing lift keeps them out of the model's text and `open_attachment` finds them. It looks a part up by file name, not attachment id, because Gmail's ids change from one read to the next. The message's `attachments` list is unchanged: send_draft's read-back compares it with the sent message, and an id in it would never match.
- The login-code filter and secret redaction now run after the lift, never on the bytes. Run over base64, the filter found "codes" (4 to 10 characters, half of them digits, on a line that says "pin" or "otp") and damaged the file. A photo's base64 almost always holds such runs.
- `open_attachment` shows a picture to the model: the default extractor makes a smaller JPEG, the first of 1024, 768 or 512 px whose base64 fits under the guest's 200,000-byte output limit, sent as `image_b64`. The tool is labelled untrusted, so the inbound screen checks the picture as it checks screenshots. A picture's media type is read from its first bytes (`/9j/` is JPEG); screenshots stay PNG.
- `import_attachment` puts a file from outside into the run's workspace with no person asked, by content disarm and reconstruction. `attachment_rebuild.py` runs in the attachment's own sandbox (no network, nothing of the person's) and rebuilds each kind from its content alone:
  - a picture from its pixels (PNG where it may be transparent, else JPEG), at most 4096 px a side;
  - a PDF re-rendered from its pages by `pdftocairo` (no JavaScript, forms or embedded files);
  - a Word, Excel, PowerPoint, RTF or OpenDocument file re-saved by LibreOffice in the macro-free format of its family (`.docm` becomes `.docx`);
  - text, data and code as clean UTF-8, with `.txt` after anything a system runs by its name (a shebang, `.sh`, `.bat`, `.ps1`, `.js`, `.py` and the like);
  - a zip, tar, gzip, bzip2 or xz archive unpacked there with no links, no paths out (`..` and absolute parts dropped), at most 500 files, 512 MiB and three levels deep, each member rebuilt by these same rules.
  
  Only the rebuilt copies are read back. The kernel checks each path again (plain parts only) and writes it under `/workspace/from_outside`, and no file is executable. The original never enters the workspace. Invariant 2's attachment test still holds: opening does not import, and the raw file reaches the person's computer only through their save.
- A program (ELF, PE, Mach-O, MSI, JAR, APK, disk images and the like) never comes in. It is said to be left out, and `open_attachment` still reads it. No agent task needs to run a program sent by a stranger.
- Kinds with no rebuild in the sandbox image come in unchanged, never executable, with the run tainted untrusted: audio, video, fonts and other data. A web download already reaches the workspace this way. A 7z or RAR archive is left out, since the image has no unpacker for it.

**Not here yet.** The sandbox image has no ffmpeg, so audio and video are not re-encoded, and no 7z. Adding both to `sandbox/Dockerfile` would close those two gaps. Outlook lists attachments without their bytes and needs `get_attachment` too. The desktop still has no Save button for keeping the original.
