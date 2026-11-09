# Read aloud respects the run's call cap (#330)

Follow-up to #319: read aloud checked only dollars. A run at its
model-call cap got one more call for polish. It now falls back to the
answer text at the cap, like it does with no dollars left.

The issue also asked for the grant check the other sites have. Left
out deliberately: grants attach to tasks created from chat, and read
aloud serves only chat-turn answers, so a run here never carries a
grant — the check would be untestable dead code. Noted on the issue;
reconsider if chat turns ever run under grants.
