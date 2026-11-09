# Calendar-card timeout cause (2026-11-07)

Fixes #200: the kept FAILED screenshot from run 42059799939 shows the app never left New conversation, so the timeout was a lost hash navigation, not a slow card. The script now clicks the sidebar's Library link. No screenshot-upload step: the box-kept screenshots (3791e1e) already serve the bullet's intent, and artifact storage is full.
