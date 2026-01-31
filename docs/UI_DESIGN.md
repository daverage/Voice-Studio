# VST UI Design Specification (Vizia-Based)

This document defines the layout, structure, and behavior of a modern, responsive VST plugin interface using the Vizia GUI framework. It avoids embedded assets, focuses on implementation-agnostic design intent, and is optimized for maintainability and clarity.

---

## 1. Overview

### Goals

* Professional, modular UI layout
* Flat, modern aesthetic (non-skeuomorphic)
* Responsive to host window resizing
* Simple and Advanced modes
* Usable, legible, and intuitive

### Scope

Applies to VST plugin UIs built with Vizia (`ui.rs` and `ui.css`), integrated into plugin hosts via `baseview` or similar windowing backends.

---

## 2. Layout Architecture

### Top-Level Zones

```
+---------------------------------------------+
| HEADER BAR (plugin title, resize handle)     |
+---------------------------------------------+
| LEFT PANEL (input/settings)   | RIGHT PANEL |
| (Advanced toggles here)       | (main DSP)  |
+---------------------------------------------+
| FOOTER (meters, status, logo)               |
+---------------------------------------------+
```

### Flex Behavior

* Panels scale with container size
* Use `Stretch(1.0)` and `Percentage` sizing for width/height
* Minimum plugin size: 640x360
* Optional max size cap: 1920x1080

### Layout Containers

* Use `HStack` for horizontal panel layouts
* Use `VStack` for vertical stacking (e.g., label + knob)
* For grids of controls: use `Wrap` or `Grid` when supported

---

## 3. Modes: Simple vs. Advanced

### Toggle Control

* Location: top-right or left-panel
* Label: `Show Advanced ▸` / `Hide Advanced ▾`
* UI Behavior:

  * Simple mode: only core controls shown
  * Advanced mode: shows hidden expert controls, graphs, sub-panels

### Implementation Note (for devs)

* In Rust: bind `show_advanced: bool` to layout visibility
* Apply conditional rendering or `Display::None` via CSS

---

## 4. UI Components

### 4.1 Knob

* Circular, flat shaded
* Optional ring for value indication
* Label below

### 4.2 Slider

* Horizontal or vertical bar
* Value label floats during interaction

### 4.3 Button

* Flat rectangle with optional icon
* Optional active/inactive state

### 4.4 Meter

* Vertical bar with LED-style segments
* Optionally peak-hold or color-coded

### 4.5 Dropdown / Select

* Label and arrow
* Expands to list, keyboard navigable

---

## 5. Styling Guidelines (`ui.css`)

### Color Theme

* Background: dark gray `#202020`
* Foreground: light gray `#e0e0e0`
* Accent (active states): `#3fa7ff`
* Error states: `#ff4d4d`

### Typography

* Font: Inter, Roboto, or system default
* Sizes:

  * Labels: 12px
  * Section headers: 16px bold

### Spacing

* Padding between controls: 8px
* Panel margin: 12px

### States

* `:hover` → slightly brighter (10%)
* `:active` → pressed inset

### Accessibility

* All color pairs must meet WCAG AA contrast
* Controls must be operable via keyboard focus

---

## 6. Responsiveness

### Resizing Behavior

* Plugin should scale with host window
* Use stretch factors in all layout containers
* Maintain minimum spacing; shrink only when constrained

### Optional Layout Tweaks

* Collapse advanced panels at small sizes
* Use scrollable sub-panels if overflowed

---

## 7. UX Enhancements

### Feedback

* Show tooltip on hover for all interactive controls
* On value change: float current value for 1–2 seconds

### Animations

* Advanced panel slide/fade in/out (optional)
* Smooth meter movement (not stepped)

---

## 8. File & Code Structure

### File Layout

```
src/
├── main.rs
├── ui.rs          # Vizia UI layout code
├── components/    # Reusable view modules
│   ├── knob.rs
│   └── meter.rs
styles/
└── ui.css         # Vizia CSS-style stylesheet
assets/
└── images/        # PNG/SVG assets (referenced, not embedded)
```

### Components

* `KnobView`, `SliderView`, `MeterView`: reusable structs
* Encapsulate layout and bindings in one module per control
* Prefer declarative construction (`View` trait)

---

## 9. Build Targets & Constraints

* Minimum plugin host support: Windows/macOS (Linux optional)
* DPI-awareness: must render sharply at 1x, 1.5x, 2x scales
* Compile time target: <5s for UI alone

---

## 10. Future Considerations

* Theme switching (light/dark)
* MIDI learn overlay UI
* Touchscreen optimization
* Plugin window resizing by dragging corners

---

End of specification.
