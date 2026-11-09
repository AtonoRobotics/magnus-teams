# Theme preference transition review fixes

Fixes the two review findings from #471: choosing an accent on a named Dark preset no longer switches to stale system mode; disabling Follow computer after an OS mode change freezes the visible mode. Added 40 named/custom × OS × follow transition assertions plus real Theme.set API regressions. `node desktop/tests/theme-check.mjs` passes, including the existing contrast/migration suite. CI and review pending; this does not establish rendered/native host acceptance.
