use crate::meters::Meters;
use crate::{AutoSuggestTargets, VoiceParams};
use nih_plug::params::Param;
use nih_plug::prelude::BoolParam;
use nih_plug::prelude::{GuiContext, ParamSetter};
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
        height: 72px;
        background-color: #1e293b;
        border-bottom: 1px solid #334155;
        child-space: 1s;
        col-between: 12px;
        padding-left: 20px;
        padding-right: 20px;
        padding-top: 12px;
    }

    .header-title {
        font-size: 20px;
        font-weight: bold;
        color: #ffffff;
    }

    .header-sub {
        font-size: 12px;
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

    .preview-toggle:checked {
        background-color: #334155;
        color: #e2e8f0;
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
"#;

#[derive(Lens, Clone)]
pub struct Data {
    pub params: Arc<VoiceParams>,
}

impl Model for Data {}

#[allow(dead_code)]
// Single write path for Auto-Suggest parameter updates.
fn apply_auto_suggest_targets(
    params: &Arc<VoiceParams>,
    gui_context: &Arc<dyn GuiContext>,
    targets: AutoSuggestTargets,
) {
    let setter = ParamSetter::new(gui_context.as_ref());

    setter.begin_set_parameter(&params.noise_reduction);
    setter.set_parameter(&params.noise_reduction, targets.noise_reduction.value);
    setter.end_set_parameter(&params.noise_reduction);

    setter.begin_set_parameter(&params.reverb_reduction);
    setter.set_parameter(&params.reverb_reduction, targets.reverb_reduction.value);
    setter.end_set_parameter(&params.reverb_reduction);

    setter.begin_set_parameter(&params.leveler);
    setter.set_parameter(&params.leveler, targets.leveler.value);
    setter.end_set_parameter(&params.leveler);

    setter.begin_set_parameter(&params.de_esser);
    setter.set_parameter(&params.de_esser, targets.de_esser.value);
    setter.end_set_parameter(&params.de_esser);
}

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
    pub fn new(
        cx: &mut Context,
        meters: Arc<Meters>,
        level: DeltaLevel,
    ) -> Handle<'_, Self> {
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
}

#[derive(Clone, Copy)]
pub enum PreviewParamId {
    NoiseReduction,
    ReverbReduction,
    Clarity,
    Proximity,
    DeEsser,
    Leveler,
    OutputGain,
}

fn preview_param_for<'a>(params: &'a Arc<VoiceParams>, id: PreviewParamId) -> &'a BoolParam {
    match id {
        PreviewParamId::NoiseReduction => &params.preview_noise_reduction,
        PreviewParamId::ReverbReduction => &params.preview_reverb_reduction,
        PreviewParamId::Clarity => &params.preview_clarity,
        PreviewParamId::Proximity => &params.preview_proximity,
        PreviewParamId::DeEsser => &params.preview_de_esser,
        PreviewParamId::Leveler => &params.preview_leveler,
        PreviewParamId::OutputGain => &params.preview_output_gain,
    }
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
    param_id: ParamId,
    preview_param_id: Option<PreviewParamId>,
    params_to_param: impl Fn(&Arc<VoiceParams>) -> &P + Copy + 'static,
) -> Handle<'a, VStack>
where
    P: Param + 'static,
{
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
        .height(Pixels(24.0));

        if let Some(preview_id) = preview_param_id {
            Binding::new(
                cx,
                Data::params.map(|params| params.preview_cuts.value()),
                move |cx, show_preview| {
                    if show_preview.get(cx) {
                        HStack::new(cx, move |cx| {
                            Element::new(cx).width(Stretch(1.0));
                            ParamButton::new(cx, Data::params, move |params| {
                                preview_param_for(params, preview_id)
                            })
                            .with_label("Preview")
                            .class("preview-toggle");
                        })
                        .width(Stretch(1.0))
                        .height(Pixels(18.0));
                    }
                },
            );
        }
    })
    .height(Auto)
    .bottom(Pixels(16.0)) // Margin bottom
}

// --- MAIN VIEW BUILDER ---

