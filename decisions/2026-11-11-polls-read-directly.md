# A poll is the kernel's own read, not a model run

On the teams server the first catalog polls ran as model runs (#406's design: a hidden task told "call this read once with these arguments"). The model looked the tool up and filled every field of its schema with an empty value. Calendar's read went out with `max_results: 0` and Google refused it. Drive's went out with a snippet setting its own description forbids beside `excludeContentSnippets`. Two runs ended with "Your reply called no tool". A poll needs no judgment: the read and its arguments are fixed by the watch.

**The kernel calls the watched read itself** (`watch_reads::poll_read`): once, with exactly the watch's arguments, through the person's own sign-in, over the app's transport. No model and no task. The answer goes only to the diff, which keeps ids. A review it wakes reads the items itself, through the boundary as before. Nothing an app returns to a poll reaches a model, so a poll needs no taint, contract or budget.

**What still holds a poll back** is what holds any read back: the tool is a read the app classes, the person has the right to the app, their sign-in is current and they are not turned off (checked by the sweep, #418), and the tool's definition passed its screen (`check_pin`). The organisation's task policies judge a task. A poll is none, as the gate's sent-mail search (#210) is none.

**Retry**: a failed read is read again once after `POLL_RETRY_WAIT` (#419's headroom, now in the kernel). Then the poll waits for its next interval, logging the failure. The start mark still restarts the clock, so a failing app is not hammered every sweep.

**Tests**: the poll scratch tests count reads at the app instead of poll runs at the model, and assert no model is asked to poll.

**Found on the server, not fixed here** (the owner's accounts):
- Google Chat answers "Google Chat is turned off. To use Chat API, turn on Google Chat" for the connected account.
- Google Drive's server has answered every call since it was connected with "The caller does not have permission", on the first call on 2026-11-10 too. The likely cause is the Google Cloud project behind Alpha Vector's Google client, which may not have the Drive MCP service enabled. A missing scope is the other possibility.
