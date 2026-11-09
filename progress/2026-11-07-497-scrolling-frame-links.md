# Scrolling frame links (#497)
The pre-fix regression exposed page-wide overflow and stale parent link hit targets after horizontal table scrolling. Parent actions now stay clipped to the reading surface and relevant scrollports; keyboard focus reveals the corresponding marker. Native WebKit additionally required a visibility-gated scroll-offset observer because child scroll events were not delivered to the parent.

Validation against the real appliance and final source:
- Browser frame-link regression passed (2.32s): an actual uploaded wide Markdown table, both scroll directions, clipped/aligned hit targets, no outer overflow, real Tab navigation and unchanged script-disabled sandbox.
- Shared controls/Markdown passed 2/2 (7.93s); chat-results also passed with column width and cached-frame geometry intact.
- Installed Linux/WebKit frame-link test passed (6.56s), including real WebDriver scrolling/Tab/clicks and both selected links reaching the actual external Chromium browser, recorded as HTTP requests by the destination server.
- Installed composed-flow regression passed (25.50s): actual uploaded image, real clipboard copy, streaming, hostile-content/CSP and no-network checks.

Full CI remains required before merging. This focused review correction does not reduce the complete v1.2 implementation and acceptance scope.
