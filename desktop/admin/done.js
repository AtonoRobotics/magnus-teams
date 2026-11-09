// The page a step-up opened in its own window comes back to (admin console
// spec, Shell: "Step-up opens the provider's prompt in a sheet and returns
// to the action"). The console's page, which follows the session, carries
// on with the action; this window closes, or says it can be closed.
"use strict";
window.close();
