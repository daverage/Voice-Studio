use crate::macro_controller::compute_targets_from_macros;
use crate::meters::Meters;
use crate::VoiceParams;
use nih_plug::params::Param;
use nih_plug::prelude::{GuiContext, ParamSetter};
use nih_plug_vizia::vizia::binding::Map;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::vizia::vg;
use nih_plug_vizia::widgets::param_base::ParamWidgetBase;
use nih_plug_vizia::widgets::*;
use std::sync::{Arc, Mutex};

// --- CSS STYLING ---
const STYLE: &str = r#"
    .main-view {
        background-color: #0f172a; /* Slate 900 */
        font-family: 'Roboto', sans-serif;
        color: #e2e8f0;
    }

    .app-root {
        background-color: #0f172a;
    }

    .header {
        height: 56px;
        background-color: #1e293b;
        border-bottom: 1px solid #334155;
        col-between: 12px;
        padding-left: 20px;
        padding-right: 20px;
        child-top: 1s;
        child-bottom: 1s;
    }

    .header-title {
        font-size: 16px;
        font-weight: bold;
        color: #ffffff;
    }

    .header-sub {
        font-size: 11px;
        color: #94a3b8;
    }

    .header-controls {
        child-space: 1s;
        col-between: 8px;
        padding-top: 6px;
    }

    .preview-button {
        background-color: #0f172a;
        border: 1px solid #334155;
        border-radius: 6px;
        color: #e2e8f0;
        font-size: 11px;
        padding-left: 10px;
        padding-right: 10px;
        padding-top: 6px;
        padding-bottom: 6px;
    }

    .preview-button:checked {
        background-color: #1d4ed8;
        border: 1px solid #3b82f6;
        color: #ffffff;
    }

    .preview-button-active {
        background-color: #1d4ed8;
        border: 1px solid #3b82f6;
        color: #ffffff;
    }

    .preview-toggle {
        background-color: #0f172a;
        border: 1px solid #334155;
        border-radius: 4px;
        color: #94a3b8;
        font-size: 9px;
        padding-left: 6px;
        padding-right: 6px;
        padding-top: 3px;
        padding-bottom: 3px;
    }

    .preview-toggle-active {
        background-color: #f97316;
        border: 1px solid #fb923c;
        color: #ffffff;
        font-weight: bold;
    }

    .mode-toggle {
        child-space: 1s;
        col-between: 6px;
    }

    .mode-button {
        background-color: #0f172a;
        border: 1px solid #334155;
        border-radius: 6px;
        color: #94a3b8;
        font-size: 11px;
        padding-left: 10px;
        padding-right: 10px;
        padding-top: 6px;
        padding-bottom: 6px;
    }

    .mode-button-active {
        background-color: #1d4ed8;
        border: 1px solid #3b82f6;
        color: #ffffff;
        font-size: 11px;
        padding-left: 10px;
        padding-right: 10px;
        padding-top: 6px;
        padding-bottom: 6px;
    }

    .column-header {
        font-size: 11px;
        font-weight: bold;
        text-transform: uppercase;
        letter-spacing: 1px;
        margin-bottom: 15px;
    }

    .col-levels { color: #94a3b8; }
    .col-clean { color: #60a5fa; } /* Blue 400 */
    .col-polish { color: #60a5fa; }

    .slider-label {
        font-size: 11px;
        color: #94a3b8;
    }

    .slider-value {
        font-size: 11px;
        color: #f1f5f9;
        font-weight: bold;
        text-align: right;
    }

    .meter-label {
        font-size: 9px;
        color: #64748b;
        width: 100%;
        text-align: center;
        margin-bottom: 5px;
    }

    .delta-label {
        font-size: 9px;
        color: #64748b;
    }

    .footer {
        height: 24px;
        background-color: #0f172a;
        padding-left: 8px;
        padding-right: 8px;
        padding-bottom: 4px;
    }

    .log-button {
        background-color: transparent;
        border: none;
        color: #475569;
        font-size: 9px;
        padding-left: 4px;
        padding-right: 4px;
        padding-top: 2px;
        padding-bottom: 2px;
    }

    .log-button:hover {
        color: #94a3b8;
    }
"#;

#[derive(Lens, Clone)]
pub struct Data {
    pub params: Arc<VoiceParams>,
}

impl Model for Data {}

type VoiceParamsBoolLens = Map<Wrapper<data_derived_lenses::params>, bool>;

// --- COMPONENT: LEVEL METER ---

pub struct LevelMeter {
    meters: Arc<Meters>,
    meter_type: MeterType,
}

#[derive(Clone, Copy)]
pub enum MeterType {
    InputL,
    InputR,
    OutputL,
    OutputR,
    GainReduction, // Simplified for visual grouping
}

impl LevelMeter {
    pub fn new(cx: &mut Context, meters: Arc<Meters>, meter_type: MeterType) -> Handle<'_, Self> {
        Self { meters, meter_type }.build(cx, |_| {})
    }
}

impl View for LevelMeter {
    fn element(&self) -> Option<&'static str> {
        Some("level-meter")
    }

    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let bounds = cx.bounds();
        let is_gr = matches!(self.meter_type, MeterType::GainReduction);

        // 1. Get Value
        let level_db = match self.meter_type {
            MeterType::InputL => self.meters.get_input_peak_l(),
            MeterType::InputR => self.meters.get_input_peak_r(),
            MeterType::OutputL => self.meters.get_output_peak_l(),
            MeterType::OutputR => self.meters.get_output_peak_r(),
            MeterType::GainReduction => {
                let gr_l = self.meters.get_gain_reduction_l();
                let gr_r = self.meters.get_gain_reduction_r();
                0.5 * (gr_l + gr_r)
            }
        };

        // 2. Normalize
        let normalized = if is_gr {
            (level_db / 20.0).clamp(0.0, 1.0)
        } else {
            ((level_db + 60.0) / 60.0).clamp(0.0, 1.0)
        };

        // 3. Draw Background (Dark Track)
        let mut bg_path = vg::Path::new();
        bg_path.rect(bounds.x, bounds.y, bounds.w, bounds.h);
        canvas.fill_path(&bg_path, &vg::Paint::color(vg::Color::rgb(15, 23, 42))); // Very dark slate

        // Border
        canvas.stroke_path(
            &bg_path,
            &vg::Paint::color(vg::Color::rgb(51, 65, 85)).with_line_width(1.0),
        );

        // 4. Draw Active Meter Fill
        if normalized > 0.001 {
            let fill_h = bounds.h * normalized;
            let fill_y = bounds.y + (bounds.h - fill_h);

            let mut fill_path = vg::Path::new();
            fill_path.rect(bounds.x + 1.0, fill_y, bounds.w - 2.0, fill_h);

            // Gradient Paint
            let paint = if is_gr {
                // Orange/Red for reduction
                vg::Paint::linear_gradient(
                    bounds.x,
                    bounds.y,
                    bounds.x,
                    bounds.y + bounds.h,
                    vg::Color::rgb(239, 68, 68),  // Red
                    vg::Color::rgb(249, 115, 22), // Orange
                )
            } else {
                // Green -> Yellow -> Red
                vg::Paint::linear_gradient(
                    bounds.x,
                    bounds.y + bounds.h,
                    bounds.x,
                    bounds.y,
                    vg::Color::rgb(34, 197, 94), // Green (Bottom)
                    vg::Color::rgb(239, 68, 68), // Red (Top)
                )
            };

            canvas.fill_path(&fill_path, &paint);
        }

        // 5. Draw "LED" segments (Grid lines)
        let mut line_path = vg::Path::new();
        let step = bounds.h / 20.0; // 20 segments
        for i in 1..20 {
            let y_pos = bounds.y + (i as f32 * step);
            line_path.move_to(bounds.x, y_pos);
            line_path.line_to(cx.bounds().x + bounds.w, y_pos);
        }
        canvas.stroke_path(
            &line_path,
            &vg::Paint::color(vg::Color::rgba(0, 0, 0, 100)).with_line_width(1.0),
        );
    }
}

