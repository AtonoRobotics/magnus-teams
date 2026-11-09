# A network change shown as Chrome's error page is loaded again (2026-11-06)

- `goto` watches the page's own main-frame request and loads again when that request failed with `net::ERR_NETWORK_CHANGED`, however Playwright reports the load. When Chromium answers the abort with its error page, Playwright reports "interrupted by another navigation to chrome-error://chromewebdata/" and never names the error. This failed #217's and #222's runs. Test: `a_network_change_shown_as_chromes_error_page_is_loaded_again`.
- Any other failure is still reported at once, and its message now carries the request's own error text. If the abort ever comes without `requestfailed` naming it, the next failure says so. Same test.
