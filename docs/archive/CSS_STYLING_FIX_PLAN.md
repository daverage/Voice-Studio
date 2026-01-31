# CSS Styling Mismatch - Fix Plan

**Issue:** UI renders but styling doesn't apply correctly
**Root Cause:** CSS uses element-type selectors that don't match Vizia element structure
**Severity:** HIGH - Blocks visual QA/DAW testing
**Date:** 2026-01-30

---

## Problem Analysis

### What's Broken
- Macro dials render as plain circles, no labels/values visible
- Header/footer styling missing
- Dropdowns not styled
- Overall layout/spacing broken
- No button styling

### Why It's Broken

The CSS file uses **element-type selectors** (e.g., `dropdown`, `button`) that expect specific Vizia element types. But:

1. **dropdown elements**: CSS tries to style:
   ```css
   dropdown.dropdown-box { ... }
   dropdown popup { ... }
   ```
   But the code creates dropdowns as `HStack` containers, NOT `dropdown` elements

2. **param-slider elements**: CSS references:
   ```css
   param-slider { ... }
   param-slider .fill { ... }
   ```
   These may not exist in the element structure

3. **Missing CSS classes**: Code uses classes that aren't defined:
   - `.body`
   - `.dials-container`
   - Some meter labels

### CSS Selectors That Are Problematic

| Selector | Issue | Fix |
|----------|-------|-----|
| `button` | ✅ Works (Button element exists) | Keep |
| `param-slider` | ❌ May not exist | Replace with `.slider-visual`, `.slider-container` |
| `dropdown.dropdown-box` | ❌ Dropdown element doesn't exist | Use `.dropdown-box` class selector only |
| `dropdown popup` | ❌ Dropdown element doesn't exist | Use `.dropdown-options` class selector |

---

## Solution: Convert to Class-Only Selectors

**The fix:** Change CSS to use **class selectors only**, removing dependency on element types.

