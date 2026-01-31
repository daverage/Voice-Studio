Here is a comprehensive documentation of the CSS grammar and parsing rules for `nih-plug` (via the Vizia GUI framework).

***

# NIH-Plug (Vizia) CSS Reference

This document outlines how CSS is parsed and applied within the `nih-plug` ecosystem. Since `nih-plug` delegates GUI rendering to **Vizia**, these rules represent the specific subset of CSS3 supported by Vizia's style engine and the [Morphorm](https://github.com/vizia/morphorm) layout system.

## 1. The Parsing Pipeline

It helps to understand strictly *where* the parsing happens so you know which limitations apply.

1.  **Lexer (Tokenization):**
    *   **Engine:** `cssparser` crate (Mozilla's compliant CSS parser).
    *   **Role:** Breaks your string into tokens (`Ident`, `Hash`, `Dimension`, `Function`).
    *   **Behavior:** It is standard-compliant. Comments `/* */` are stripped here. Syntax errors (like missing semicolons) are caught here.

2.  **Parser (Grammar):**
    *   **Engine:** `vizia_style` crate.
    *   **Role:** Iterates over tokens and matches them against valid **properties** and **selectors**.
    *   **Behavior:** If a property name is unknown, it is typically ignored/dropped with a warning in the console.

3.  **Layout Engine:**
    *   **Engine:** `Morphorm`.
    *   **Role:** Takes the parsed style values and calculates X/Y coordinates and geometry.

---

## 2. Supported Selectors

Vizia supports a standard subset of CSS selectors. Complex logical combinations (like `:not()` or `nth-child`) are generally **not** supported.

| Selector Type | Syntax | Description |
| :--- | :--- | :--- |
| **Type** | `knob`, `label`, `button` | Matches the Rust element name (struct name). |
| **Class** | `.my-class` | Matches elements where `.class("my-class")` was called in Rust. |
| **ID** | `#header` | Matches elements where `.id("header")` was called in Rust. |
| **Universal** | `*` | Matches all elements. |
| **Descendant** | `div label` | Matches a `label` anywhere inside a `div`. |
| **Child** | `div > label` | Matches a `label` that is a direct child of a `div`. |
| **Multiple** | `div, span` | Applies rules to both `div` and `span`. |

### Pseudo-Classes (State)
These are critical for audio plugins (e.g., hovering a parameter, turning a knob).

| Pseudo-class | Trigger |
| :--- | :--- |
| `:hover` | Mouse cursor is over the element. |
| `:active` | Mouse button is pressed down on the element. |
| `:focus` | Element has keyboard focus. |
| `:disabled` | Element is disabled/grayed out. |
| `:checked` | Element is toggled "on" (checkboxes, toggle buttons). |

---

## 3. Property Reference

Values generally accept:
*   **Length:** `10px` (pixels), `10` (implied pixels).
*   **Percent:** `50%` (percentage of parent dimension).
*   **Auto:** `auto`.

### A. Layout & Positioning (Flexbox-ish)
Vizia uses a system similar to Flexbox.

| Property | Values | Notes |
| :--- | :--- | :--- |
| `display` | `flex`, `none` | `none` hides the element completely. |
| `position-type` | `self-directed`, `parent-directed` | `self-directed` acts like CSS `absolute`. |
| `top`, `left` | `<length>`, `<percentage>`, `auto` | |
| `right`, `bottom`| `<length>`, `<percentage>`, `auto` | |
| `width`, `height`| `<length>`, `<percentage>`, `auto` | `auto` usually hugs content. |
| `min-width`... | `<length>`, `<percentage>`, `none` | (Also max-width/height). |

### B. Spacing & Alignment
| Property | Values | Equivalent / Notes |
| :--- | :--- | :--- |
| `child-space` | `<length>` | **Padding.** Sets space on all sides inside the container. |
| `child-left`... | `<length>` | Specific padding (also `child-right`, `child-top`...). |
| `row-between` | `<length>` | **Gap** (Horizontal). Space between rows. |
| `col-between` | `<length>` | **Gap** (Vertical). Space between columns. |
| `flex-direction` | `row`, `column`, `row-reverse`... | Defines main axis direction. |
| `align-items` | `stretch`, `center`, `flex-start`... | Alignment on the cross axis. |
| `justify-content`| `flex-start`, `center`, `space-between`... | Alignment on the main axis. |

### C. Visual Appearance
| Property | Values | Notes |
| :--- | :--- | :--- |
| `background-color` | `<color>` | See Color Formats below. |
| `border-radius` | `<length>`, `<percentage>` | Supports single value (all corners) or corner-specific. |
| `border-width` | `<length>` | |
| `border-color` | `<color>` | |
| `outline-width` | `<length>` | |
| `outline-color` | `<color>` | |
| `opacity` | `0.0` to `1.0` | |
| `visibility` | `visible`, `hidden` | Unlike `display: none`, element still takes up space. |

### D. Typography
| Property | Values |
| :--- | :--- |
| `color` | `<color>` (Text color) |
| `font-size` | `<length>` |
| `font-family` | `"<string>"` (e.g., `"sans-serif"`, `"Open Sans"`) |
| `font-weight` | `normal`, `bold`, number (`100`–`900`) |
| `font-style` | `normal`, `italic` |
| `text-align` | `left`, `center`, `right`, `justify` |

### E. Transitions (Animations)
Vizia supports standard CSS transitions for smooth UI updates.

| Property | Syntax |
| :--- | :--- |
| `transition` | `[property] [duration] [timing-func] [delay]` |

**Example:**
```css
transition: background-color 200ms ease-out;
```

---

## 4. Data Types & Formats

### Colors
The parser accepts standard CSS color formats.
1.  **Named:** `red`, `blue`, `transparent`, `cornflowerblue`.
2.  **Hex:** `#RGB`, `#RRGGBB`, `#RRGGBBAA` (Alpha supported).
3.  **Functions:**
    *   `rgb(255, 0, 0)`
    *   `rgba(255, 0, 0, 0.5)`
    *   `hsl(0, 100%, 50%)`

### Dimensions
1.  **Pixels:** `10px` or just `10`.
2.  **Percentage:** `50%`.
3.  **Stretch:** `1s` (Specific to Vizia/Morphorm, equivalent to `flex-grow: 1`).

---

## 5. Full Example

Here is a `style.css` file you might find in an `nih-plug` project, annotating the parsed rules.

```css
/* 
   Matches the Editor struct wrapper.
   Sets up a flex column layout.
*/
editor {
    width: 300px;
    height: auto;
    background-color: #323232;
    font-family: "Roboto";
    child-space: 10px; /* Padding inside the editor */
    col-between: 5px;  /* Vertical Gap between children */
}

/* 
   Matches a hypothetical custom widget named "Knob".
*/
knob {
    width: 50px;
    height: 50px;
    background-color: #1a1a1a;
    border-radius: 50%;
    border-width: 2px;
    border-color: #555;
    
    /* Animation definition */
    transition: border-color 100ms ease;
}

/* 
   State change: When mouse hovers over knob
*/
knob:hover {
    border-color: #ff9e42;
    background-color: #2a2a2a;
}

/* 
   Matches a label inside a parameter group
*/
.param-group label {
    font-size: 12px;
    color: white;
    text-align: center;
}
```
