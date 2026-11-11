# An admin's schedule list carries no words; Overview names every stop; the workspace form shows only to those allowed

Spec rev 141, Administration (People, Overview) and the Workspace experience ("created by typing its name").

## Schedules an admin sees

"Every admin read of a person's data is logged, and the person is told." The list an admin opens to suspend or resume someone's schedules (`GET /v1/admin/people/{id}/schedules`, and the console's control route under it) returned each schedule's text as its title, with no record and no notice. It now returns no words: each schedule's id, rule, next and last run, run and failure counts and whether it is paused, which is all suspending one needs. What a schedule does is the person's data, read only through the recorded data read (`POST /v1/admin/people/{id}/data/tasks`). Returning no content was chosen over recording each list read because the list is opened to act, not to read, and a notice "Ana read your schedules" each time an admin looked for the pause button would teach people to ignore the notice that matters.

The other admin reads were checked: people rows and sessions are account records, Agents lists runs by kind with no words, activity is already a recorded read, spend and developer-access sessions carry no content.

## Overview: an agent stopped

Every kill switch on below the server's (one person's agents, one agent, one connector) and every paused agent is an Overview alert naming who did it and when, fixed in Agents. The answer also carries them as `stops`.

## Starting a workspace

The spec's Team section does not let members create workspaces, and the server already refused anyone but an owner or admin. The Teams page now offers the name field only to those it would accept, and tells everyone else an owner or admin starts one. The server's check now uses the same rule as the Admin entry (`admin_of`: the config's owner or admin role, or the directory's admin role), so a directory admin who sees the field is not refused.
