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

Layout (owner, after the first version: "team chat is unchanged… The ui per
post should match"): the controls sit in a row of icons under each post, as
the personal chat's do, always shown. React (a menu of the quick reactions)
and Reply in thread join that row as icons; the hover-only floating toolbar
of text buttons is gone, because it hid the new controls.

Review follow-up (same day): a work card with no text still shows React and
Reply. Picking a reaction from the menu closes the menu and gives focus back
to the button that opened it. After a redraw, focus goes back to the same
post, not to the first post that has the same control. The reactions menu
is a row, so Left and Right move along it (`aria-orientation="horizontal"`,
which Menus now honours). The two icons now live in the shared ICONS set.
