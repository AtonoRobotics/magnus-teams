# Generated connectors

A connector Magnus builds is a declared set of HTTP calls to one address, not code. The kernel runs it, so nothing the agent wrote touches a token.

- Building one is an irreversible action, so a person approves it. A GET reads; any other method is irreversible. The agent's own claim about risk is not taken.
- It belongs to one person. It is listed with their connectors and no one else sees it, including the owner.
- It reuses the app path: the address is `generated:` plus the definition, and `Mcp` answers for it in process, so classification, pinning, boundary checks, grants and the audit trail are the ones every other connector gets.
- The token is the person's, set in the app and kept in the vault. The address is fixed at approval, DNS answers are pinned, private addresses are refused unless the appliance allows them, and redirects are not followed.
- The file at `/workspace/connectors/<name>.json` is the person's copy. The agent has root on the computer and could change it, so what runs is the definition approved in the log.
- Deleting one revokes its token and grants, removes its approval rules, and erases its file.
