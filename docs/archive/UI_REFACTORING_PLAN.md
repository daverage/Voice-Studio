# UI Refactoring Implementation Plan

**Voice Studio (vxcleaner) - Modular UI Structure**

**Date Created:** 2026-01-30
**Status:** Ready for Execution
**Target:** Refactor monolithic `src/ui.rs` (61KB) into modular structure per design specification

---

## Overview

This plan refactors the Voice Studio UI from a single 61KB `ui.rs` file into a modular structure with clear separation of concerns:

- **ui/mod.rs** - Module initialization and re-exports
- **ui/layout.rs** - Header, body, footer builders
- **ui/components.rs** - Reusable helper functions (sliders, knobs, dropdowns, buttons)
- **ui/advanced.rs** - Advanced mode UI builders and tab logic
- **ui/simple.rs** - Simple mode UI builders
- **ui/meters.rs** - Custom meter widgets and drawing logic
- **ui/state.rs** - Model struct, events, and parameter sync logic

**Expected Outcome:**
- Better code maintainability
- Easier to locate and modify specific features
- Clearer separation of concerns
- Compliance with design specification
- Full backward compatibility (same functionality, reorganized code)

---

## Prerequisites

1. **Backup Existing Files**
   - `src/ui.rs` → `src/ui.rs.backup` (already exists)
   - `src/meters.rs` → `src/meters.rs.backup` (create new)
   - Current git status: master branch, all changes committed

2. **Environment Checks**
   ```bash
   cd /Users/andrzejmarczewski/Documents/GitHub/Voice-Studio-Rollback
   cargo build --release 2>&1 | head -20  # Verify current build passes
   wc -l src/ui.rs  # Confirm file size
   ```

3. **Read-Only Reference Files**
   - `src/ui.rs` (source)
   - `src/meters.rs` (source)
   - `src/lib.rs` (understand how UI is initialized)

---

## Step-by-Step Execution

### PHASE 1: Directory Structure Setup

**1.1 Create ui module directory**
```bash
mkdir -p /Users/andrzejmarczewski/Documents/GitHub/Voice-Studio-Rollback/src/ui
```

**1.2 Verify creation**
```bash
ls -la /Users/andrzejmarczewski/Documents/GitHub/Voice-Studio-Rollback/src/ui
# Should show: empty directory
```

---

### PHASE 2: Extract State Management (ui/state.rs)

**Purpose:** Model struct, custom events, sync logic

**2.1 Identify what to extract from ui.rs**

Search for and extract these items:
- `VoiceStudioData` struct (if it exists, or similar data model)
- `AdvancedTabEvent` enum (tab switching events)
- `AdvancedTab` enum (Clean & Repair, Shape & Polish)
- `sync_advanced_from_macros()` function
- `set_macro_mode()` function
- Any other event-related code

**2.2 Create ui/state.rs**

Pattern:
```rust
use crate::VoiceParams;  // Import from parent crate
use nih_plug_vizia::vizia::prelude::*;
use std::sync::Arc;

// Import events enum (if defined elsewhere)
// #[derive(Debug, Clone, Copy)]
// pub enum AdvancedTabEvent { ... }

// Re-export model struct
pub struct VoiceStudioData {
    // Fields needed for UI state
}

// Sync functions
pub fn sync_advanced_from_macros(params: &Arc<VoiceParams>, gui: Arc<dyn GuiContext>) {
    // Implementation from ui.rs
}

pub fn set_macro_mode(params: &Arc<VoiceParams>, gui: Arc<dyn GuiContext>, enabled: bool) {
    // Implementation from ui.rs
}
```

**2.3 Verify extraction**
- All event types defined
- All model fields included
- All sync functions present
- No missing imports

---

### PHASE 3: Extract Component Helpers (ui/components.rs)

**Purpose:** Reusable button, slider, knob, dropdown builders

**3.1 Create ui/components.rs with functions:**

