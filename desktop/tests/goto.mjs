// Opening a page, again when the machine's network changed under the load.
// Chromium aborts a navigation in flight with net::ERR_NETWORK_CHANGED when
// the machine's addresses change (anything on a shared build box bringing an
// interface up or down is enough); the page was never answered, and Chromium
// leaves it to the caller to try again. Release run 42032928589 (#171) failed
// two console tests at their first page this way.
//
// Playwright does not always name that error: when Chromium answers the
// abort with its own error page, the load is reported as "interrupted by
// another navigation to chrome-error://chromewebdata/" (#218, runs
// 42139753250 and 42146521203). So the page's own request is watched, and
// its failure is what decides: net::ERR_NETWORK_CHANGED, however the load
// was reported, is loaded again. Any other failure is the test's to report
// at once, naming the request's own error, and a network that never settles
// fails after `tries` loads.
//
// Playwright can reject the load before it reports the request's failure,
// or not report it at all: master's run 42164847174 failed this way, and
// the failed step's screenshot showed the console loaded, Chrome having
// reloaded its error page by itself (#248). So after Chrome's error page the
// failure is waited for, briefly; when nothing names a cause, the page is
// loaded again too, within the same `tries`.
const CHROME_ERROR = "chrome-error://chromewebdata/";
export const LATE_FAILURE_MS = 2000;
export async function goto(page, url, tries = 3) {
  for (let i = 1; ; i++) {
    let failed = "";
    const watch = r => {
      try {
        if (r.isNavigationRequest() && r.frame() === page.mainFrame()) failed = r.failure()?.errorText ?? "";
      } catch {}
    };
    page.on?.("requestfailed", watch);
    try {
      return await page.goto(url);
    } catch (e) {
      const said = String(e?.message ?? e);
      const errorPage = said.includes(CHROME_ERROR);
      for (let waited = 0; errorPage && !failed && waited < LATE_FAILURE_MS; waited += 50) await new Promise(r => setTimeout(r, 50));
      const changed = said.includes("net::ERR_NETWORK_CHANGED") || failed.includes("net::ERR_NETWORK_CHANGED");
      if (i < tries && (changed || (errorPage && !failed))) continue;
      if (failed && !said.includes(failed) && e instanceof Error) e.message += ` (the page's request failed: ${failed})`;
      else if (errorPage && !failed && e instanceof Error) e.message += ` (the page's request named no failure in ${LATE_FAILURE_MS} ms, after ${i} loads)`;
      throw e;
    } finally {
      page.off?.("requestfailed", watch);
    }
  }
}