// --- COMPONENT: DELTA ACTIVITY ---

pub struct DeltaActivityLight {
    meters: Arc<Meters>,
    level: DeltaLevel,
}

#[derive(Clone, Copy)]
pub enum DeltaLevel {
    Idle,
    Light,
    Heavy,
}

impl DeltaActivityLight {
    pub fn new(cx: &mut Context, meters: Arc<Meters>, level: DeltaLevel) -> Handle<'_, Self> {
        Self { meters, level }.build(cx, |_| {})
    }
}

impl View for DeltaActivityLight {
    fn element(&self) -> Option<&'static str> {
        Some("delta-activity-light")
    }

    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let bounds = cx.bounds();
        let activity = self.meters.get_delta_activity();
        let active_level = if activity < 0.5 {
            DeltaLevel::Idle
        } else if activity < 1.5 {
            DeltaLevel::Light
        } else {
            DeltaLevel::Heavy
        };
        let is_active = matches!(
            (self.level, active_level),
            (DeltaLevel::Idle, DeltaLevel::Idle)
                | (DeltaLevel::Light, DeltaLevel::Light)
                | (DeltaLevel::Heavy, DeltaLevel::Heavy)
        );

        let mut path = vg::Path::new();
        path.rounded_rect(bounds.x, bounds.y, bounds.w, bounds.h, 2.0);

        let inactive = vg::Color::rgb(30, 41, 59);
        let active = match self.level {
            DeltaLevel::Idle => vg::Color::rgb(148, 163, 184),
            DeltaLevel::Light => vg::Color::rgb(250, 204, 21),
            DeltaLevel::Heavy => vg::Color::rgb(239, 68, 68),
        };

        canvas.fill_path(
            &path,
            &vg::Paint::color(if is_active { active } else { inactive }),
        );
        canvas.stroke_path(
            &path,
            &vg::Paint::color(vg::Color::rgb(51, 65, 85)).with_line_width(1.0),
        );
    }
}

