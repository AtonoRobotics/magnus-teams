# A team workspace fills the window, and an admin gives its agent a computer from the panel

The owner found the workspace did not match the spec's layout: the app's own navigation (with its personal projects and conversations) and its top bar sat beside the workspace's rail, so there were two left rails. The spec has one: projects, channels, direct messages and agents.

- In a workspace the app's navigation folds to icons, as it already does when work opens beside a conversation, and the app's top bar goes. The person can still open the navigation; on a phone the drawer is unchanged.
- The right panel no longer says "No team computer is running" when the workspace has none. It says the team agent has no computer yet and what that means, and offers an admin "Give Magnus a computer" (the existing `POST /v1/computers` with the workspace). Anyone else is told an admin can. A computer that sleeps shows the same caption as the computer's own page.
- A work card's title is the ask without the mention that made it, and a step with no tool yet reads "Thinking it through".
- The knowledge tab shows each entry's type, source and its weight, author, owner, last review, relations and flags, as the spec lists them; only the project's owner and curators are offered to correct or retire.

Reason: the spec's "Workspace experience" and "Project knowledge base", and the mockup in docs/design/team-and-computer/, where it agrees with the spec.
