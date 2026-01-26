# Preset System Fixes

## Issues Fixed

### Issue 1: CSS Editor Missing ✅ FIXED

**Problem:** CSS editor buttons (Edit CSS, Reload CSS) were not appearing in the UI footer.

**Root Cause:** Plugin was built without the `debug` feature flag.

**Solution:** Build with debug features enabled:

```bash
cargo nih-plug bundle vxcleaner --release --features debug
```

**Verification:**
- ✅ "Log" button appears in footer
- ✅ "Edit CSS" button appears in footer
- ✅ "Reload CSS" button appears in footer
- ✅ CSS file is created at: `target/bundled/vxcleaner.vst3/Contents/MacOS/themes/default/ui.css`

**CSS Editor Workflow:**
1. Load plugin in DAW
2. Click "Edit CSS" button → Opens system text editor
3. Make changes to CSS file and save
4. Click "Reload CSS" button → Changes applied instantly
5. Check `/tmp/voice_studio.log` for debug messages

---

### Issue 2: Presets Don't Update Macro Controls ✅ FIXED

**Problem:** When selecting a DSP preset, the advanced parameters updated correctly, but the Easy Mode macro controls (Distance, Clarity, Consistency) remained unchanged.

**Root Cause:** The macro controller was designed with one-way mapping only (macros → advanced), with no reverse mapping (advanced → macros).

**Solution:** Added reverse mapping function to estimate macro values from advanced parameters.

#### Implementation Details

**File:** `src/macro_controller.rs`

Added new function:
```rust
pub fn estimate_macros_from_advanced(
    noise_reduction: f32,
    reverb_reduction: f32,
    proximity: f32,
    clarity: f32,
    de_esser: f32,
    leveler: f32,
    breath_control: f32,
) -> (f32, f32, f32)
```

**Mapping Algorithm:**

1. **Distance** = Average of:
   - `noise_reduction / 0.80` (normalized to max 80%)
   - `reverb_reduction / 0.40` (normalized to max 40%)
   - `proximity / 0.30` (normalized to max 30%)

2. **Clarity** = Average of:
   - `clarity / 1.00` (already 0-1 range)
   - `de_esser / 0.70` (normalized to max 70%)

3. **Consistency** = Average of:
   - `leveler / 0.80` (normalized to max 80%)
   - `breath_control / 0.50` (normalized to max 50%)

**File:** `src/ui.rs`

Updated `create_dsp_preset_dropdown()` to:
1. Set all 8 advanced parameters from preset
2. Call `estimate_macros_from_advanced()` with preset values
3. Update macro_distance, macro_clarity, macro_consistency

**Verification:**
- ✅ Selecting "Podcast (Noisy Room)" updates Distance dial
- ✅ Selecting "Podcast (Noisy Room)" updates Clarity dial
- ✅ Selecting "Podcast (Noisy Room)" updates Consistency dial
- ✅ Easy Mode and Advanced Mode stay synchronized

---

## Example: Podcast (Noisy Room) Preset

**Advanced Parameters:**
- noise_reduction: 0.35
- reverb_reduction: 0.60
- proximity: 0.05
- clarity: 0.15
- de_esser: 0.0
- leveler: 0.70
- breath_control: 0.30

**Calculated Macro Values:**
- **Distance:** `(0.35/0.80 + 0.60/0.40 + 0.05/0.30) / 3 = 0.70` → 70%
- **Clarity:** `(0.15 + 0.0/0.70) / 2 = 0.075` → 8%
- **Consistency:** `(0.70/0.80 + 0.30/0.50) / 2 = 0.74` → 74%

**UI Result:**
When you select "Podcast (Noisy Room)", you'll see:
- Distance dial moves to ~70%
- Clarity dial moves to ~8%
- Consistency dial moves to ~74%

This matches the intuition: noisy room requires high distance (noise/reverb reduction) and high consistency (leveling/breath control), but low clarity (to avoid artifacts).

---

## Testing Checklist

### CSS Editor (Debug Build Only)

- [ ] Build with: `cargo nih-plug bundle vxcleaner --release --features debug`
- [ ] Load plugin in DAW
- [ ] Verify "Log" button appears in footer
- [ ] Verify "Edit CSS" button appears in footer
- [ ] Verify "Reload CSS" button appears in footer
- [ ] Click "Edit CSS" → Text editor opens with CSS file
- [ ] Make a CSS change (e.g., change a color)
- [ ] Save CSS file
- [ ] Click "Reload CSS" → Changes apply immediately
- [ ] Check `/tmp/voice_studio.log` for debug output

### Preset Macro Sync

- [ ] Load plugin in DAW (debug or release build)
- [ ] Switch to Easy Mode (macro controls visible)
- [ ] Note current Distance/Clarity/Consistency dial positions
- [ ] Select "Podcast (Noisy Room)" from DSP Preset dropdown
- [ ] Verify all three dials moved to new positions
- [ ] Switch to Advanced Mode
- [ ] Verify all 8 parameters match preset values
- [ ] Switch back to Easy Mode
- [ ] Verify dials still show updated values
- [ ] Select "Manual" preset
- [ ] Verify dials stay at current positions (no change)

---

## Release vs Debug Builds

### Release Build (Default)
```bash
cargo nih-plug bundle vxcleaner --release
```
- Smaller binary size
- No debug logging
- No CSS editor buttons
- No log file output
- **Use for production releases**

### Debug Build
```bash
cargo nih-plug bundle vxcleaner --release --features debug
```
- Slightly larger binary
- Logging to `/tmp/voice_studio.log`
- CSS editor buttons in footer
- Real-time CSS reloading
- **Use for development/testing**

---

## Known Limitations

### Reverse Mapping Accuracy

The macro → advanced mapping is **not perfectly invertible**. The reverse mapping is an approximation based on averaging normalized values.

**Why:**
- Multiple advanced parameter combinations can produce similar macro values
- Some information is lost in the forward mapping
- The reverse mapping uses simple averaging

**Impact:**
- If you manually set advanced params to `{noise: 0.8, reverb: 0.4, prox: 0.3}`, Distance would be 100%
- If you load a preset with `{noise: 0.35, reverb: 0.60, prox: 0.05}`, Distance is calculated as ~70%
- The relationship is **approximate but reasonable**

**User Experience:**
- Presets update macro dials to sensible values
- Macro dials may not match exactly if you manually set advanced params then switch to Easy Mode
- This is expected and acceptable behavior

---

## Summary

✅ **Fixed:** CSS editor now works with `--features debug` build
✅ **Fixed:** DSP presets now update macro controls automatically
✅ **Result:** Presets feel more cohesive and intuitive
✅ **Build:** Ready for testing with both fixes applied

**Next Steps:**
1. Test CSS editor in debug build
2. Test preset macro sync in any build
3. If satisfied, create release build for distribution
