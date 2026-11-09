# A model pick holds only under the default it was made against (2026-11-08, #540)

The composer always sends an explicit model, and a saved pick beat the
appliance default forever. After the default moved from GLM back to codex,
new messages kept going to GLM for 47 minutes, which read as "every model
fails".

- A saved pick and a draft's model now record the default in force when they
  were made, and apply only while that is still the default. Reason: the pick
  stays useful day to day, and changing the default reaches every composer.
- Sending on the default keeps nothing. Reason: chosenModel() ran on every
  send, so merely sending turned the default of the moment into a lasting pick.
- A choice saved before this change names no default, so it gives way at once.
  Reason: the owner's app holds exactly such a stale choice; no migration.
