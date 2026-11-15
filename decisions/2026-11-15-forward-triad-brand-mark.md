# Forward Triad Brand Mark and Anodized Obsidian App Icon

## Context
The owner approved the machined **Forward Triad** logo mark (three evenly spaced 18° forward-angled bars with the center pillar protruding subtly above) and anodized obsidian squircle app icon.

## Decision
- Replaced the plain `M` brand glyph in `desktop/ui/app.js` with the inline Forward Triad SVG mark (`currentColor` center bar + `0.52` opacity flanking bars) and styled `.brand .logo` and `.brand .brand-name` in `desktop/ui/modern-ux.css`.
- Rendered the anodized obsidian Forward Triad app icon across all Tauri icon sizes (`32x32.png`, `64x64.png`, `128x128.png`, `128x128@2x.png`, `icon.png`, `icon.ico`) in `desktop/src-tauri/icons/`.

## Reason
Provides a cohesive, human-centric, high-craft brand identity across the in-app sidebar and desktop launcher/dock icons.