pub fn build_ui(
    cx: &mut Context,
    params: Arc<VoiceParams>,
    meters: Arc<Meters>,
    _suggested_settings: Arc<Mutex<Option<AutoSuggestTargets>>>,
    _ui_proxy: Arc<Mutex<Option<ContextProxy>>>,
    _gui_context: Arc<dyn GuiContext>,
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
            VStack::new(cx, |cx| {
                Label::new(cx, "VOICE STUDIO").class("header-title");
                Label::new(cx, "Vocal Restoration & Enhancement").class("header-sub");
            })
            .width(Stretch(1.0));

            HStack::new(cx, |cx| {
                ParamButton::new(cx, Data::params, |params| &params.preview_cuts)
                    .with_label("Preview Cuts")
                    .class("preview-button");
            })
            .class("header-controls");
        })
        .class("header");

        // --- MAIN CONTENT GRID ---
        let meters_for_main = meters.clone();

        HStack::new(cx, move |cx| {
            let meters_in = meters_for_main.clone();
            let meters_gr = meters_for_main.clone();
            let meters_out = meters_for_main.clone();
            let params_clean = params.clone();
            let params_polish = params.clone();
            // COLUMN 1: LEVELS
            VStack::new(cx, move |cx| {
                Label::new(cx, "LEVELS")
                    .class("column-header")
                    .class("col-levels");

                HStack::new(cx, move |cx| {
                    // IN
                    VStack::new(cx, move |cx| {
                        Label::new(cx, "IN").class("meter-label");
                        HStack::new(cx, move |cx| {
                            LevelMeter::new(cx, meters_in.clone(), MeterType::InputL)
                                .width(Pixels(10.0));
                            LevelMeter::new(cx, meters_in.clone(), MeterType::InputR)
                                .width(Pixels(10.0));
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
                            LevelMeter::new(cx, meters_out.clone(), MeterType::OutputL)
                                .width(Pixels(10.0));
                            LevelMeter::new(cx, meters_out.clone(), MeterType::OutputR)
                                .width(Pixels(10.0));
                        })
                        .col_between(Pixels(2.0))
                        .height(Stretch(1.0));
                    })
                    .width(Pixels(30.0))
                    .height(Stretch(1.0));
                })
                .col_between(Pixels(10.0))
                .height(Pixels(200.0)); // Fixed height for meters

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
            .width(Stretch(1.0));

            // COLUMN 2: CLEAN & REPAIR
            VStack::new(cx, move |cx| {
                Label::new(cx, "CLEAN & REPAIR")
                    .class("column-header")
                    .class("col-clean");

                create_modern_slider(
                    cx,
                    "Noise Reduction",
                    params_clean.clone(),
                    ParamId::NoiseReduction,
                    Some(PreviewParamId::NoiseReduction),
                    |p| &p.noise_reduction,
                );
                create_modern_slider(
                    cx,
                    "Tone",
                    params_clean.clone(),
                    ParamId::NoiseTone,
                    None,
                    |p| &p.noise_tone,
                );
                create_modern_slider(
                    cx,
                    "De-Verb",
                    params_clean.clone(),
                    ParamId::ReverbReduction,
                    Some(PreviewParamId::ReverbReduction),
                    |p| &p.reverb_reduction,
                );
                create_modern_slider(
                    cx,
                    "Clarity",
                    params_clean,
                    ParamId::Clarity,
                    Some(PreviewParamId::Clarity),
                    |p| &p.clarity,
                );
            })
            .width(Stretch(1.0));

            // COLUMN 3: POLISH & ENHANCE
            VStack::new(cx, move |cx| {
                Label::new(cx, "POLISH & ENHANCE")
                    .class("column-header")
                    .class("col-polish");

                create_modern_slider(
                    cx,
                    "Proximity",
                    params_polish.clone(),
                    ParamId::Proximity,
                    Some(PreviewParamId::Proximity),
                    |p| &p.proximity,
                );
                create_modern_slider(
                    cx,
                    "De-Ess",
                    params_polish.clone(),
                    ParamId::DeEsser,
                    Some(PreviewParamId::DeEsser),
                    |p| &p.de_esser,
                );
                create_modern_slider(
                    cx,
                    "Leveler",
                    params_polish.clone(),
                    ParamId::Leveler,
                    Some(PreviewParamId::Leveler),
                    |p| &p.leveler,
                );

                // Spacing
                Element::new(cx).height(Pixels(20.0));

                // Output Gain (Highlighted)
                Label::new(cx, "OUTPUT")
                    .class("column-header")
                    .color(Color::rgb(249, 115, 22));
                create_modern_slider(
                    cx,
                    "Gain",
                    params_polish,
                    ParamId::OutputGain,
                    Some(PreviewParamId::OutputGain),
                    |p| &p.output_gain,
                );
            })
            .width(Stretch(1.0));
        })
        .col_between(Pixels(20.0))
        .class("main-view")
        .child_left(Pixels(30.0))
        .child_right(Pixels(30.0))
        .child_top(Pixels(30.0))
        .child_bottom(Pixels(30.0))
        .width(Stretch(1.0))
        .height(Stretch(1.0));
    })
    .class("app-root")
    .width(Stretch(1.0))
    .height(Stretch(1.0));
}
