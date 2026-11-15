# Getting started: one page, three steps, nothing new to fill in

The owner asked for onboarding (2026-11-15): a step-by-step guide to connect
Slack, Microsoft, Google and GitHub, set up team chat and schedule a first
automated workflow, that is intuitive and gives clear feedback.

## The choice

A **Get started** page (`#/start`, `desktop/ui/onboarding.js`) with three
steps the person can take in any order:

1. **Connect your apps.** Google, Microsoft, Slack and GitHub, each with its
   apps from the catalog (`google-*`, `microsoft-*`, `slack`, `github`).
   Every app's row says where it stands and offers the one thing to do next.
   - **Connect:** the same consent screen and sign-in as Apps.
   - **Signing in:** where to finish, and *Open it again*.
   - **Connected:** as which account, and a link to the conversation Magnus
     opened about the app.
   - **Sign in again:** when a sign-in stopped working.
   - **Not set up on this server yet:** when Alpha Vector has not set up
     the provider here.

   One connected app completes the step.
2. **Set up team chat.** A Team Admin types a name and the workspace exists,
   with the team agent in every channel (spec, Workspace experience).
   Anyone else pastes the invite link they were sent. Once someone is in a
   workspace they administer, the guide offers two things:
   - inviting people (the workspace's own invite form);
   - choosing how the agent takes part in the first channel (the channel's
     own setting).
3. **Schedule your first workflow.** The person says what and when in their
   own words. Suggestions follow the apps they connected. A suggestion or
   their own sentence goes into a new conversation, where they can change it
   and send it. Magnus proposes the schedule there and they confirm it. The
   step is done when a schedule exists.

## Why

- **Nothing new to fill in.** Spec, Workspace experience, says nothing is
  set up through a step-by-step form. So the guide orders actions the app
  already has; it does not add a wizard. Each form is one field and one
  button, and `onboarding-check.mjs` holds it there.
- **The server decides when a step is done.** Each step's status is read
  from the server every time: connections, workspaces and schedules. The
  guide stores nothing that could disagree with the server, and work done
  elsewhere completes its step.
- **Magnus makes the schedule.** The first workflow is a request to Magnus,
  not a schedule form. The model makes the schedule, as with every other
  schedule (spec, Tasks). Without a form there is no second way to make a
  schedule, and no form to keep in step with what Magnus can schedule.
- **Clear feedback.** A progress bar with an "n of 3 done" count. The next
  step is marked. Each step's result or failure is said inside the step:
  `role=status`, or `role=alert` for a failure. Buttons say what is under
  way ("Creating…").

## When it shows

- **Opening by itself:** once per device, at a newcomer's first sign-in. A
  newcomer has no task, and no conversation except the ones Magnus opened.
  People who already work here are not taken to it.
- **In the navigation:** under New conversation, with its count, until every
  step is done or the person hides it.
- **Afterwards:** Settings, Account opens it again.
- **Where the hidden and opened marks live:** on the device, like the look
  (`magnus.guide.*` per server and person). The steps themselves are always
  the server's.

## Not done

There is no server-side onboarding state and no new API. Hiding the guide
on one device does not hide it on another. If that matters, it becomes a
preference on the server.
