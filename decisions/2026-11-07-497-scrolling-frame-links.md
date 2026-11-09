# Keep trusted frame links aligned and clipped (#497)
Keep external-link actions in the trusted parent and preserve script-disabled composed frames. Measure marker geometry against the frame and each clipping scrollport, then batch parent hit-target updates. Offscreen links cannot extend document overflow or overlay unrelated cells. Preserve prose text accessibility and independent parent action names without duplicate visual painting.

Scroll and font events schedule alignment. Installed WebKit does not reliably forward child scroll events from sandboxed documents, so interactive visible frames also compare scroll offsets on parent animation frames; full geometry is measured only after movement. Intersection observation pauses offscreen work, and removal/document replacement disposes listeners, observation and frames. Keyboard focus reveals its real marker with room for the focus outline.

Verify actual scrolling, keyboard navigation and OS external-browser handoff, rather than substituting bridge responses. Original file bytes, permissions, download behavior and sandbox policy remain unchanged.
