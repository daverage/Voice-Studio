# Dead Code & Unused Variables - Decision Guide

**Date:** 2026-01-30
**Items to Resolve:** 2 (build_advanced_tabs, published_at)
**Category:** Code cleanup warnings (no functional impact)

---

## Item #1: build_advanced_tabs Function

### Current State

**Definition:** src/ui/advanced.rs, lines 217-267

```rust
pub fn build_advanced_tabs<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
    meters: Arc<Meters>,
) -> Handle<'a, VStack> {
    VStack::new(cx, |cx| {
        // Tab Headers
        HStack::new(cx, |cx| {
            // ... button code ...
        })
        .class("tabs-header");

        // Tab Content
        Binding::new(cx, VoiceStudioData::advanced_tab, |cx, tab_lens| {
            let current_tab = tab_lens.get(cx);
            match current_tab {
                AdvancedTab::CleanRepair => {
                    build_clean_repair_tab(cx, params.clone(), gui.clone(), meters.clone());
                },
                AdvancedTab::ShapePolish => {
                    build_shape_polish_tab(cx, params.clone(), gui.clone());
                }
            }
        })
        .class("tabs-content");
    })
    .class("advanced-tabs-container")
}
```

**Export:** src/ui/mod.rs, line 10

```rust
pub use advanced::{build_clean_repair_tab, build_shape_polish_tab, build_advanced_tabs};
```

**Usage in Codebase:**
```bash
grep -r "build_advanced_tabs" src/
# Result: Only in definition (advanced.rs) and export (mod.rs)
# NOT called anywhere
```

### Analysis

**What does it do?**
- Creates a unified tab interface (headers + content binding)
- Tab headers: Interactive buttons for "Clean & Repair" and "Shape & Polish"
- Tab content: Binding that switches between tab content based on state
- Essentially combines the functionality already split across build_clean_repair_tab and build_shape_polish_tab

**Why is it dead?**
- The unified builder creates the exact same UI structure inline in build_body (layout.rs)
- Both `build_clean_repair_tab` and `build_shape_polish_tab` are called directly from layout.rs
- build_advanced_tabs is never invoked
- Appears to be a refactoring artifact or an abandoned refactoring approach

**Impact of removing:**
- Zero impact to UI or functionality
- build_clean_repair_tab and build_shape_polish_tab still available and used
- Tab switching still works (handled in layout.rs Binding)
- Code becomes cleaner and smaller

**Impact of keeping:**
- Unused function takes up lines
- Adds to API surface unnecessarily
- Confuses future developers ("why are there two ways to build tabs?")
- Compiler warning every build

### Options & Recommendation

**OPTION A: Remove** ✅ RECOMMENDED
- Delete function from advanced.rs (250 lines)
- Remove from mod.rs export
- Rationale: Dead code, confusing, no value
- Risk: Very low (function not used anywhere)

**OPTION B: Mark #[allow(dead_code)]**
- Add attribute above function definition
- Keep for "future use" or "reference implementation"
- Rationale: Might be useful someday
- Risk: Accumulates unused code over time

**OPTION C: Keep and integrate**
- Use this function as the canonical "advanced tabs builder"
- Refactor layout.rs to call this instead of duplicating logic
- Rationale: DRY principle, single source of truth
- Risk: Requires refactoring, more complex changes

### Recommendation: **OPTION A - REMOVE**

**Reasoning:**
1. Not used anywhere
2. Functions it calls (build_clean_repair_tab, build_shape_polish_tab) are the canonical versions
3. Tab structure is already correctly built in layout.rs
4. Cleaner codebase without it
5. Easy to restore from git if needed later

**Implementation:**

1. Delete entire function from src/ui/advanced.rs (lines ~217-267)
2. Remove from src/ui/mod.rs export:
   ```rust
   // OLD
   pub use advanced::{build_clean_repair_tab, build_shape_polish_tab, build_advanced_tabs};

   // NEW
   pub use advanced::{build_clean_repair_tab, build_shape_polish_tab};
   ```
3. Verify: `cargo build --release 2>&1 | grep build_advanced_tabs` (no results)

---

## Item #2: RemoteRelease::published_at

### Current State

**Struct Definition:** src/version.rs, lines 79-85

```rust
#[derive(Debug, Clone)]
pub struct RemoteRelease {
    pub version: Version,
    pub url: String,
    pub tag: String,
    pub published_at: Option<String>,  // ← Dead field
}
```

**Deserialization Source:** src/version.rs, lines 170-175

```rust
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    published_at: Option<String>,  // ← Fetched from API
}
```

**Population:** src/version.rs, lines 151-156

```rust
Ok(RemoteRelease {
    version: parsed_version,
    url: release.html_url,
    tag: release.tag_name,
    published_at: release.published_at,  // ← Stored but never used
})
```

**Usage in Codebase:**
```bash
grep -r "published_at" src/ | grep -v "pub published_at\|published_at:"
# Result: Only in assignment, never read or displayed
```

### Analysis

**What is it?**
- Optional string field containing release publication date from GitHub API
- Format: ISO 8601 timestamp (e.g., "2026-01-30T10:00:00Z")
- Fetched from GitHub but never displayed or used

**Why is it unused?**
- VersionUiState (the UI state) doesn't include a date field (lines 20-25)
- Only displays: version, detail (with version and tag), status, and URL
- published_at could show "Released on 2026-01-30" but doesn't currently
- Appears to be "fetched for future use"

