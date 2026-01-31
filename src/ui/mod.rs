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
#[allow(unused_imports)]
pub use state::{VoiceStudioData, set_macro_mode, sync_advanced_from_macros, AdvancedTab, AdvancedTabEvent};
#[allow(unused_imports)]
pub use components::{
    create_button, create_toggle_button, create_momentary_button,
    create_slider, create_macro_dial, create_dropdown, create_dsp_preset_dropdown,
    SliderVisuals, DialVisuals, ParamId
};
#[allow(unused_imports)]
pub use layout::{build_header, build_footer, build_body, build_levels, build_macro, build_output};
#[allow(unused_imports)]
pub use advanced::{build_clean_repair_tab, build_shape_polish_tab};
#[allow(unused_imports)]
pub use meters::{LevelMeter, NoiseFloorLeds, MeterType, NoiseLearnQualityMeter};

// Main UI entry point
pub use layout::build_ui;
