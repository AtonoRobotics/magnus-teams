# Team workspace and computer: mockups

Design references for the desktop app, made 2026-11-09. The live canvas is
https://claude.ai/artifact/ALYy7H52hVKVWefQ2ZgBtP. These files are copies of
its artboards, kept here so the design and the spec change together. They are
not app code: the app stays plain JavaScript with no build step.

- `team-workspace.dc.html`: the three-pane workspace. The team agent takes part
  without a button, shows live progress with the requester named, and posts
  results as rich content. A private agent's whisper, the per-channel setting
  for how Magnus takes part, and the project panel with the team computer,
  agents at work, tasks and files.
- `computer-panel.dc.html`: the computer beside a conversation, in its four
  states: asleep, waking, the agent working, and the person in control.
  Taking control happens in place, and the screen never disappears.
- `create-without-forms.dc.html`: creating a project from the command menu and
  inviting people inline, with no step-by-step form.

The spec sections they illustrate are "Workspace experience" and "User takeover
and handback" in the Magnus Agent Computer spec. Where they differ, the spec wins.