// --- COMPONENT: MODERN SLIDER ---

/// Draws the visual representation (Fill bar)
pub struct SliderVisuals {
    params: Arc<VoiceParams>,
    param_id: ParamId,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ParamId {
    NoiseReduction,
    NoiseTone,
    ReverbReduction,
    Clarity,
    Proximity,
    DeEsser,
    Leveler,
    OutputGain,
    UseMl,
    MacroDistance,
    MacroClarity,
    MacroConsistency,
}

/// Preview parameters - only subtractive effects have preview
#[derive(Clone, Copy)]
pub enum PreviewParamId {
    Denoise,
    Deverb,
    DeEsser,
}

impl SliderVisuals {
    pub fn new(cx: &mut Context, params: Arc<VoiceParams>, param_id: ParamId) -> Handle<'_, Self> {
        Self { params, param_id }.build(cx, |_| {})
    }
}

impl View for SliderVisuals {
    fn element(&self) -> Option<&'static str> {
        Some("slider-visuals")
    }

    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let bounds = cx.bounds();

        // Get Normalized Value
        let normalized = match self.param_id {
            ParamId::NoiseReduction => self.params.noise_reduction.modulated_normalized_value(),
            ParamId::NoiseTone => self.params.noise_tone.modulated_normalized_value(),
            ParamId::ReverbReduction => self.params.reverb_reduction.modulated_normalized_value(),
            ParamId::Clarity => self.params.clarity.modulated_normalized_value(),
            ParamId::Proximity => self.params.proximity.modulated_normalized_value(),
            ParamId::DeEsser => self.params.de_esser.modulated_normalized_value(),
            ParamId::Leveler => self.params.leveler.modulated_normalized_value(),
            ParamId::OutputGain => self.params.output_gain.modulated_normalized_value(),
            ParamId::UseMl => self.params.use_ml.modulated_normalized_value(),
            ParamId::MacroDistance => self.params.macro_distance.modulated_normalized_value(),
            ParamId::MacroClarity => self.params.macro_clarity.modulated_normalized_value(),
            ParamId::MacroConsistency => self.params.macro_consistency.modulated_normalized_value(),
        };

        // 1. Draw Background Track
        let mut bg = vg::Path::new();
        bg.rounded_rect(bounds.x, bounds.y, bounds.w, bounds.h, 2.0);
        canvas.fill_path(&bg, &vg::Paint::color(vg::Color::rgb(30, 41, 59))); // Slate 800
        canvas.stroke_path(
            &bg,
            &vg::Paint::color(vg::Color::rgb(51, 65, 85)).with_line_width(1.0),
        ); // Border

        // 2. Draw Fill Bar
        if self.param_id == ParamId::NoiseTone {
            // Bipolar drawing for Tone
            let center_x = bounds.x + bounds.w / 2.0;
            let val_x = bounds.x + bounds.w * normalized;

            let (start_x, w) = if normalized >= 0.5 {
                (center_x, val_x - center_x)
            } else {
                (val_x, center_x - val_x)
            };

            if w > 0.5 {
                let mut fill = vg::Path::new();
                fill.rounded_rect(start_x, bounds.y, w, bounds.h, 2.0);

                // Use a slightly different color or same blue
                let fill_color = vg::Color::rgba(59, 130, 246, 180);
                canvas.fill_path(&fill, &vg::Paint::color(fill_color));

                // Cap line at value end
                let mut cap = vg::Path::new();
                cap.move_to(val_x, bounds.y);
                cap.line_to(val_x, bounds.y + bounds.h);
                canvas.stroke_path(
                    &cap,
                    &vg::Paint::color(vg::Color::rgb(96, 165, 250)).with_line_width(1.0),
                );
            }
        } else if normalized > 0.0 {
            // Standard Unipolar drawing
            let mut fill = vg::Path::new();
            fill.rounded_rect(bounds.x, bounds.y, bounds.w * normalized, bounds.h, 2.0);

            // Blue fill with slight transparency
            let fill_color = vg::Color::rgba(59, 130, 246, 180); // Blue 500
            canvas.fill_path(&fill, &vg::Paint::color(fill_color));

            // Bright end cap line
            let mut cap = vg::Path::new();
            cap.move_to(bounds.x + (bounds.w * normalized), bounds.y);
            cap.line_to(bounds.x + (bounds.w * normalized), bounds.y + bounds.h);
            canvas.stroke_path(
                &cap,
                &vg::Paint::color(vg::Color::rgb(96, 165, 250)).with_line_width(1.0),
            );
        }

        // 3. Center line for Tone (if needed, optional logic here)
        if self.param_id == ParamId::NoiseTone {
            let mut center = vg::Path::new();
            center.move_to(bounds.x + bounds.w / 2.0, bounds.y);
            center.line_to(bounds.x + bounds.w / 2.0, bounds.y + bounds.h);
            canvas.stroke_path(
                &center,
                &vg::Paint::color(vg::Color::rgba(255, 255, 255, 30)).with_line_width(1.0),
            );
        }
    }
}

