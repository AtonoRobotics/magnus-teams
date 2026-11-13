# Team messages have the personal chat's message controls

The owner: "The teams chat window should have the same message controls as
the other chat window."

A team message now carries the personal chat's controls in the same shape:
Edit (one's own) or Copy and, on the agent's answers, Read aloud at hand; the
rest in the "More message actions" menu: Copy, Retry, Branch, Share, Export,
Save to Files, Make a task, Feedback and Delete. The team's own controls stay:
quick reactions, Reply in thread, and Pin (in the menu).

What each does in a workspace:
- Retry and Feedback are for who asked the agent. Retry asks the same request
  again in the same place; Feedback is the task verdict, which only the person
  a task is for gives.
- Branch opens a new private chat with the message quoted in the composer,
  not sent: the private agent is where private work goes.
- Share sends the message, quoted, to a member in a direct conversation.
- Export downloads the message as Markdown through a new read-only route,
  `GET .../messages/{id}/export`, which serves only what that person already
  sees in the channel view (an answer holding data only its requester may see
  stays withheld from others).
- Save to Files saves it into the workspace's files, which the whole team
  sees.
- An answer marked as holding data only the requester may see offers no Share
  and no Save to Files, so neither moves it to others.
- Guests are offered no Save to Files, Make a task or Pin, as before.
