# The Windows window starts solid

The Windows app opened to a black window. The window is created transparent so the system's glass (Mica, Acrylic) can show behind the chrome, and WebView2 draws such a window black wherever the page is not painted. The page's background came only from theme.js, so it was black until the scripts ran, and black for good on a computer that cannot composite a transparent webview (remote desktop, some virtual machines and graphics drivers).

Choice: the webview starts solid in the system's light or dark color, and turns clear only when `set_material` reports the system's material is behind it; it goes solid again when glass is taken off or fails. The page's `html` background also has a solid fallback in both modes before theme.js sets its tokens. Glass stays as it was for anyone whose computer applies it.

Checks: `look::tests::the_webview_is_clear_only_over_the_system_material` and `desktop/tests/solid-start-check.mjs` (fast tier). The Windows build type-checks; it has not yet been opened on a Windows computer.
