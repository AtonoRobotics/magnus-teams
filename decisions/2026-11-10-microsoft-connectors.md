# Microsoft's connectors are brokered over Graph: Outlook, Outlook Calendar and OneDrive

**Choice.** Three catalog connectors, split as Google's are: `microsoft-outlook` (mail), `microsoft-calendar` and `microsoft-onedrive` (files). Each is brokered: an Alpha Vector server over Microsoft Graph (`brokered/outlook`, `brokered/outlook-calendar`, `brokered/onedrive`, sharing `brokered/microsoft`), its image named by digest, listed in `release/brokered-images`, reaching only `graph.microsoft.com`. People sign in through Alpha Vector's public client (`catalog/registrations.toml`, `microsoft`) at `https://login.microsoftonline.com/organizations/v2.0`, so anyone from any organisation can connect, and no secret is provisioned.

Tools and classes:
- Outlook: `list_messages`, `search_messages`, `get_message`, `list_drafts`, `get_draft` read; `create_draft`, `update_draft`, `reply_draft` reversible writes that reach only the person (`self`); `send_draft` irreversible, with who it reaches read from the draft at the gate (`[destinations.send_draft]`, as Gmail's), and a digest of the body, sender, reply-to and every attachment's bytes bound to the approval.
- Outlook Calendar: `list_calendars`, `list_events`, `get_event` read; `create_event`, `update_event`, `respond_to_event` irreversible, since Outlook tells the people invited and the organizer.
- OneDrive: `list_files`, `search_files`, `read_file` read; `upload_file` a reversible write (`self`): it makes a new file and never replaces one (`conflictBehavior=fail`), so deleting it undoes it.

No tool deletes, moves or shares anything.

Scopes, least privilege per Graph's permissions reference: `Mail.ReadWrite` and `Mail.Send`; `Calendars.ReadWrite`; `Files.ReadWrite` (the person's own OneDrive, not `Files.ReadWrite.All` or `Sites.*`). Each also asks `offline_access`, without which Microsoft gives no refresh token and the sign-in would end after an hour.

**Reason.** Microsoft's own MCP servers for mail, calendar and files (Work IQ, the Agent 365 tooling servers) are in preview. Each organisation's admin registers and consents to them through Agent 365, and they need Microsoft 365 Copilot. A public client cannot sign in to them from any organisation, so they are not a remote connector the catalog can ship set up. Graph is the API under them, and the brokered pattern is already built and reviewed for Google.

**The issuer.** Microsoft's multi-tenant metadata names its issuer `https://login.microsoftonline.com/{tenantid}/v2.0`, not the address asked. The kernel refused it as another issuer, so a Microsoft connector could not start a sign-in. `issuers::same` accepts that template, or one organisation's GUID in its place, only when the address asked is Microsoft's `organizations` or `common` authority on `login.microsoftonline.com`. Every other issuer is still compared exactly. This follows Microsoft's documented multi-tenant validation and widens nothing for any other provider.

**File bytes.** Graph's `/content` (v1.0) answers with a redirect to the organisation's own SharePoint host (`<tenant>-my.sharepoint.com`). That host differs for each organisation, so it cannot be one of the exact hosts a manifest names, and a wildcard would let the microVM reach every organisation's SharePoint. `read_file` therefore reads through Graph's `contentStream`, which answers with the bytes itself but is a beta endpoint. If Microsoft changes it, reads fail and say so, and nothing else breaks. A read is capped at 4 MiB, and so is an upload (Graph's simple upload; larger files need an upload session on another host).

**Left out of the first version.** Teams chat (`Chat.ReadWrite` needs admin consent in many tenants, and sending a chat message is a send to review apart), SharePoint sites and files shared by others (`Files.Read.All`, `Sites.Read.All`), and deleting, moving or filing mail. Outlook has no `[sent]` search yet: Graph's search cannot be limited to Sent Items through one input, so the gate treats recipients as it does for any app without one.

**Kit.** `magnus_brokered_kit::send` sends a body of any type and asks for any answer type. The OneDrive server needs it for a file's bytes. `exchange` is now built on it and sends the same bytes as before. The change rebuilt the Gmail, Google Calendar, Google Chat and Slack images, and their manifests name the new digests. Their behaviour is unchanged.
