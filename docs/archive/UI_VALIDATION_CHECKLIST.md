# Voice Studio UI Consistency Validation Checklist
**Date:** 2026-01-30
**Version:** Post-UI Standardization
**Plugin:** vxcleaner v0.1.0

---

## Build Verification

- [x] Code compiles without errors
- [x] Plugin bundle created successfully (`cargo nih-plug bundle vxcleaner --release`)
- [ ] Plugin loads in DAW without crashes

---

## Visual Consistency Testing

### Header Bar
- [ ] Title "VxCLEANER" displays in white (#ffffff)
- [ ] Subtitle "Vocal Restoration" displays in muted blue-gray (#94a3b8)
- [ ] Simple/Advanced toggle buttons are visible and properly styled
- [ ] Simple button shows cyan accent (#3fa7ff) when active
- [ ] Advanced button shows cyan accent when active
- [ ] Buttons respond to hover with lighter background (#475569)

### Left Panel - Meters
- [ ] "LEVELS" header displays in gray (#9ca3af)
- [ ] Input meters (L/R) render correctly
- [ ] Gain reduction meter renders correctly
- [ ] Output meters (L/R) render correctly
- [ ] Meters are properly spaced (6px between columns, 2px between L/R pairs)
- [ ] Activity LEDs render below meters
- [ ] LEDs change color based on processing level (green/yellow/red)

### Simple Mode
- [ ] Switching to Simple mode shows three macro dials: CLEAN, ENHANCE, CONTROL
- [ ] Dial labels are bold and centered
- [ ] Dials have cyan border (#3fa7ff)
- [ ] Dial values display centered in white
- [ ] Dials respond to mouse interaction
- [ ] DSP PRESET dropdown is visible and functional

### Advanced Mode
- [ ] Switching to Advanced mode shows two tabs: "Clean & Repair" and "Shape & Polish"
- [ ] Active tab has cyan border (#3fa7ff) and white text
- [ ] Inactive tab has gray background (#2a2a2a)
- [ ] Tab headers respond to hover (lighter background)
- [ ] Tab content has dark background (#1a1a1a)

### Clean & Repair Tab
- [ ] Column 1 sliders render correctly:
  - [ ] Rumble slider with tooltip
  - [ ] Hiss slider with tooltip
  - [ ] Static Noise slider
  - [ ] Learn button (small button style)
  - [ ] Clear button (small button style)
  - [ ] Quality meter visible
- [ ] Column 2 sliders render correctly:
  - [ ] Noise Reduction with tooltip
  - [ ] De-Verb with tooltip
  - [ ] Breath Control with tooltip
- [ ] All slider labels are right-aligned, width 120px
- [ ] All sliders have cyan border (#3fa7ff)
- [ ] Slider values display centered in white
- [ ] Learn/Clear buttons respond to press/release (momentary behavior)

### Shape & Polish Tab
- [ ] Column 1 sliders:
  - [ ] Proximity with tooltip
  - [ ] Clarity with tooltip
- [ ] Column 2 sliders:
  - [ ] De-Ess slider
  - [ ] Leveler slider
- [ ] All sliders follow same styling as Clean & Repair tab

### Output Section
- [ ] "OUTPUT" header displays in cyan (#3fa7ff)
- [ ] Gain slider renders correctly
- [ ] FINAL OUTPUT dropdown is visible and functional
- [ ] Output section positioned correctly below main content

### Dropdowns
- [ ] DSP PRESET dropdown:
  - [ ] Label "DSP PRESET" displays correctly
  - [ ] Dropdown box has cyan border
  - [ ] Current selection shows in white
  - [ ] Clicking opens popup menu
  - [ ] Options list on dark background (#1a1a1a)
  - [ ] Hover highlights option in cyan
  - [ ] Selecting option closes popup and updates value
- [ ] FINAL OUTPUT dropdown:
  - [ ] Same visual style as DSP PRESET dropdown
  - [ ] All options visible and selectable

### Footer Bar
- [ ] Version info displays on left (11px gray text)
- [ ] Help button visible and styled consistently
- [ ] Reset button visible and styled consistently
- [ ] Debug buttons visible (if built with `--features debug`):
  - [ ] Log button
  - [ ] Edit CSS button
  - [ ] Reload CSS button
- [ ] All footer buttons use same style (gray background, cyan on hover/active)

---

## Interaction Testing

### Button Consistency
- [ ] All standard buttons respond to hover (lighter background)
- [ ] All standard buttons respond to click (cyan accent)
- [ ] Mode toggle buttons show active state correctly
- [ ] Tab buttons show active state correctly
- [ ] Learn/Clear buttons work as momentary (only active while pressed)

### Slider Consistency
- [ ] All sliders in Advanced mode follow same visual pattern
- [ ] All sliders show value label centered over slider bar
- [ ] All sliders respond to mouse drag
- [ ] All sliders disable macro mode when adjusted

### Dropdown Consistency
- [ ] Both dropdowns have identical visual appearance
- [ ] Both dropdowns open/close with same animation
- [ ] Both dropdowns highlight options on hover
- [ ] Both dropdowns close after selection

### Mode Switching
- [ ] Switching from Simple to Advanced preserves meter visibility
- [ ] Switching from Advanced to Simple preserves meter visibility
- [ ] Parameter values persist when switching modes
- [ ] Macro mode parameter is set correctly when switching

---

## Color Accuracy Testing

### Background Colors
- [ ] Main background: #202020 (dark gray)
- [ ] Header/Footer: #2a2a2a (slightly lighter gray)
- [ ] Dial/Slider backgrounds: #1a1a1a (darkest gray)
- [ ] Tab content: #1a1a1a (darkest gray)

### Foreground Colors
- [ ] Primary text: #e0e0e0 (light gray)
- [ ] Titles: #ffffff (white)
- [ ] Subtitles/labels: #94a3b8 or #cbd5e1 (muted blue-gray)
- [ ] Accent (active states, borders): #3fa7ff (cyan)
- [ ] OUTPUT label: #3fa7ff (cyan accent)

### Button States
- [ ] Default: #334155 (slate gray)
- [ ] Hover: #475569 (lighter slate)
- [ ] Active: #3fa7ff (cyan)

---

## Layout Verification

### Spacing
- [ ] Main view has 24px padding on all sides
- [ ] Header height is 68px
- [ ] Footer height is 40px
- [ ] Left meter panel width is 180px
- [ ] Columns have 40px horizontal spacing
- [ ] Tab columns have 24px horizontal spacing
- [ ] Sliders have 12px vertical spacing

### Alignment
- [ ] All slider labels are right-aligned and same width (120px)
- [ ] All dropdown labels are same width (120px)
- [ ] Meter labels are centered
- [ ] Dial labels are centered
- [ ] Value displays are centered in their containers

---

## Edge Cases

- [ ] Plugin window resizes correctly (min 640x360)
- [ ] All text is legible at default size
- [ ] No elements overlap or clip
- [ ] No scrollbars appear unexpectedly
- [ ] Parameter automation from DAW updates UI correctly
- [ ] Preset loading updates all UI elements

---

## Issues Found

Document any issues discovered during testing below:

### Critical Issues
_None found - add any critical issues here_

### Minor Issues
_None found - add any minor issues here_

### Visual Inconsistencies
_None found - add any inconsistencies here_

---

## Test Environment

**DAW:**
**OS:**
**Screen Resolution:**
**Plugin Format:** (VST3 / CLAP)
**Build Date:**
**Built With Debug:** Yes / No

---

## Sign-off

**Tested by:**
**Date:**
**Status:** PASS / FAIL / NEEDS REVISION

---

**Note:** This checklist should be completed by loading the plugin in a DAW and visually inspecting each element. All automation and UI updates should be verified with actual audio processing.