**Impact of removing:**
- Slightly smaller struct (one field less)
- Slightly smaller JSON deserialization (one field less)
- Network payload unchanged (API still returns it)
- UI unchanged (wasn't displayed anyway)
- Code cleaner

**Impact of keeping:**
- Adds unnecessary data to memory model
- Confuses future developers ("where is this used?")
- Takes up lines and struct space
- Compiler warning every build

### Options & Recommendation

**OPTION A: Remove** ✅ RECOMMENDED
- Delete field from RemoteRelease struct
- Delete field from GitHubRelease deserialization struct
- Rationale: Unused, confusing
- Risk: Very low (never displayed or accessed)

**OPTION B: Add to UI and display**
- Update VersionUiState to include published_at field
- Display in version UI: "Latest release: v1.0.0 (released 2026-01-30)"
- Better UX for users (know how fresh the release is)
- Rationale: Useful information for users
- Risk: Requires UI changes, needs design consideration

**OPTION C: Keep but flag as intentionally unused**
- Add `#[allow(dead_code)]` to the field
- Keep in case future features need it
- Rationale: Low cost, future-proofing
- Risk: Accumulates unused code

### Recommendation: **OPTION A - REMOVE**

**Reasoning:**
1. Not used anywhere in code
2. Not displayed in UI
3. GitHub API still provides it (we're not "losing" the data, just not storing it)
4. Can be added back if feature is requested (easy change)
5. Cleaner code without it

**Alternative Consideration:** If you want to show users "Last updated X days ago" or display the release date in the version UI, OPTION B is worth pursuing. But requires UI design decisions.

**Implementation:**

1. Remove from src/version.rs, line 84:
   ```rust
   // OLD
   pub struct RemoteRelease {
       pub version: Version,
       pub url: String,
       pub tag: String,
       pub published_at: Option<String>,  // ← DELETE THIS LINE
   }

   // NEW
   pub struct RemoteRelease {
       pub version: Version,
       pub url: String,
       pub tag: String,
   }
   ```

2. Remove from src/version.rs, line 174:
   ```rust
   // OLD
   #[derive(Debug, Deserialize)]
   struct GitHubRelease {
       tag_name: String,
       html_url: String,
       published_at: Option<String>,  // ← DELETE THIS LINE
   }

   // NEW
   #[derive(Debug, Deserialize)]
   struct GitHubRelease {
       tag_name: String,
       html_url: String,
   }
   ```

3. Remove from src/version.rs, line 155:
   ```rust
   // OLD
   Ok(RemoteRelease {
       version: parsed_version,
       url: release.html_url,
       tag: release.tag_name,
       published_at: release.published_at,  // ← DELETE THIS LINE
   })

   // NEW
   Ok(RemoteRelease {
       version: parsed_version,
       url: release.html_url,
       tag: release.tag_name,
   })
   ```

4. Verify: `cargo build --release 2>&1 | grep published_at` (no results)

---

## Decision Summary

| Item | Recommendation | Action | Time | Risk |
|------|---|---|---|---|
| build_advanced_tabs | Remove | Delete function + export | 2 min | Very Low |
| published_at | Remove | Delete field + deserialization | 5 min | Very Low |

---

## Implementation Steps (If Choosing "Remove" for Both)

### Step 1: Remove build_advanced_tabs (2 minutes)

**File:** src/ui/advanced.rs

**Action 1:** Delete function definition (lines ~217-267)
```rust
// DELETE ENTIRE FUNCTION:
pub fn build_advanced_tabs<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
    meters: Arc<Meters>,
) -> Handle<'a, VStack> {
    // ... all this code ...
}
```

**Action 2:** Update export in src/ui/mod.rs (line 10)
```rust
// OLD
pub use advanced::{build_clean_repair_tab, build_shape_polish_tab, build_advanced_tabs};

// NEW
pub use advanced::{build_clean_repair_tab, build_shape_polish_tab};
```

### Step 2: Remove published_at (5 minutes)

**File:** src/version.rs

**Action 1:** Update RemoteRelease struct (line 84)
```rust
// OLD
#[derive(Debug, Clone)]
pub struct RemoteRelease {
    pub version: Version,
    pub url: String,
    pub tag: String,
    pub published_at: Option<String>,
}

// NEW
#[derive(Debug, Clone)]
pub struct RemoteRelease {
    pub version: Version,
    pub url: String,
    pub tag: String,
}
```

**Action 2:** Update GitHubRelease deserialization struct (line 174)
```rust
// OLD
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    published_at: Option<String>,
}

// NEW
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
}
```

**Action 3:** Update RemoteRelease construction (line 155)
```rust
// OLD
Ok(RemoteRelease {
    version: parsed_version,
    url: release.html_url,
    tag: release.tag_name,
    published_at: release.published_at,
})

// NEW
Ok(RemoteRelease {
    version: parsed_version,
    url: release.html_url,
    tag: release.tag_name,
})
```

### Step 3: Verify Build (2 minutes)

```bash
cargo build --release 2>&1 | tail -10
# Expected: Finished release, zero errors, zero warnings
```

### Step 4: Proceed to DAW Testing (Ongoing)

After verification, proceed directly to:
```bash
open UI_VALIDATION_CHECKLIST.md
# Load plugin in DAW and test systematically
```

---

## Rollback

If anything breaks:

```bash
# Restore from backup
git checkout -- src/ui/advanced.rs src/ui/mod.rs src/version.rs

# Or manually re-add the code from this document
```

---

## Total Time Estimate

- **Both removals:** ~10 minutes
- **Verification:** ~2 minutes
- **Total:** ~12 minutes

---

## After Decision

Once you decide on these two items:

**If removing both:** Execute steps above, verify build, proceed to UI_VALIDATION_CHECKLIST.md for DAW testing

**If keeping some:** Mark with `#[allow(dead_code)]` and proceed to DAW testing

**Next Phase:** UI_VALIDATION_CHECKLIST.md (DAW/visual quality assurance)

---

**Decision Required From User:**
1. Remove `build_advanced_tabs`? (Recommend: YES)
2. Remove `published_at`? (Recommend: YES)
