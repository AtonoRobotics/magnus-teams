# Continuity recovery and deliberate scrolling (#482, #492)

- Retain the complete #463 continuity feature on an independent master-based replacement for #479. Theme scoping and draft lifecycle both survive the actual cherry-pick conflict; neither side is discarded.
- Treat the journal as the source for a mounted composer after an asynchronous transition. Input events synchronously update it; subscriptions synchronize recovered text/selection into the current view, including rejection callbacks owned by an older view.
- Preserve a recovered secondary record's navigation identity while acknowledgement leaves independent text, attachments or pending submissions. Do not merge that content into the original thread draft or clear it because another snapshot was accepted.
- Suppress only the scroll offset produced by anchor restoration. A deliberate scroll arriving during the same frame must become the new anchor; a blanket animation-frame suppression loses that intent. Keep resize/stream anchoring and the 80px following boundary.
- Native checks use real WebDriver clicks and actual stored messages/tasks, including a review accepted under Needs you. Fixed model responses test UI behavior, not reasoning. The exact KVM-dependent failed release workflow remains a required CI check.