/// Helper: Creates Label, Value, Visual Bar, and Invisible Interaction Slider
pub fn create_modern_slider<'a, P>(
    cx: &'a mut Context,
    label_text: &'static str,
    params_arc: Arc<VoiceParams>,
    gui_context: Arc<dyn GuiContext>,
    param_id: ParamId,
    params_to_param: impl Fn(&Arc<VoiceParams>) -> &P + Copy + 'static,
) -> Handle<'a, VStack>
where
    P: Param + 'static,
{
    // Setup for mouse down handler (macro write lock)
    let params_for_mouse = params_arc.clone();
    let gui_context_for_mouse = gui_context.clone();

    let should_disable_macro = match param_id {
        ParamId::MacroDistance | ParamId::MacroClarity | ParamId::MacroConsistency => false,
        _ => true,
    };

    VStack::new(cx, move |cx| {
        // 1. Text Row (Label ----- Value)
        HStack::new(cx, move |cx| {
            Label::new(cx, label_text)
                .class("slider-label")
                .text_wrap(false);
            Element::new(cx).width(Stretch(1.0));
            let value_lens =
                ParamWidgetBase::make_lens(Data::params, params_to_param, |param: &P| {
                    param.normalized_value_to_string(param.unmodulated_normalized_value(), true)
                });
            Label::new(cx, value_lens)
                .class("slider-value")
                .width(Pixels(60.0));
        })
        .height(Pixels(20.0))
        .width(Stretch(1.0))
        .col_between(Pixels(6.0));

        // 2. The Slider Stack
        ZStack::new(cx, move |cx| {
            // Layer 1: The Visuals (Custom Drawing)
            SliderVisuals::new(cx, params_arc.clone(), param_id)
                .width(Stretch(1.0))
                .height(Stretch(1.0));

            // Layer 2: The Logic (Invisible ParamSlider)
            ParamSlider::new(cx, Data::params, move |params| params_to_param(params))
                .width(Stretch(1.0))
                .height(Stretch(1.0))
                .opacity(0.0); // Invisible but captures mouse events
        })
        .height(Pixels(24.0))
        .on_mouse_down(move |_, _| {
            if should_disable_macro {
                set_macro_mode(&params_for_mouse, &gui_context_for_mouse, false);
            }
        });
    })
    .height(Auto)
    .bottom(Pixels(8.0)) // Reduced margin
}

