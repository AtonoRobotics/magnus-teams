# Continuity CI selector correction

Read #479 CI run 42432202208: 706/710 passed, with desktop_flow (two auth paths), kill_switch and library failures from ambiguous selectors. The Message/attachment ambiguity is already fixed by #480. Corrected the three New conversation link locators in flow and killswitch to exact matching. This retains the required independently discoverable draft entry. Full browser/KVM and installed-host validation remains required in CI; local selector fixture is only a locator regression, not agent reasoning evidence.