```rust
use nih_plug_vizia::vizia::prelude::*;
use std::sync::Arc;
use crate::VoiceParams;

// BUTTON HELPERS
pub fn create_button<'a>(
    cx: &'a mut Context,
    label: &'static str,
    class: &'static str,
    callback: impl Fn(&mut EventContext) + 'static,
) -> Handle<'a, Button> {
    // Extract from ui.rs: create_button implementation
}

pub fn create_toggle_button<'a>(
    cx: &'a mut Context,
    label: &'static str,
    is_active: bool,
    active_class: &'static str,
    inactive_class: &'static str,
    callback: impl Fn(&mut EventContext) + 'static,
) -> Handle<'a, Button> {
    // Extract from ui.rs: create_toggle_button implementation
}

pub fn create_momentary_button<'a, P>(
    cx: &'a mut Context,
    label: &'static str,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
    param_getter: impl Fn(&VoiceParams) -> &P + Copy + Send + Sync + 'static,
) -> Handle<'a, HStack>
where
    P: Param<Plain = bool> + 'static,
{
    // Extract from ui.rs: create_momentary_button implementation
}

// SLIDER HELPERS
pub fn create_slider<'a>(
    cx: &'a mut Context,
    label: &'static str,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
    param_getter: impl Fn(&VoiceParams) -> &impl Param + Copy + Send + Sync + 'static,
) -> Handle<'a, HStack> {
    // Extract from ui.rs: create_slider implementation
}

// KNOB HELPERS
pub fn create_macro_dial<'a>(
    cx: &'a mut Context,
    label: &'static str,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
    param_getter: impl Fn(&VoiceParams) -> &impl Param + Copy + Send + Sync + 'static,
) -> Handle<'a, VStack> {
    // Extract from ui.rs: create_macro_dial implementation
}

// DROPDOWN HELPERS
pub fn create_dropdown<'a>(
    cx: &'a mut Context,
    label: &'static str,
    // ... other parameters from ui.rs
) -> Handle<'a, HStack> {
    // Extract from ui.rs: create_dropdown implementation
}

pub fn create_dsp_preset_dropdown<'a>(
    cx: &'a mut Context,
    label: &'static str,
    // ... other parameters from ui.rs
) -> Handle<'a, HStack> {
    // Extract from ui.rs: create_dsp_preset_dropdown implementation
}
```

**3.2 Extract all helper function bodies**

Go through `src/ui.rs` and copy the complete function implementations:
- `create_button` - find in ui.rs, copy entire function
- `create_toggle_button` - copy entire function
- `create_momentary_button` - copy entire function
- `create_slider` - copy entire function
- `create_macro_dial` - copy entire function
- `create_dropdown` - copy entire function
- `create_dsp_preset_dropdown` - copy entire function

**3.3 Add module documentation**

```rust
//! Reusable UI component builders
//!
//! This module provides helper functions for creating consistent UI elements:
//! - Buttons: standard, toggle, momentary
//! - Sliders: horizontal parameter controls
//! - Knobs: macro dials
//! - Dropdowns: preset selection
//!
//! All builders use consistent patterns with nih_plug's ParamSlider for binding
//! to plugin parameters. Styling is handled via CSS classes defined in ui.css.
```

---

### PHASE 4: Extract Meters (ui/meters.rs)

**Purpose:** Custom meter widgets and level visualization

**4.1 Move existing meters.rs**

```bash
mv /Users/andrzejmarczewski/Documents/GitHub/Voice-Studio-Rollback/src/meters.rs \
   /Users/andrzejmarczewski/Documents/GitHub/Voice-Studio-Rollback/src/ui/meters.rs
```

**4.2 Update module declaration in ui/meters.rs**

At the top of the moved file, add if not present:
```rust
//! Custom meter widgets for level visualization
//!
//! Provides LevelMeter, NoiseFloorLeds, and other custom Vizia views
//! for displaying audio levels and processing feedback.
```

**4.3 Verify no imports are broken**
- Check that all `use` statements are still valid
- If meters.rs imported from parent crate, may need adjustment

---

### PHASE 5: Extract Layout Builders (ui/layout.rs)

**Purpose:** High-level UI structure (header, body, footer)

**5.1 Create ui/layout.rs**

