# Defaults are not stored as choices

**Choice.** `theme.js` stores the look only when a person chooses something,
or when an old record is migrated. A record saved before the Theme choice
existed (one with no `style`) and an accent of `blue` takes today's default
accent. `blue` was the default then, written on first load, not chosen.

**Why.** `9a8b690` made Monochrome the default accent, but no existing app
or browser showed it. On first load, `Theme.scope` stored the whole set of
defaults as if the person had chosen them, so everyone already had
`accent: "blue"` saved and kept Indigo. Any later change of default would
have failed the same way. Someone who picks Indigo now has it stored with a
style, and keeps it.
