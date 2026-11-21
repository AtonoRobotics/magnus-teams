# Aurora motion backgrounds, floating bento separation, and choreographed entrances

**Date:** 2026-11-20

## Choice
1. Added a `background` appearance preference (`off`, `particles`, `ribbons`, `constellation`; default `particles`) scoped exclusively to Aurora-based themes (`aurora` and `obsidian-aurora`), surfaced as a **Motion background** segmented picker in Settings → Appearance only when an Aurora-based theme is active.
2. Rendered the selected 3D motion background on a dedicated unblurred `<canvas class="aurora-field-canvas">` behind `.shell`, layered over the blurred ambient aurora wash (`.aurora-bg-canvas`), with DPR capped at `1.5`, pointer/touch parallax, state-reactive palette/speed, automatic pause on hidden tabs (`visibilitychange`), static single-frame fallback under `data-motion="reduced"`, and zero rendering under `data-motion="off"` or static themes.
3. Fixed the remaining structural overrides from `layout.css` and `chat.css` that kept Aurora visually close to the static layout: transparent `#workspace-splitter` with a floating glowing pill handle, stronger floating bento shell margins (`12px` desktop / `10px` mobile), higher-specificity rounded `main` in split mode, glass `.reqgroup.chatcard` reasoning cards, a live pulsing `.aurora-state-pill` in `.conv-head`, and choreographed view/tab/card/message entrance animations (`.aurora-view-in`, `.aurora-stagger`) so routes and tabs glide in rather than snapping.

## Reason
The blurred ambient shader canvas alone could not draw crisp spatial depth elements, and three inherited static rules (`#workspace-splitter`'s opaque gradient, `body.with-detail .shell > main { border-radius: 0 }`, and instant DOM swaps without entrance choreography) made Aurora feel visually closer to the legacy layout than the dynamic bento concept. Scoping the motion background picker and canvas strictly to `aurora` and `obsidian-aurora` preserves zero runtime overhead and untouched styling on `obsidian`, `obsidian-2`, `prism`, and `studio`, while honoring the zero-`backdrop-filter` and ≥4.5:1 contrast rules.
