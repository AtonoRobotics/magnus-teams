# Setup Wizard and Separate Automation Discovery Wizard

## Decision
1. Replaced the sidebar `Get started 0/3` menu item and stacked `#/start` checklist page in `desktop/ui/onboarding.js` with a dismissible 3-step `<dialog>` **Setup Wizard** (`openSetupWizard()`) that opens automatically on first sign-in for newcomers and can be reopened from Settings → Account.
2. Added a separate 3-step `<dialog>` **Automation Discovery Wizard** (`openDiscoveryWizard()`) accessible from **Activity → Schedules**, **Settings → Account**, and Step 3 of the Setup Wizard.
3. Both wizards use Magnus's theme variables (`--surface`, `--main`, `--side`, `--fg`, `--accent`, etc.) so they automatically match Obsidian Dark, Light, and every accent theme.

## Reason
Testers were treating Magnus like a search bar or image generator because the previous checklist showed plain text bullets without visually explaining autonomous triggers, cross-app execution, and approval gates. Separating the 60-second **Setup Wizard** (connect apps → interactive approval simulator → Simple / Multi-Step / Long-Running task launch) from the ongoing **Automation Discovery Wizard** lets new users get started fast while giving everyone an agent-led way to hand off repetitive work safely.
