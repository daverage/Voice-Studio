# Voice Studio UI Consistency Audit Report
**Date:** 2026-01-30
**File:** src/ui.rs (1687 lines)
**Goal:** Identify inconsistent component creation patterns and missing CSS classes

---

## Executive Summary

The ui.rs file has **5 critical inconsistencies** where the same type of element is created in multiple different ways. This audit identifies each pattern and provides recommendations for standardization.

---

## 1. CRITICAL: Button Creation - 5 Different Patterns ⚠️

### Issue
Buttons are created using 5 completely different patterns throughout the codebase:

### Pattern A: Mode Toggle Buttons (Header)
**Location:** Lines 1273-1295
**Implementation:**
```rust
Button::new(
    cx,
    move |_| set_macro_mode(&p1, &g1, true),
    |cx| Label::new(cx, "Simple"),
)
.class(if m { "mode-button-active" } else { "mode-button" })
```
**Classes:** `mode-button`, `mode-button-active` (conditional)
**Wrapper:** None (direct Button)

---

### Pattern B: Tab Header Buttons (Advanced Mode)
**Location:** Lines 1619-1651
**Implementation:**
```rust
Button::new(
    cx,
    |ex| ex.emit(AdvancedTabEvent::SetTab(AdvancedTab::CleanRepair)),
    |cx| Label::new(cx, "Clean & Repair"),
)
.class(if current_tab == AdvancedTab::CleanRepair {
    "tab-header-active"
} else {
    "tab-header"
})
```
**Classes:** `tab-header`, `tab-header-active` (conditional)
**Wrapper:** None (direct Button)

---

### Pattern C: Momentary Action Buttons (Learn/Clear)
**Location:** Lines 1043-1092
**Implementation:**
```rust
HStack::new(cx, |cx| {
    Label::new(cx, "Learn").hoverable(false);
})
.class("small-button")
.on_mouse_down(move |cx, btn| { /* ... */ })
.on_mouse_up(move |cx, btn| { /* ... */ })
```
**Classes:** `small-button`
**Wrapper:** HStack (not Button widget)
**Special:** Uses on_mouse_down/on_mouse_up for momentary behavior

---

### Pattern D: Standard Footer Buttons
**Location:** Lines 1366-1511
**Implementation:**
```rust
Button::new(
    cx,
    move |_| { open_url("..."); },
    |cx| Label::new(cx, "Help"),
)
.class("footer-button")
```
**Classes:** `footer-button`, sometimes `version-release-button`
**Wrapper:** None (direct Button)

---

### Pattern E: Reset Button
**Location:** Lines 1375-1466
**Implementation:**
```rust
Button::new(
    cx,
    move |_| { /* reset all parameters */ },
    |cx| Label::new(cx, "Reset"),
)
.class("reset-button")
```
**Classes:** `reset-button`
**Wrapper:** None (direct Button)
**Issue:** `reset-button` class is NOT in ui.css - relies on inherited button styles

---

### Recommendation
Create a unified button helper function:
```rust
fn create_button<'a>(
    cx: &'a mut Context,
    label: &'static str,
    class: &'static str,
    callback: impl Fn(&mut EventContext) + 'static,
) -> Handle<'a, Button>
```

For momentary buttons (Learn/Clear), create a separate helper:
```rust
fn create_momentary_button<'a>(
    cx: &'a mut Context,
    label: &'static str,
    on_press: impl Fn(...) + 'static,
    on_release: impl Fn(...) + 'static,
) -> Handle<'a, HStack>
```

---

## 2. MODERATE: Dropdown Creation - 2 Nearly Identical Functions

### Issue
Two dropdown helper functions exist with almost identical implementations:

### Function A: create_dropdown
**Location:** Lines 681-727
**Purpose:** Output preset selection
**Classes:** `dropdown-row`, `dropdown-label`, `dropdown-box`, `dropdown-selected`, `dropdown-options`, `dropdown-option`
**Unique:** None

### Function B: create_dsp_preset_dropdown
**Location:** Lines 729-848
**Purpose:** DSP preset selection
**Classes:** Same as Function A + `dsp-preset-dropdown`
**Unique:** Includes preset application logic (lines 776-835)

### Code Duplication
Both functions share:
- Identical structure (HStack → Label + Dropdown)
- Identical class application
- Identical option rendering pattern

### Recommendation
Consolidate into a single parameterized function:
```rust
fn create_preset_dropdown<'a, T>(
    cx: &'a mut Context,
    label: &'static str,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
    param_getter: impl Fn(&VoiceParams) -> &impl Param,
    presets: &[T],
    preset_name: impl Fn(&T) -> &str,
    on_select: impl Fn(&T, &ParamSetter, &VoiceParams),
    extra_class: Option<&'static str>,
) -> Handle<'a, HStack>
```

---

## 3. MINOR: Spacer Elements - 4 Different Patterns

### Issue
Empty space is created in multiple ways:

### Pattern A: Dedicated Spacer Class
**Location:** Line 914
```rust
Element::new(cx).class("spacer");
```
**CSS:** `.spacer { height: 18px; }`