```rust
use nih_plug_vizia::vizia::prelude::*;
use std::sync::Arc;
use crate::VoiceParams;

pub fn build_header<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, Element> {
    // Extract from ui.rs: build_header implementation
}

pub fn build_footer<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, Element> {
    // Extract from ui.rs: build_footer implementation
}

pub fn build_body<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, Element> {
    // Extract from ui.rs: body building logic (conditional simple/advanced)
}

pub fn build_levels<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
) -> Handle<'a, Element> {
    // Extract from ui.rs: build_levels implementation
}

pub fn build_macro<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, Element> {
    // Extract from ui.rs: build_macro implementation
}

pub fn build_output<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, Element> {
    // Extract from ui.rs: build_output implementation
}
```

**5.2 Extract builder function bodies**

Search ui.rs for these functions:
- `fn build_header`
- `fn build_footer`
- `fn build_levels`
- `fn build_macro`
- `fn build_output`
- Any top-level body/layout building logic

Copy complete function bodies to ui/layout.rs.

**5.3 Update public visibility**

Ensure all functions are marked `pub fn` so they can be called from other modules.

---

### PHASE 6: Extract Advanced Mode UI (ui/advanced.rs)

**Purpose:** Advanced mode specific UI builders and tab logic

**6.1 Create ui/advanced.rs**

```rust
use nih_plug_vizia::vizia::prelude::*;
use std::sync::Arc;
use crate::VoiceParams;

pub fn build_clean_repair_tab<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, Element> {
    // Extract from ui.rs: build_clean_repair_tab implementation
}

pub fn build_shape_polish_tab<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, Element> {
    // Extract from ui.rs: build_shape_polish_tab implementation
}

pub fn build_advanced_tabs<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, Element> {
    // Extract from ui.rs: tab bar and tab switching logic
}
```

**6.2 Extract from ui.rs**

Find and copy these functions:
- `fn build_clean_repair_tab`
- `fn build_shape_polish_tab`
- Any tab-related logic
- AdvancedTab enum if defined in ui.rs (move to state.rs)
- AdvancedTabEvent enum if defined in ui.rs (move to state.rs)

**6.3 Add documentation**

```rust
//! Advanced mode UI builders
//!
//! Provides tab-based UI for detailed parameter control.
//!
//! Tabs:
//! - Clean & Repair: Static and adaptive noise reduction
//! - Shape & Polish: Proximity and clarity shaping
```

---

### PHASE 7: Extract Simple Mode UI (ui/simple.rs)

**Purpose:** Simple mode specific UI (if not covered by build_macro)

**7.1 Create ui/simple.rs**

```rust
use nih_plug_vizia::vizia::prelude::*;
use std::sync::Arc;
use crate::VoiceParams;

pub fn build_simple_mode<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, Element> {
    // Extract from ui.rs: simple mode specific logic
    // (if different from build_macro)
}
```

**7.2 Note**

If simple mode UI is already fully contained in `build_macro()`, this file can be minimal or just re-export. Verify in ui.rs how simple mode is built.

---

### PHASE 8: Create Module Initialization (ui/mod.rs)

**Purpose:** Central re-export point for all UI modules

**8.1 Create ui/mod.rs**

```rust
//! Voice Studio UI module
//!
//! Modular organization of the Vizia GUI:
//! - `state`: Data model and events
//! - `components`: Reusable UI builders
//! - `layout`: Top-level layout structure
//! - `advanced`: Advanced mode panels
//! - `simple`: Simple mode panels
//! - `meters`: Custom meter widgets

pub mod state;
pub mod components;
pub mod layout;
pub mod advanced;
pub mod simple;
pub mod meters;

// Re-export public items for convenience
pub use state::{VoiceStudioData, set_macro_mode, sync_advanced_from_macros};
pub use components::{
    create_button, create_toggle_button, create_momentary_button,
    create_slider, create_macro_dial, create_dropdown, create_dsp_preset_dropdown,
};
pub use layout::{build_header, build_footer, build_body, build_levels, build_macro, build_output};
pub use advanced::{build_clean_repair_tab, build_shape_polish_tab, build_advanced_tabs};
pub use meters::{LevelMeter, NoiseFloorLeds};
```

**8.2 Module documentation**

Include clear documentation about the organization and how to use each submodule.

---

### PHASE 9: Update Root UI Dispatcher

**Purpose:** Update the file that actually builds the UI (likely in lib.rs)

**9.1 Identify where UI is initialized**

Search `src/lib.rs` for:
- `nih_plug_vizia` usage
- References to `ui_raw` or similar
- Where `include_str!("ui.css")` is used

