# Keep the composer in the conversation column

The user reported an unchanged workspace and malformed input in Windows 0.7.1-test.1014. That published build maps to f362ea0, before workspace #483; later master gates failed and therefore did not publish its complete layout. Track actual installed build and publication separately from implementation. Zero's native window is at 192 DPI (200% display scaling). Its native runtime verification is pending because the host went offline during read-only inspection.

A real current-renderer regression also reproduced a concrete composer defect: at 1440px, the composer was 244px wide while the conversation column was 696px. Pane-relative padding was applied inside an already capped 760px composer wrapper. Give that wrapper fixed 16px inner gutters and a 728px cap, retaining the existing 696px reading measure. Align it with the thread at every width.

Use explicit single-column composer rows for the editor, recovery content, draft status and controls. Omit the empty submitted container from layout, allow draft status to wrap, and keep attachment/send controls inside the box with the model at the start and Send at the end. Preserve all existing DOM actions, drafts, uploads, selection, IME and send semantics. No fonts were reduced and no framework, permission or transport changed.
