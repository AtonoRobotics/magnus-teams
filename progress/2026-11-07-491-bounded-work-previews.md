# Bounded, stable work previews (#491, #494)

Controls CI 42439695729 passed 709/710 unit/browser tests and failed the actual chat-result width assertion (392.56px frame in a 536px column). This replacement retains the complete feature and corrects wrapper-aware width, blank-frame height-cache writes, Markdown preview row caps and parent copy tooltip dismissal. Absolute external Markdown URLs also work on opaque native origins without permitting unsafe schemes.

Validation:
- A 1,000-row Markdown file preview failed the old renderer (1,001 table rows including the header), and now renders 200 body rows plus the truthful remainder notice.
- Exact `chat_results` browser regression passes full column width, redraw height stability, no repeated redraw, timeline/table contents and unobscured conversation regions. The local test exposed and verified correction of a 387px → 158px initial-blank-frame collapse.
- Shared controls/browser suite: 2/2 passed, including an actual uploaded bounded preview, parent Copy code tooltip dismissal, actual clipboard content and unchanged original file bytes.
- Real installed Linux/WebKit composed-flow check passed: streaming, actual image upload/native decoding, absolute prose link rendering, actual parent Copy code/native OS clipboard, and hostile-content/CSP/no-network checks.
- Existing native speech and all-message-action evidence from #489 remains applicable; those paths did not change here. Full CI, including the KVM-dependent workflow and all host tests, remains required.

The full baseline is still tracked through layout/status/continuity integration and final acceptance; this PR does not redefine the user's complete outcome.