fn set_macro_mode(params: &Arc<VoiceParams>, gui_context: &Arc<dyn GuiContext>, enabled: bool) {
    let setter = ParamSetter::new(gui_context.as_ref());

    // When switching FROM Simple mode TO Advanced mode, sync the advanced params
    // to reflect the current macro values. This ensures that the state is consistent.
    if !enabled && params.macro_mode.value() {
        let distance = params.macro_distance.value();
        let clarity = params.macro_clarity.value();
        let consistency = params.macro_consistency.value();

        let targets = compute_targets_from_macros(distance, clarity, consistency);

        // Set advanced params to match macro targets
        setter.begin_set_parameter(&params.noise_reduction);
        setter.set_parameter(&params.noise_reduction, targets.noise_reduction);
        setter.end_set_parameter(&params.noise_reduction);

        setter.begin_set_parameter(&params.noise_tone);
        setter.set_parameter(&params.noise_tone, targets.noise_tone);
        setter.end_set_parameter(&params.noise_tone);

        setter.begin_set_parameter(&params.reverb_reduction);
        setter.set_parameter(&params.reverb_reduction, targets.reverb_reduction);
        setter.end_set_parameter(&params.reverb_reduction);

        setter.begin_set_parameter(&params.proximity);
        setter.set_parameter(&params.proximity, targets.proximity);
        setter.end_set_parameter(&params.proximity);

        setter.begin_set_parameter(&params.de_esser);
        setter.set_parameter(&params.de_esser, targets.de_esser);
        setter.end_set_parameter(&params.de_esser);

        setter.begin_set_parameter(&params.leveler);
        setter.set_parameter(&params.leveler, targets.leveler);
        setter.end_set_parameter(&params.leveler);

        // ML Advisor state is preserved (it's a user preference)
        setter.begin_set_parameter(&params.use_ml);
        setter.set_parameter(&params.use_ml, params.use_ml.value());
        setter.end_set_parameter(&params.use_ml);
    }

    setter.begin_set_parameter(&params.macro_mode);
    setter.set_parameter(&params.macro_mode, enabled);
    setter.end_set_parameter(&params.macro_mode);

    if enabled {
        set_preview_mode(params, gui_context, None);
    }
}

fn set_preview_mode(
    params: &Arc<VoiceParams>,
    gui_context: &Arc<dyn GuiContext>,
    active: Option<PreviewParamId>,
) {
    let setter = ParamSetter::new(gui_context.as_ref());
    let (denoise, deverb, deesser) = match active {
        Some(PreviewParamId::Denoise) => (true, false, false),
        Some(PreviewParamId::Deverb) => (false, true, false),
        Some(PreviewParamId::DeEsser) => (false, false, true),
        None => (false, false, false),
    };

    setter.begin_set_parameter(&params.preview_denoise);
    setter.set_parameter(&params.preview_denoise, denoise);
    setter.end_set_parameter(&params.preview_denoise);

    setter.begin_set_parameter(&params.preview_deverb);
    setter.set_parameter(&params.preview_deverb, deverb);
    setter.end_set_parameter(&params.preview_deverb);

    setter.begin_set_parameter(&params.preview_deesser);
    setter.set_parameter(&params.preview_deesser, deesser);
    setter.end_set_parameter(&params.preview_deesser);
}

fn build_header_previews(
    cx: &mut Context,
    params: Arc<VoiceParams>,
    gui_context: Arc<dyn GuiContext>,
) {
    HStack::new(cx, move |cx| {
        Label::new(cx, "PREVIEW CUTS:")
            .class("header-sub")
            .top(Pixels(2.0));

        create_header_preview_button(
            cx,
            "Noise",
            params.clone(),
            gui_context.clone(),
            PreviewParamId::Denoise,
        );
        create_header_preview_button(
            cx,
            "Reverb",
            params.clone(),
            gui_context.clone(),
            PreviewParamId::Deverb,
        );
        create_header_preview_button(
            cx,
            "Sibilance",
            params.clone(),
            gui_context.clone(),
            PreviewParamId::DeEsser,
        );
    })
    .col_between(Pixels(8.0))
    .child_top(Stretch(1.0))
    .child_bottom(Stretch(1.0));
}

fn create_header_preview_button(
    cx: &mut Context,
    text: &'static str,
    params: Arc<VoiceParams>,
    gui_context: Arc<dyn GuiContext>,
    id: PreviewParamId,
) {
    let params_for_binding = params.clone();
    let gui_context_for_binding = gui_context.clone();

    Binding::new(
        cx,
        Data::params.map(move |p| match id {
            PreviewParamId::Denoise => p.preview_denoise.value(),
            PreviewParamId::Deverb => p.preview_deverb.value(),
            PreviewParamId::DeEsser => p.preview_deesser.value(),
        }),
        move |cx, is_active_lens: VoiceParamsBoolLens| {
            let is_active = is_active_lens.get(cx);
            let params_btn = params_for_binding.clone();
            let gui_ctx = gui_context_for_binding.clone();

            Button::new(
                cx,
                move |_| {
                    let next = if is_active { None } else { Some(id) };
                    set_preview_mode(&params_btn, &gui_ctx, next);
                },
                |cx| Label::new(cx, text),
            )
            .class(if is_active {
                "preview-button-active"
            } else {
                "preview-button"
            });
        },
    );
}

