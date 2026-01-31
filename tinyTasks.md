# Tasks – Phase 3 Feature Implementation & Documentation Cleanup

- [x] Clean up and sync documentation files
  - [x] Update `CLAUDE.md` (remove protocol duplicates, update version/architecture)
  - [x] Sync `GEMINI.md` with `CLAUDE.md`
  - [x] Sync `QWEN.md` with `CLAUDE.md`
  - [x] Refresh `README.md` to match the current DSP pipeline, macros, and release instructions
  - [x] Update `web/index.html` + `web/help.html` so the website/help page reflect the current mode imagery, version, and feature set
  - [x] Archive or tidy non-core Markdown (`UI_*`, `NEXT_STEPS.md`, etc.) under `docs/archive/` so the root retains only working/project docs
- [ ] Implement Double-Click Reset for UI controls (blocked pending the roadmap that follows documentation cleanup)
-  - [ ] Implement for sliders in `src/ui/components.rs`
-  - [ ] Implement for macro dials in `src/ui/components.rs`
- [ ] Implement Hover Tooltips (Comprehensive) (blocked until UI refinement sprint)
-  - [ ] Verify existing tooltips
-  - [ ] Add missing tooltips to all advanced controls
- [ ] Implement Cursor Changes on hover (blocked until UI refinement sprint)
-  - [ ] Update `src/ui.css` with cursor properties for interactive elements
- [ ] Final Verification (blocked until UI feature tickets are addressed)
  - [ ] cargo build --release
  - [ ] verify all documentation is consistent