### Pattern B: Fill Height
**Location:** Lines 965, 981
```rust
Element::new(cx).class("fill-height");
```
**CSS:** `.fill-height { height: 1s; }`

### Pattern C: Fill Width
**Location:** Lines 1257, 1359
```rust
Element::new(cx).class("fill-width");
```
**CSS:** `.fill-width { width: 1s; }`

### Pattern D: Explicit Zero Size
**Location:** Line 950
```rust
Element::new(cx).height(Pixels(0.0)).width(Pixels(0.0));
```
**No CSS class**

### Recommendation
This is acceptable diversity - different spacers serve different purposes:
- `.spacer` - fixed 18px vertical gap
- `.fill-height`/`.fill-width` - flexible stretch spacers
- Zero-size - invisible binding trigger elements

**Action:** Add comments in ui.css explaining each spacer type

---

## 4. MINOR: Labels - Inconsistent hoverable(false)

### Issue
Some labels use `.hoverable(false)`, some don't:

### With hoverable(false):
- Line 1044: "Learn" button label
- Line 1072: "Clear" button label
- Line 668: Dial value labels
- Line 627: Slider value labels

### Without hoverable(false):
- All other labels (hundreds of instances)

### Analysis
The `.hoverable(false)` is used on labels inside interactive parent elements (buttons, sliders) to prevent hover state conflicts.

### Recommendation
This is **intentional and correct** - no action needed.

---

## 5. MISSING CSS CLASSES

### Classes in ui.rs NOT in ui.css:

#### Custom View Elements (these use draw() method, not CSS):
- `level-meter` - LevelMeter custom view
- `noise-learn-meter` - NoiseLearnQualityMeter custom view
- `nf-leds` - NoiseFloorLeds custom view
- `slider-visuals` - SliderVisuals custom view
- `dial-visuals` - DialVisuals custom view

**Recommendation:** Add commented placeholders in ui.css for documentation

#### Actual Missing Classes:
- `reset-button` - Used on line 1466, but NOT in ui.css
- `column-header` - Used on lines 875, 955, 1229, partially in CSS as `.column-header, .col-levels`
- `quality-meter-container` - Used on line 1102, NOT in ui.css
- `group-container` - Used on line 1106, NOT in ui.css
- `macro-column` - Used on line 983, NOT in ui.css

**Recommendation:** Add these missing classes to ui.css

---

## 6. ELEMENTS WITHOUT UNIQUE CLASSES

All major UI elements have unique identifiable classes. ✓

**Containers:**
- Header: `.header`
- Footer: `.footer`
- Main view: `.main-view`
- Levels column: `.levels-column`
- Simple container: `.simple-container`
- Advanced columns: `.adv-columns`
- Tab content: `.tab-content`, `.tab-clean-repair`, `.tab-shape-polish`
- Output section: `.output-section`

**Components:**
- Sliders: `.slider-container`, `.adv-row`
- Dials: `.dial-container`
- Dropdowns: `.dropdown-row`
- Buttons: `.mode-button`, `.tab-header`, `.footer-button`, `.small-button`
- Meters: `.meter-grid`, `.meter-col`, `.meter-track`

---

## 7. HELPER FUNCTIONS - Current State

### Existing Helpers (GOOD):
✓ `create_slider` - Consistent slider creation
✓ `create_macro_dial` - Consistent dial creation
✓ `create_dropdown` - Output preset dropdown
✓ `create_dsp_preset_dropdown` - DSP preset dropdown

### Existing Builders (GOOD):
✓ `build_levels` - Meter panel
✓ `build_macro` - Simple mode panel
✓ `build_clean_repair_tab` - Advanced tab 1
✓ `build_shape_polish_tab` - Advanced tab 2
✓ `build_output` - Output section
✓ `build_header` - Header bar
✓ `build_footer` - Footer bar

### Utility Functions (GOOD):
✓ `set_macro_mode` - Mode switching
✓ `sync_advanced_from_macros` - Macro sync
✓ `open_url` - External links

### Missing Helpers (SHOULD ADD):
❌ Button creation helper
❌ Momentary button helper
❌ Unified dropdown helper

---

## PRIORITY ACTION ITEMS

### HIGH PRIORITY:
1. **Standardize button creation** - Create `create_button()` helper
2. **Add missing CSS classes** - `reset-button`, `macro-column`, `quality-meter-container`, `group-container`
3. **Consolidate dropdown functions** - Merge `create_dropdown` and `create_dsp_preset_dropdown`

### MEDIUM PRIORITY:
4. **Document custom view elements** - Add commented placeholders in ui.css
5. **Add spacer documentation** - Comment in ui.css explaining each spacer type

### LOW PRIORITY:
6. **Code cleanup** - Remove unused imports/types if any

---

## NEXT STEPS

1. Create button helper functions
2. Refactor all button creations to use new helpers
3. Consolidate dropdown functions
4. Update ui.css with missing classes and documentation
5. Run memory_ralph to verify builds
6. Test UI in DAW to verify no visual regressions

---

**End of Audit Report**