**9.2 Update imports**

Replace:
```rust
// OLD
mod ui;
// ...
use ui::{ ... };
```

With:
```rust
// NEW
mod ui;
use ui::{VoiceStudioData, build_header, build_footer, /* ... */};
```

Or if using `ui::*`:
```rust
use ui::*;
```

**9.3 Note on ui.css location**

The file `src/ui.css` should NOT be moved. Update its include path if needed:
```rust
const STYLE: &str = include_str!("ui.css");
```

This remains valid since we're still in `src/`.

---

### PHASE 10: Build and Test

**10.1 Test compilation**
```bash
cd /Users/andrzejmarczewski/Documents/GitHub/Voice-Studio-Rollback
cargo build 2>&1 | tee /tmp/build.log
```

**Expected:** Should compile without errors.

**If errors:** Review error messages, check:
- Missing imports
- Visibility (pub/private) issues
- Module path references

**10.2 Test release build**
```bash
cargo build --release 2>&1 | tee /tmp/build-release.log
```

**10.3 Test with debug features**
```bash
cargo build --release --features debug 2>&1 | tee /tmp/build-debug.log
```

**10.4 Verify bundle creation**
```bash
cargo nih-plug bundle vxcleaner --release 2>&1 | head -50
```

**Expected:** Bundle should be created successfully.

---

### PHASE 11: Validation Checklist

**Functional Tests** (run these checks):

- [ ] Code compiles without warnings
- [ ] Release build succeeds
- [ ] Debug build with --features debug succeeds
- [ ] Plugin bundle created successfully
- [ ] All files in correct locations:
  - [ ] `src/ui/mod.rs` exists
  - [ ] `src/ui/state.rs` exists
  - [ ] `src/ui/components.rs` exists
  - [ ] `src/ui/layout.rs` exists
  - [ ] `src/ui/advanced.rs` exists
  - [ ] `src/ui/simple.rs` exists
  - [ ] `src/ui/meters.rs` exists
  - [ ] `src/ui.css` still in `src/` (not moved)
  - [ ] `src/ui.rs` removed (or renamed to ui.rs.old)

**Code Quality Checks**:

- [ ] No duplicate code between modules
- [ ] All imports valid
- [ ] No circular dependencies
- [ ] Module documentation present
- [ ] Function documentation preserved
- [ ] All helper functions callable from other modules

**Functionality Checks** (requires DAW):

- [ ] Simple mode loads without errors
- [ ] Advanced mode loads without errors
- [ ] Mode switching works (Simple ↔ Advanced)
- [ ] All sliders functional
- [ ] All buttons clickable and functional
- [ ] Meters animate correctly
- [ ] Dropdowns work
- [ ] All parameters respond to changes

---

## Rollback Plan

If refactoring encounters problems:

### Quick Rollback (if build fails)

```bash
# Option 1: Restore from backup and rebuild
rm -rf /Users/andrzejmarczewski/Documents/GitHub/Voice-Studio-Rollback/src/ui/
cp /Users/andrzejmarczewski/Documents/GitHub/Voice-Studio-Rollback/src/ui.rs.backup \
   /Users/andrzejmarczewski/Documents/GitHub/Voice-Studio-Rollback/src/ui.rs
cp /Users/andrzejmarczewski/Documents/GitHub/Voice-Studio-Rollback/src/meters.rs.backup \
   /Users/andrzejmarczewski/Documents/GitHub/Voice-Studio-Rollback/src/meters.rs
cargo build --release
```

### Git Rollback (if changes committed)

```bash
# Revert last commit
git reset --hard HEAD~1

# Or restore specific files
git checkout HEAD -- src/ui.rs src/meters.rs
rm -rf src/ui/
```

---

## File Dependency Map

After refactoring, dependencies should flow like this:

```
lib.rs
  ├── ui/mod.rs
  │   ├── ui/state.rs (no dependencies on other ui modules)
  │   ├── ui/components.rs (depends on: state)
  │   ├── ui/layout.rs (depends on: components)
  │   ├── ui/advanced.rs (depends on: components, state)
  │   ├── ui/simple.rs (depends on: components, state)
  │   └── ui/meters.rs (standalone, no internal ui dependencies)
  └── ui.css (no Rust dependencies)
```

