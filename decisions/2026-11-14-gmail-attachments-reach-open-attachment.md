# Gmail attachments reach open_attachment, and pictures can be looked at

**What happened.** A run asked to make a flier from six JPEGs in an email. Gmail's tools gave only the file names, `open_attachment` answered "this task has no attachment from outside called image0.jpeg", and the run stopped. The kernel lifts an attachment out of a mail result only when the result carries its bytes as `data_b64`. No mail connector ever sent them, so the path from 2026-11-08 ("Attachments from outside") had never been reachable from mail.

**Choices.**
- Gmail gets `get_attachment` (message id and file name, class `read`). It returns the bytes as `data_b64`, so the existing lift keeps them out of the model's text and `open_attachment` finds them. It looks a part up by file name, not attachment id, because Gmail's ids change from one read to the next. The message's `attachments` list is unchanged: send_draft's read-back compares it with the sent message, and an id in it would never match.
- The login-code filter and secret redaction now run after the lift, never on the bytes. Run over base64, the filter found "codes" (4 to 10 characters, half of them digits, on a line that says "pin" or "otp") and damaged the file. A photo's base64 almost always holds such runs.
- `open_attachment`'s default extractor turns a picture into a smaller JPEG: the first of 1024, 768 or 512 px whose base64 fits under the guest's 200,000-byte output limit. The JPEG goes to the model as `image_b64`. The tool is labelled untrusted, so the inbound screen checks the picture as it checks screenshots. A picture's media type is read from its first bytes (`/9j/` is JPEG); screenshots stay PNG.
- The file itself still stays out of the run's workspace. Only the person keeps it, with `POST /v1/tasks/{task}/attachments/{name}/save`, as before.

**Not here yet.** Outlook lists attachments without their bytes and needs the same tool. The desktop has no Save button for an attachment from outside, so a run that has to use the file itself, such as putting a photo in a flier, still needs the person to upload it.