Vizia's CSS engine properly handles:
- ✅ Class selectors: `.dropdown-box { ... }`
- ✅ Descendant selectors: `.dropdown-row .dropdown-label { ... }`
- ✅ Pseudo-classes: `.button:hover { ... }`
- ❌ Element-type selectors: `dropdown { ... }` (doesn't work reliably)

---

## Fixes Required

### Fix #1: Replace `dropdown.dropdown-box` selectors

**Current (Broken):**
```css
dropdown.dropdown-box {
    width: 180px;
    height: 28px;
    /* ... */
}

dropdown popup {
    background-color: #1a1a1a;
    border: 1px solid #3fa7ff;
    /* ... */
}
```

**Replace With:**
```css
.dropdown-box {
    width: 180px;
    height: 28px;
    /* ... */
}

.dropdown-options {
    background-color: #1a1a1a;
    border: 1px solid #3fa7ff;
    /* ... */
}
```

### Fix #2: Replace `param-slider` selectors

**Current (Broken):**
```css
param-slider {
    height: auto;
}

param-slider .fill {
    background-color: #3fa7ff;
}
```

**Replace With:**
```css
.slider-visual {
    height: auto;
}

.slider-visual .fill {
    background-color: #3fa7ff;
}
```

### Fix #3: Add missing CSS classes

Add to ui.css:

```css
/* Missing container classes */
.body {
    height: 1s;
    child-space: 24px;
}

.dials-container {
    col-between: 40px;
    row-between: 20px;
}

/* Meter label classes */
.meter-label-in {
    font-size: 11px;
    color: #9ca3af;
}

.meter-label-out {
    font-size: 11px;
    color: #9ca3af;
}

.meter-label-gr {
    font-size: 11px;
    color: #9ca3af;
}
```

---

## Implementation Steps

### Step 1: Backup current CSS
```bash
cp src/ui.css src/ui.css.backup.v2
```

### Step 2: Find all problematic selectors
```bash
grep -E "^(button|dropdown|param-slider|label|slider)[^{]*\{" src/ui.css
```

### Step 3: Edit ui.css - Replace selectors

**Location:** src/ui.css

**Change 1: Dropdown selectors**

Find: `dropdown.dropdown-box {`
Replace with: `.dropdown-box {`

Find: `dropdown popup {`
Replace with: `.dropdown-options {`

**Change 2: ParamSlider selectors**

Find: `param-slider {`
Replace with: `.slider-visual {`

Find: `param-slider .fill {`
Replace with: `.slider-visual .fill {`

**Change 3: Add missing classes**

Add to end of ui.css:
```css
/* Container styles */
.body {
    height: 1s;
    child-space: 12px;
}

.dials-container {
    col-between: 30px;
    row-between: 15px;
}

/* Additional spacing classes */
.tabs-header {
    height: auto;
    col-between: 8px;
}

.tabs-content {
    height: 1s;
}

/* Meter label specific styles */
.meter-label-in,
.meter-label-out,
.meter-label-gr {
    font-size: 11px;
    color: #9ca3af;
    text-align: center;
}
```

### Step 4: Verify CSS is valid

```bash
# Check for syntax errors
grep -c "{" src/ui.css  # Count opening braces
grep -c "}" src/ui.css  # Count closing braces
# Should be equal
```

### Step 5: Test in DAW

1. Rebuild: `cargo nih-plug bundle vxcleaner --release`
2. Load in DAW
3. Check that:
   - ✅ Header displays with title and mode buttons
   - ✅ Macro dials show with labels and values
   - ✅ Dropdowns are styled
   - ✅ Buttons have hover effects
   - ✅ Spacing/padding applied

---

## CSS Class Coverage Verification

After fixing, verify all these classes are defined:

### Layout Classes
- [x] `.app-root`
- [x] `.header`
- [x] `.footer`
- [x] `.main-view`
- [x] `.columns-container`
- [ ] `.body` ← Add

### Component Classes
- [x] `.dial-container`
- [x] `.dial-visual`
- [x] `.dial-label`
- [x] `.dial-value`
- [ ] `.dials-container` ← Add
- [x] `.slider-visual`
- [x] `.slider-container`
- [x] `.slider-label`
- [x] `.slider-value`

### Dropdown Classes
- [x] `.dropdown-row`
- [x] `.dropdown-label`
- [x] `.dropdown-box` ← Change from `dropdown.dropdown-box`
- [x] `.dropdown-selected`
- [x] `.dropdown-option`
- [x] `.dropdown-options` ← Change from `dropdown popup`

### Button Classes
- [x] `.mode-button`
- [x] `.mode-button-active`
- [x] `.tab-header`
- [x] `.tab-header-active`
- [x] `.footer-button`
- [x] `.small-button`

---

## Testing Checklist

After CSS fixes, test in DAW:

- [ ] Header visible and styled
  - [ ] Title "VxCLEANER" displays
  - [ ] Subtitle displays
  - [ ] Mode buttons visible and responsive

- [ ] Macro dials
  - [ ] 3 dials visible (CLEAN, ENHANCE, CONTROL)
  - [ ] Labels visible under each dial
  - [ ] Value displays in center of dial
  - [ ] Dials are interactive

- [ ] Meters
  - [ ] Input level meters visible on left
  - [ ] Labels (IN, GR, OUT) visible
  - [ ] Meters animate with audio

- [ ] Dropdowns
  - [ ] DSP PRESET dropdown visible and styled
  - [ ] FINAL OUTPUT dropdown visible and styled
  - [ ] Dropdowns open/close on click
  - [ ] Options selectable

- [ ] Buttons
  - [ ] Footer buttons styled
  - [ ] Buttons have hover effects
  - [ ] Buttons respond to clicks

- [ ] Spacing
  - [ ] Proper padding/margins throughout
  - [ ] Elements properly aligned
  - [ ] No overlapping elements

---

## Rollback Plan

If CSS changes break things:

```bash
# Restore from backup
cp src/ui.css.backup.v2 src/ui.css

# Or restore original if issues are worse
cp src/ui.css.backup src/ui.css

# Rebuild and test
cargo nih-plug bundle vxcleaner --release
```

---

## Root Cause Prevention

To avoid this in the future:

1. **CSS-first validation:** After any refactoring, verify CSS classes are used in code
2. **Avoid element selectors:** Use class-based selectors only
3. **CSS audit checklist:** After code changes, run:
   ```bash
   # Classes in code but not in CSS
   grep -r "\.class(" src/ui/ | grep -o '"[^"]*"' | sort -u > /tmp/code-classes.txt
   grep -o "^\.[a-z-]*" src/ui.css | sort -u > /tmp/css-classes.txt
   comm -23 /tmp/code-classes.txt /tmp/css-classes.txt  # Classes in code but not CSS
   ```

---

## Time Estimate

- **CSS edits:** 10-15 minutes
- **Rebuild and test:** 5 minutes
- **DAW testing:** 10-15 minutes
- **Total:** ~30 minutes

---

## Success Criteria

✅ UI renders with proper styling
✅ All major components visible:
  - Header with title and mode buttons
  - Macro dials with labels and values
  - Dropdowns styled and functional
  - Buttons with hover effects
  - Proper spacing and alignment

✅ No visual regressions
✅ Ready to proceed with feature implementation

---

**Next Steps After Fix:**
1. Apply CSS changes
2. Rebuild bundle
3. Test in DAW
4. Verify visual appearance matches design spec
5. If good, proceed to UI_VALIDATION_CHECKLIST.md
