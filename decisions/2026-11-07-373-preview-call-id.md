# The preview audit gets its own call ID (2026-11-07)

- The destination-preview Decision carries `preview:{send call ID}`
  instead of the send's call ID, and the developer trace labels that
  prefix "the gate (destination preview)" (#373). Reason: the trace
  indexes actions solely by call ID, so sharing it merged the preview
  read and the send into one action named after the read, displaying
  the send as a read with the send's arguments and result. The prefix
  follows the existing `check:`/`verify:` convention for non-model
  callers.
