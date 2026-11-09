# Website sign-in, in the order the spec gives it

A sign-in tries the vault, then a connected password manager, then a secure form the run waits on. The password manager is an app the kernel calls itself. Its tools are classed prohibited whatever the catalog says, so the agent is neither offered them nor able to call them, and the login goes only to the browser VM.

Reason: a password manager the agent could call would hand it every login the person has.

A profile is sealed with a nonce derived from its key, chunk index and content. The earlier index-only nonce reused a keystream every time the same profile was sealed again under the same key, which leaks the difference between versions. An unchanged chunk still seals to the same bytes.

An open profile is never backed up. It holds live sessions, and the backup key is on the same disk. Backups take only the sealed file.

A real credential a site echoes back is replaced with its surrogate before it reaches the computer, so the proxy drops compression on those responses.

Mail attachments leave the tool result before the model sees it. The model is told a file is there and reads it only through `open_attachment`, in its own sandbox, as data. This applies to apps with login-code filtering on, which are the mail apps.

A screenshot is refused while the page visibly shows a value filled in securely. Text can be masked, a picture cannot, and reading the value out of the picture is not something the spec asks for.
