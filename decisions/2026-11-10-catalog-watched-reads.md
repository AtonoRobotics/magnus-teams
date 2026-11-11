# Catalog apps declare their watched reads, and a connected app is polled

The owner connected Gmail, Drive, Calendar, Chat and Cloudflare on the teams server and no notification ever reached Magnus: there has never been a poll or push event. Polling (#406) and push (2026-11-09-app-push.md) act only on an app's watched reads, and no catalog manifest declared one: `catalog::connector` set `watch` empty. Two more gaps sat behind it. The answer of a watched read was diffed only when the app was in the config file (`cfg.connectors`), never for a catalog or owner-added app. And a poll called its read with no arguments, which for most reads lists the wrong thing (Calendar's oldest events, Slack's search refuses).

**A manifest declares its watches** (`[[watch]]`, the same `Watch` as a config app's), and the connector made from it carries them. A release brings new watches to every server: the catalog is compiled in and each app is rebuilt from it on every use, so a connected app picks them up without reconnecting. A watched answer is diffed against the watches of the app the call went through, whatever list it came from.

**A watch names its read's arguments** (`input`). A string `{now}`, `{now-1d}`, `{now+6h}` becomes that time when the poll runs. The polling run is told the arguments exactly. Reason: what the person would want to be woken by is almost never what the read lists by default.

**An id may join fields** (`event_id+changed`, `thread_id+snippet`). An item counts as new when any part changes, so a changed event or a thread with a new reply wakes like a new one.

**A watch is checked** (`watch_reads::fault`, in each manifest's well-formedness test): a read the app classes, watched once (its poll clock and seen items are kept by tool), a JSON pointer to its items, an id, an interval of at least a minute, and arguments that are an object whose time placeholders read. A catalog watch points into the answer, since an app's answer is an object. A watched answer with no array where the watch points is logged as a mismatch instead of reset to nothing, which would wake on everything at the next answer.

**Intervals**: five minutes for mail and chat (Gmail, Outlook, Google Chat, Slack), where a person expects to hear soon. Fifteen for the rest (calendars, files, GitHub, Asana, Vercel), where changes are slower and each poll is a model run.

| App | Read | Arguments | Items, id |
|---|---|---|---|
| Gmail | search_threads | `in:inbox newer_than:2d` | /threads, thread_id+snippet |
| Google Calendar | list_events | from now, changed in the last day | /events, event_id+changed |
| Google Drive | list_recent_files | by last change, 20 | /files, id |
| Google Chat | search_conversations | none | /spaces, space_id+last_active |
| Outlook | list_messages | inbox, 25 | /messages, message_id |
| Outlook Calendar | list_events | changed in the last day | /events, event_id+changed |
| OneDrive | list_files | the top folder | /items, item_id+modified |
| Slack | search_messages | `to:me`, newest first | /messages, channel_id+ts |
| GitHub | search_issues, search_pull_requests | open and involving the person; open and asking for their review | /items, id+updated_at |
| Asana | get_my_tasks | incomplete | /data, gid |
| Vercel | list_deployments | failed | /deployments, uid |

**Brokered servers changed to make these reads possible.** Their images were rebuilt and their digests updated. Calendar's list_events takes `changed_since` (Google's `updatedMin`) and each event says when it `changed`. Outlook Calendar's list_events takes `changed_since` in place of two times: Graph's calendar view cannot filter by change, so it lists events and series by `lastModifiedDateTime`. Chat's spaces say when each was `last_active`. Slack's search sorts `newest_first` on request. The test server gained `list`, for the end-to-end poll test.

**Not watched.** Sentry: its server answers in prose, not JSON, so there is no item to diff. GitLab: the shape of its todo list's answer is unknown, and a guess would wake on nothing or everything. Shopify is held (not shipped). Cloudflare, Supabase and Hugging Face have no read of something new happening to the person.

**Unverified.** The answers of remote servers (Drive, GitHub, Asana, Vercel) are shaped as each server's code and the provider's API give them. None has been seen from a live poll yet. A mismatch is logged at the first poll; correct the watch from that log. Slack's `to:me` is Slack's modifier for messages sent to the person. Mentions in channels are not covered, since Slack's search has no modifier for the person's own mentions.