fn build_levels_column(cx: &mut Context, meters: Arc<Meters>) {
    let meters_in = meters.clone();
    let meters_gr = meters.clone();
    let meters_out = meters.clone();

    VStack::new(cx, move |cx| {
        Label::new(cx, "LEVELS")
            .class("column-header")
            .class("col-levels");

        HStack::new(cx, move |cx| {
            // IN
            VStack::new(cx, move |cx| {
                Label::new(cx, "IN").class("meter-label");
                HStack::new(cx, move |cx| {
                    LevelMeter::new(cx, meters_in.clone(), MeterType::InputL).width(Pixels(10.0));
                    LevelMeter::new(cx, meters_in.clone(), MeterType::InputR).width(Pixels(10.0));
                })
                .col_between(Pixels(2.0))
                .height(Stretch(1.0));
            })
            .width(Pixels(30.0))
            .height(Stretch(1.0));

            // GR
            VStack::new(cx, move |cx| {
                Label::new(cx, "GR").class("meter-label");
                LevelMeter::new(cx, meters_gr.clone(), MeterType::GainReduction)
                    .width(Pixels(14.0))
                    .height(Stretch(1.0));
            })
            .width(Pixels(30.0))
            .height(Stretch(1.0));

            // OUT
            VStack::new(cx, move |cx| {
                Label::new(cx, "OUT").class("meter-label");
                HStack::new(cx, move |cx| {
                    LevelMeter::new(cx, meters_out.clone(), MeterType::OutputL).width(Pixels(10.0));
                    LevelMeter::new(cx, meters_out.clone(), MeterType::OutputR).width(Pixels(10.0));
                })
                .col_between(Pixels(2.0))
                .height(Stretch(1.0));
            })
            .width(Pixels(30.0))
            .height(Stretch(1.0));
        })
        .col_between(Pixels(10.0))
        .height(Pixels(180.0)); // Fixed height for meters

        // Spacing between meters and delta activity
        Element::new(cx).height(Pixels(10.0));

        Label::new(cx, "DELTA ACTIVITY").class("meter-label");
        HStack::new(cx, move |cx| {
            let meters_delta = meters.clone();
            VStack::new(cx, move |cx| {
                DeltaActivityLight::new(cx, meters_delta.clone(), DeltaLevel::Idle)
                    .width(Pixels(12.0))
                    .height(Pixels(8.0));
                Label::new(cx, "Idle").class("delta-label");
            });
            let meters_delta = meters.clone();
            VStack::new(cx, move |cx| {
                DeltaActivityLight::new(cx, meters_delta.clone(), DeltaLevel::Light)
                    .width(Pixels(12.0))
                    .height(Pixels(8.0));
                Label::new(cx, "Light").class("delta-label");
            });
            let meters_delta = meters.clone();
            VStack::new(cx, move |cx| {
                DeltaActivityLight::new(cx, meters_delta.clone(), DeltaLevel::Heavy)
                    .width(Pixels(12.0))
                    .height(Pixels(8.0));
                Label::new(cx, "Heavy").class("delta-label");
            });
        })
        .col_between(Pixels(10.0));
    })
    .width(Pixels(130.0)); // Fixed width for levels column
}

