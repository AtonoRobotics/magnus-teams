# One place per style

**Choice.** `desktop/ui/app.css` is replaced by one sheet per area of the app,
loaded in a fixed order: `base`, `layout`, `chat`, `pages`, then the existing
`teams`, `computer`, `admin` and `onboarding`. A property is set for a
selector in one rule per context. Nothing styles a class that no code uses,
and every token a sheet reads exists. `desktop/tests/stylesheet-check.mjs`
checks all of this in the fast tier, and `docs/ui-styles.md` says where each
style lives.

**Why.** The two UI commits before this one (`c5bb8af`, `cd56664`) changed
values that never showed. The same selectors were set again further down
`app.css`, and a later rule wins a tie, so 18 new declarations had no
effect. The composer's new border used `--line2`, a token nothing defines,
which drops the whole declaration: the composer had no border at all. One
1,336-line file with rules for one component in several places made this
easy to do and hard to see.

**How it was done.** First a restructure that changes nothing on screen.
The rules were split by area, duplicated selectors merged, overridden
declarations deleted, and the rules for 16 unused classes deleted. A
computed-style comparison of every element on 60 views, at two widths and
in both modes, matched main's stylesheets. Then the lost edits were applied
on purpose:

- The composer has its border again, in `--line-strong`, the input-boundary
  colour.
- A result's and a request's left rule show again, in `--line`, at 2px with
  an 8px indent.
- Sidebar conversation rows are 28px with a mouse, and stay 44px on touch.
- The workspace group heading takes the newer padding.
- Message action buttons take the newer radius and easing.

**What stays.** Some overriding rules were accessibility floors, not
leftovers. Small links stay at 13px, every control keeps its focus ring,
inputs keep a visible boundary, touch targets stay at 44px, and
high-contrast mode keeps system colours. `docs/ui-styles.md` lists these
floors.
