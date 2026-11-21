# Current PlanCraft screenshots

These PNGs document the current working app UI. They are **not** an independent design specification: the approved ArtCraft reference remains the design target.

The screenshots are actual `PlanCraftApp` client-area renders from the Rust/egui test harness. They use the standard demo or a small synthetic scheduling example, not an open user plan. Native macOS window decorations are outside these client-area captures. No physical print job is sent; `print-preview.png` shows the preview dialog only.

## Regenerate safely

From the repository root:

```sh
CARGO_TARGET_DIR=target-menus cargo test --offline -p plancraft-app docs_screenshot_images -- --ignored --nocapture
```

This writes the candidate PNGs and capture-state manifest to `target/docs-screenshots/`, **not** directly into this folder. Inspect those renders before replacing the documentation images. The harness fails on caught UI errors, missing expected controls, invalid synthetic scheduling data, duplicate filenames, or unsuccessful PNG rendering/saving.

`manifest.json` records capture states, languages, themes, dimensions, source provenance and PNG checksums. Standard captures are 1440×900; `overview-narrow.png` is 960×900.

The original fourteen screenshot filenames are retained. Additional captures show Overview in dark/light/narrow layouts, calculated Auto duration, resource allocation controls on Resources, active Format toggles and the single selection-filter clear action.

The Auto-duration example keeps its stored five-day fallback but calculates two working days, 9–10 December 2026, before a one-day send task due on 11 December and a review milestone on 16 December. It is separate from any real project.
