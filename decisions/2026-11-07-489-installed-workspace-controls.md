# Installed workspace controls (#489, #475, #477)

- Carry the complete shared-controls feature onto an independent master-based branch to replace the failed #473. Keep the existing action APIs, permission effects, feedback values and confirmations.
- End entry entrance transforms before opening viewport-positioned menus, clean them after animation, and dismiss menus on actual anchor movement rather than delayed scroll notifications for an unchanged anchor. Preserve normal focus and real pointer activation.
- Keep composed frames script-disabled. Native WebKit does not execute the parent-installed frame event handlers, so generated Copy code and prose-link affordances live in the trusted parent, positioned against inert markers in the reading surface. A resize observer updates frame height and action geometry. No model-authored interactive HTML is admitted.
- Preserve the authenticated file response content type through the native byte bridge. Canonical local Markdown image paths work with HTTP, Tauri and opaque origins; no new remote URL or frame permission is allowed.
- Reserve read-aloud ownership and expose Stop before asynchronous host/script startup. Only the current generation can clear playback state. Native startup and stop serialize; stop waits for the owned process to terminate. System fallback reads bounded pieces and reports process failures.
- Installed-host tests keep genuine WebDriver clicks, uploads, image decoding, clipboard reads and playback. Scripted model responses provide fixed UI fixtures only; they do not establish successful agent reasoning.
