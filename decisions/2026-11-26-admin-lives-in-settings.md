# Admin lives in Settings, with no entry in the navigation

## Decision
The navigation has no Admin entry in any style: no shield on the rail and
no "Workspace" group in the sidebar.

Admin is in Settings, where it already was:
- `#/admin/...` opens the Settings window on the matching Admin section
  (2026-11-20-unified-settings-modal-and-reasoning-level.md).
- The stop banner's "Open Admin" link still goes there.
- The kill switch stays in an admin's top bar.

With it the sidebar's "Workspace" group is gone in every style, since
Admin was all it held. In Nova the sidebar also has no Trash row, as in the
mockup, whose sidebar ends with its Projects and Recent lists. Trash is a
page that Quick Jump (⌘K) opens, and other styles keep their row.

The browser checks that reached Admin through the entry go to `#/admin`.

## Reason
The owner asked on 2026-11-26 for the entry to be removed, said it was
meant to go long ago, and called it redundant with Settings. Since
2026-11-20 it only opened Settings, so it was a second door to the same
place on the rail. The mockup has none.
