# 2026-11-27: Shared Project Instructions & Streamlined Promotion (Phase 3)

## Choice
1. **Shared project instructions (`PUT /v1/workspaces/{ws}/instructions`, `Change::Instructions`)**:
   - Added `instructions` / `instructions_by` to `Workspace`, stored via additive `Change::Instructions { workspace, text, by }` (using content-store field `text` so erasure removes it automatically), validated against `MAX_INSTRUCTIONS_CHARS = 8_000` and `scrub_check`.
   - Exposed `instructions` on `GET /v1/workspaces/{ws}/home` and rendered the same collapsible `Instructions` card on the Shared Project Home (`desktop/ui/projects.js`).
   - Injected shared project `instructions` (marking `untrusted: true` when written by someone other than the asker) and visible shared document names into direct personal-agent room replies (`crates/magnus/src/team/reply.rs`).
2. **Streamlined clean-room "Add people" promotion (`desktop/ui/shared-canvas.js`)**:
   - Promoting a personal project (`Add people` on `#/project/:id`) now defaults to creating a new shared project with the same name, lets the user pick which project documents and instructions to copy across the clean-room boundary, skips the extra channel-name/summary form, and lands directly on the unified Shared Project Home (`#/team/:ws`).

## Reason
Shared projects now have full feature parity with private projects (instructions, documents, chats, and personal-agent replies) while keeping the strict clean-room privacy boundary intact when adding people to a private project.