---

## Common Issues & Solutions

### Issue: "Module not found" error

**Cause:** Module not declared in parent `mod.rs`
**Solution:** Ensure `ui/mod.rs` has `pub mod xyz;` declarations

### Issue: "Private item used outside module"

**Cause:** Function not marked `pub`
**Solution:** Add `pub` to all exported functions in submodules

### Issue: "Cannot find crate in scope"

**Cause:** Import path incorrect
**Solution:** Check:
- `use crate::X` for parent crate items
- `use super::X` for sibling modules
- `use crate::ui::X` for other ui modules

### Issue: Circular dependency

**Cause:** Module A imports from Module B, B imports from A
**Solution:** Restructure - move shared types to state.rs or create new common module

---

## After Refactoring

### Update Documentation

Update these files to reflect new structure:
- [ ] `UI_CONSISTENCY_SUMMARY.md` - add section on refactored structure
- [ ] `UI_DESIGN.md` - update File Organization section with actual paths
- [ ] `CLAUDE.md` - if it mentions file structure
- [ ] Add comments to `src/ui/mod.rs` explaining module organization

### Create Architecture Document

Consider creating `docs/UI_ARCHITECTURE.md` with:
- Module dependency diagram
- How to add new components
- Naming conventions
- Import guidelines

### Future: Consider Splitting Further

Once refactoring is complete, future enhancements might include:
- `ui/styles/` - Organize CSS into multiple files (light/dark themes)
- `ui/custom_views/` - If more custom widgets are added beyond meters
- `ui/events/` - If event types grow

---

## Success Criteria

The refactoring is **COMPLETE** when:

1. ✅ All code is in modular files under `src/ui/`
2. ✅ `src/ui.rs` (monolithic) is removed or archived
3. ✅ No circular dependencies exist
4. ✅ All modules compile with zero warnings
5. ✅ Release and debug builds succeed
6. ✅ Plugin loads in DAW without errors
7. ✅ All UI functionality works identically to before
8. ✅ Code is documented and easy to navigate
9. ✅ Future developers can easily find and modify UI code

---

## Estimated Scope

- **Files to create:** 7 new files in `src/ui/`
- **Files to modify:** 1 file (`src/lib.rs` or equivalent)
- **Files to move:** 1 file (`src/meters.rs` → `src/ui/meters.rs`)
- **Files to delete:** 1 file (`src/ui.rs` - after extraction complete)
- **Lines of code to reorganize:** ~61,000 lines
- **Total time estimate:** Agent execution time (actual implementation by another AI agent)

---

## Notes for Executing Agent

**CRITICAL REMINDERS:**

1. **Do NOT delete** `src/ui.rs` until ALL content is extracted and verified to compile
2. **DO create** backups before making changes
3. **DO test compilation** after each major phase
4. **DO preserve** all function logic - this is refactoring, not rewriting
5. **DO maintain** the same public API - nothing should break for lib.rs
6. **DO check** that ui.css is NOT moved (stays in src/)
7. **DO verify** that `include_str!("ui.css")` still works in lib.rs

**Working with Large Functions:**

- Use line counts to verify extraction: `wc -l src/ui/components.rs` should be reasonable
- If any single module grows over 1500 lines, consider further splitting
- Use `grep -n "pub fn"` to find all function boundaries

**Git Workflow:**

```bash
# Recommended commits:
git add src/ui/state.rs && git commit -m "refactor: extract state module"
git add src/ui/components.rs && git commit -m "refactor: extract components module"
git add src/ui/layout.rs && git commit -m "refactor: extract layout module"
# ... etc for each module
git rm src/ui.rs && git commit -m "refactor: remove monolithic ui.rs after extraction"
```

This allows rollback to any point if needed.

---

## References

- Design Specification: `docs/Design Specification for VST Plugin UI Redesign (Vizia Framework).pdf` - Section "File Organization"
- Current UI Code: `src/ui.rs` (61KB source)
- Meter Code: `src/meters.rs` (to be moved)
- CSS: `src/ui.css` (DO NOT MOVE)

---

**Document Version:** 1.0
**Created:** 2026-01-30
**Status:** Ready for Execution
**Next Step:** Assign to agent and execute Phase 1-11 in sequence
