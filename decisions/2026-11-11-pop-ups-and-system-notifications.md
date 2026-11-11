# Pop-ups in the lower corner, and the system's own notifications

The owner asked: "Users should have a popup notification in the lower corner if they are messaged or something needs their attention. Desktop should show the notifications from magnus in it's notification service if possible."

What the spec says, and what it leaves open:

- "Notifications are set per channel to all, mentions or off. An activity view gathers mentions, replies and asks." (Workspace experience › Messaging.) So each channel's setting decides what is told.
- "Background mechanics never reach them as tasks, approvals or notifications unless they need attention" and "Magnus messages the user only when something changed, a result is ready, or a decision is needed." So Magnus's own mechanics never pop up; what pops up is what already reaches the person: a message, an answer, an item in Needs you.
- "No animations, no notification pop-ups" is about the agent's computer's desktop (Calm for the agent too), not the app.
- The spec says nothing of what a notification may carry. Choices below follow common practice and the safer reading.

## What pops up, and when

One look every 3 seconds, the app's existing refresh pace, at a new route, `GET /v1/team/notices?after=<the last look's now>`, across every workspace the person is in, not only the one open. Before, a workspace told the system of new messages only while it was open on screen. The route answers, by each channel's setting: a message in a direct conversation (kind `direct`), a mention (`mention`), any message in a channel set to all (`message`), and the team agent's finished answer to the person's own request (`answer`, "a result is ready"). Never the person's own messages, nothing in a channel set to off, nothing they have read, nothing they deleted for themselves, nothing deleted by its sender. The first look of a session answers only the time, so signing in never replays old news. At most 20 per look.

Items that need the person (Needs you) pop up too, by the existing per-person choices in Settings › Notifications (a task needs me now; something can wait for me; the agent changes), which already decided what was sent to the system.

## The pop-up

- Lower right corner, over the page, at most 3 at once (the oldest gives way), each gone after 8 seconds unless the pointer or focus is on it, with its own Dismiss button. Each is `role="status"`, so a screen reader reads it without moving focus. It slides in briefly, and not at all under `prefers-reduced-motion`.
- It says who and where, and the start of the message (its first line, 140 characters), masked as the conversation masks it. Clicking it opens the conversation, at the thread when it is in one, or Needs you.
- Nothing pops up for the conversation the person is looking at with the app in front: they are reading it.

## The system's notifications

- **When**: only while the app is not in front (its window is hidden or not focused). In front, the pop-up is enough; two of the same at once is noise.
- **What**: who and where, never what a message says: "Dana — sent you a message", "Dana mentioned you — in #launch, Alpha". A system notification shows on a locked screen and stays in the system's list after the app closes, where the person's workspace content would be read by whoever is at the computer. This is the safer of the two readings the spec leaves open. Needs-you items keep the summary they already carried.
- **Desktop app**: the Tauri notification plugin, already in the app with `notification:default`, shows them through the operating system's notification service. A click on one is the system's to handle: the plugin has no click event on Linux or Windows.
- **Browser**: the Web Notifications API, and only once the person allowed it. The browser is asked only from their own click: the first pop-up in a browser that has not been asked offers "Show these when Magnus is in the background", and Settings › Notifications has the same switch. Before, the page asked the browser by itself, at the first notification, which browsers now block or penalise. A click on a browser notification brings the page to front and opens the item.
- **A switch per device**: Settings › Notifications › On this device turns the system's notifications off or on for this computer only, kept in this device's storage, because whether a computer should show them is the computer's question, not the account's. The per-person choices of what to tell stay on the server, as before.
