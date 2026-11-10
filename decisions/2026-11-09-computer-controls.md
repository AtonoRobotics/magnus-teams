# Pause, reset and delete a computer from the app

**The controls.** A computer's page has Start (Resume when paused), Put to sleep, Pause now, Reset and Delete. They map onto what the spec names: the kill switch pauses or destroys a computer and revokes all its grants in one step; a reset returns it to the base image and the person chooses whether their volume survives; deleting it removes everything held for it.

**Pause now is the kill switch.** It is `POST /v1/computers/{id}/kill` with `mode: pause`: the VM is frozen and every grant is revoked in the same log write. It asks no question, because the kill switch has to be fast; it loses nothing and Resume undoes the freeze.

**Reset is the kill switch's destroy.** `POST /v1/computers/{id}/reset {keep_files}`. Keeping the files is a destroy without the volume: the VM, overlay and snapshot go, grants are revoked, and the app manifest replays on the next start, which is the spec's "returns to the base image". Erasing the files deletes the computer and creates an empty one for the same owner, so the person still has a computer; the new id is returned. There is no separate "destroy and keep the files" button: it would be the same operation under a second name.

**Delete is a destroy with the volume.** It uses the existing kill route with `delete_volume: true`, which only the person the computer belongs to may ask for.

**What deletion removes.** The vault entries scoped to the computer (its proxy CA, `computer-ca-{id}`, and the real credentials behind its surrogates, `surrogate/{id}/…`) are removed right after the log forgets the computer, before computerd is asked, so a failed VM step cannot leave them behind. Computerd now removes the computer's backups with its volume, since they hold the same files. At start the kernel sweeps computer-scoped vault entries whose computer the log does not know, which covers a kill cut short and computers deleted before this. Identity deletion also drops its computer-scoped entries. Vault entries scoped to a person or a workspace are untouched: the spec puts the vault's scope there, not on a computer.

**Confirmation.** Reset and Delete each ask first, and say plainly what is lost: running programs and changes outside /workspace for a reset, and for an erase or a delete the files, apps, snapshots, backups and the passwords and tokens the vault keeps for that computer.

**Not tested here.** The buttons were checked for syntax only; the UI has no automated test. The kernel's routes are covered by unit tests of the vault cleanup and computerd's test of what a delete removes; there is no kernel test against a live computerd.