fn build_clean_column(
    cx: &mut Context,
    params: Arc<VoiceParams>,
    gui_context: Arc<dyn GuiContext>,
) {
    VStack::new(cx, move |cx| {
        Label::new(cx, "CLEAN & REPAIR")
            .class("column-header")
            .class("col-clean");

        create_modern_slider(
            cx,
            "Noise Reduction",
            params.clone(),
            gui_context.clone(),
            ParamId::NoiseReduction,
            |p| &p.noise_reduction,
        );
        create_modern_slider(
            cx,
            "Tone",
            params.clone(),
            gui_context.clone(),
            ParamId::NoiseTone,
            |p| &p.noise_tone,
        );
        create_modern_slider(
            cx,
            "De-Verb",
            params.clone(),
            gui_context.clone(),
            ParamId::ReverbReduction,
            |p| &p.reverb_reduction,
        );
        create_modern_slider(
            cx,
            "Clarity",
            params.clone(),
            gui_context.clone(),
            ParamId::Clarity,
            |p| &p.clarity,
        );

        // ML Toggle
        let params_for_ml = params.clone();
        let gui_ctx_for_ml = gui_context.clone();
        Binding::new(
            cx,
            Data::params.map(|p| p.use_ml.value()),
            move |cx, use_ml_lens: VoiceParamsBoolLens| {
                let use_ml = use_ml_lens.get(cx);
                let params_btn = params_for_ml.clone();
                let gui_ctx_btn = gui_ctx_for_ml.clone();
                HStack::new(cx, move |cx| {
                    Label::new(cx, "ML Advisor")
                        .class("slider-label")
                        .text_wrap(false);
                    Element::new(cx).width(Stretch(1.0));
                    Button::new(
                        cx,
                        move |_| {
                            let setter = ParamSetter::new(gui_ctx_btn.as_ref());
                            setter.begin_set_parameter(&params_btn.use_ml);
                            setter.set_parameter(&params_btn.use_ml, !use_ml);
                            setter.end_set_parameter(&params_btn.use_ml);
                        },
                        |cx| Label::new(cx, if use_ml { "ON" } else { "OFF" }),
                    )
                    .class(if use_ml {
                        "preview-toggle-active"
                    } else {
                        "preview-toggle"
                    })
                    .width(Pixels(40.0));
                })
                .height(Pixels(20.0))
                .width(Stretch(1.0))
                .top(Pixels(4.0));
            },
        );
    })
    .width(Stretch(1.0));
}

fn build_polish_column(
    cx: &mut Context,
    params: Arc<VoiceParams>,
    gui_context: Arc<dyn GuiContext>,
) {
    VStack::new(cx, move |cx| {
        Label::new(cx, "POLISH & ENHANCE")
            .class("column-header")
            .class("col-polish");

        create_modern_slider(
            cx,
            "Proximity",
            params.clone(),
            gui_context.clone(),
            ParamId::Proximity,
            |p| &p.proximity,
        );
        create_modern_slider(
            cx,
            "De-Ess",
            params.clone(),
            gui_context.clone(),
            ParamId::DeEsser,
            |p| &p.de_esser,
        );
        create_modern_slider(
            cx,
            "Leveler",
            params.clone(),
            gui_context.clone(),
            ParamId::Leveler,
            |p| &p.leveler,
        );

        // Spacing
        Element::new(cx).height(Pixels(20.0));

        // Output Gain (Highlighted)
        Label::new(cx, "OUTPUT")
            .class("column-header")
            .color(Color::rgb(249, 115, 22));
        create_modern_slider(cx, "Gain", params, gui_context, ParamId::OutputGain, |p| {
            &p.output_gain
        });
    })
    .width(Stretch(1.0));
}

fn build_macro_column(
    cx: &mut Context,
    params: Arc<VoiceParams>,
    gui_context: Arc<dyn GuiContext>,
) {
    VStack::new(cx, move |cx| {
        Label::new(cx, "EASY CONTROLS")
            .class("column-header")
            .class("col-clean");

        create_modern_slider(
            cx,
            "Distance",
            params.clone(),
            gui_context.clone(),
            ParamId::MacroDistance,
            |p| &p.macro_distance,
        );
        create_modern_slider(
            cx,
            "Clarity",
            params.clone(),
            gui_context.clone(),
            ParamId::MacroClarity,
            |p| &p.macro_clarity,
        );
        create_modern_slider(
            cx,
            "Consistency",
            params.clone(),
            gui_context.clone(),
            ParamId::MacroConsistency,
            |p| &p.macro_consistency,
        );
    })
    .width(Stretch(1.0));
}

// --- MAIN VIEW BUILDER ---

pub fn build_ui(
    cx: &mut Context,
    params: Arc<VoiceParams>,
    meters: Arc<Meters>,
    _ui_proxy: Arc<Mutex<Option<ContextProxy>>>,
    gui_context: Arc<dyn GuiContext>,
) {
    // Inject CSS
    let _ = cx.add_stylesheet(STYLE);

    // Bind Data
    Data {
        params: params.clone(),
    }
    .build(cx);

    // Main Container
    VStack::new(cx, move |cx| {
        // --- HEADER ---
        HStack::new(cx, |cx| {
            // Title on left
            HStack::new(cx, |cx| {
                Label::new(cx, "VOICE STUDIO").class("header-title");
                Label::new(cx, "Vocal Restoration & Enhancement")
                    .class("header-sub")
                    .left(Pixels(12.0));
            })
            .col_between(Pixels(0.0))
            .child_top(Stretch(1.0))
            .child_bottom(Stretch(1.0));

            // Spacer
            Element::new(cx).width(Stretch(1.0));

            // Header Preview Buttons
            build_header_previews(cx, params.clone(), gui_context.clone());

            // Mode toggle on right
            let params_for_mode = params.clone();
            let gui_context_for_mode = gui_context.clone();
            Binding::new(
                cx,
                Data::params.map(|p| p.macro_mode.value()),
                move |cx, macro_mode_lens: VoiceParamsBoolLens| {
                    let params_btn = params_for_mode.clone();
                    let gui_ctx = gui_context_for_mode.clone();
                    let macro_mode = macro_mode_lens.get(cx);
                    HStack::new(cx, move |cx| {
                        Button::new(
                            cx,
                            {
                                let params_btn = params_btn.clone();
                                let gui_ctx = gui_ctx.clone();
                                move |_| set_macro_mode(&params_btn, &gui_ctx, true)
                            },
                            |cx| Label::new(cx, "Simple"),
                        )
                        .class(if macro_mode {
                            "mode-button-active"
                        } else {
                            "mode-button"
                        });
                        Button::new(
                            cx,
                            {
                                let params_btn = params_btn.clone();
                                let gui_ctx = gui_ctx.clone();
                                move |_| set_macro_mode(&params_btn, &gui_ctx, false)
                            },
                            |cx| Label::new(cx, "Advanced"),
                        )
                        .class(if macro_mode {
                            "mode-button"
                        } else {
                            "mode-button-active"
                        });
                    })
                    .col_between(Pixels(4.0));
                },
            );
        })
        .class("header");

        // --- MAIN CONTENT GRID ---
        let meters_for_main = meters.clone();
        let params_for_main = params.clone();
        let gui_context_for_main = gui_context.clone();
        Binding::new(
            cx,
            Data::params.map(|p| p.macro_mode.value()),
            move |cx, macro_mode_lens: VoiceParamsBoolLens| {
                // Clone inside Binding closure (Fn requires this)
                let meters_inner = meters_for_main.clone();
                let params_inner = params_for_main.clone();
                let gui_ctx_inner = gui_context_for_main.clone();
                let macro_mode = macro_mode_lens.get(cx);
                HStack::new(cx, move |cx| {
                    build_levels_column(cx, meters_inner.clone());
                    if macro_mode {
                        build_macro_column(cx, params_inner.clone(), gui_ctx_inner.clone());
                        // Spacer to fill remaining space in Simple mode
                        Element::new(cx).width(Stretch(1.0));
                    } else {
                        build_clean_column(cx, params_inner.clone(), gui_ctx_inner.clone());
                        build_polish_column(cx, params_inner.clone(), gui_ctx_inner.clone());
                    }
                })
                .col_between(Pixels(24.0))
                .class("main-view")
                .child_left(Pixels(20.0))
                .child_right(Pixels(20.0))
                .child_top(Pixels(20.0))
                .child_bottom(Pixels(12.0))
                .width(Stretch(1.0))
                .height(Stretch(1.0));
            },
        );

        // --- FOOTER ---
        HStack::new(cx, |cx| {
            Button::new(
                cx,
                |_| {
                    // Open the log file
                    #[cfg(target_os = "macos")]
                    {
                        // Touch the file first to create it if it doesn't exist
                        let _ = std::process::Command::new("touch")
                            .arg("/tmp/voice_studio.log")
                            .status();
                        let _ = std::process::Command::new("open")
                            .arg("-a")
                            .arg("Console")
                            .arg("/tmp/voice_studio.log")
                            .spawn();
                    }
                    #[cfg(target_os = "linux")]
                    {
                        let _ = std::process::Command::new("xdg-open")
                            .arg("/tmp/voice_studio.log")
                            .spawn();
                    }
                    #[cfg(target_os = "windows")]
                    {
                        let _ = std::process::Command::new("notepad")
                            .arg("C:\\temp\\voice_studio.log")
                            .spawn();
                    }
                },
                |cx| Label::new(cx, "Log"),
            )
            .class("log-button");
        })
        .class("footer");
    })
    .class("app-root")
    .width(Stretch(1.0))
    .height(Stretch(1.0));
}
