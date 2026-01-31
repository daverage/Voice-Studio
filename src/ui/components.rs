//! Reusable UI component builders
//!
//! This module provides helper functions for creating consistent UI elements:
//! - Buttons: standard, toggle, momentary
//! - Sliders: horizontal parameter controls
//! - Knobs: macro dials
//! - Dropdowns: preset selection
//!
//! All builders use consistent patterns with nih_plug's ParamSlider for binding
//! to plugin parameters. Styling is handled via CSS classes defined in ui.css.

use nih_plug::params::Param;
use nih_plug::prelude::{GuiContext, ParamSetter};
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::widgets::*;
use nih_plug_vizia::widgets::param_base::ParamWidgetBase;
use std::sync::Arc;
use crate::VoiceParams;
use crate::ui::state::set_macro_mode;

#[derive(Clone, Copy, PartialEq, Data)]
pub enum ParamId {
    NoiseReduction,
    RumbleAmount,
    HissAmount,
    NoiseLearnAmount,
    ReverbReduction,
    Clarity,
    Proximity,
    DeEsser,
    Leveler,
    OutputGain,
    BreathControl,
    MacroDistance,
    MacroClarity,
    MacroConsistency,
}

// BUTTON HELPERS
pub fn create_button<'a>(
    cx: &'a mut Context,
    label: &'static str,
    class: &'static str,
    callback: impl Fn(&mut EventContext) + 'static,
) -> Handle<'a, Button> {
    Button::new(cx, callback, |cx| Label::new(cx, label)).class(class)
}

pub fn create_toggle_button<'a>(
    cx: &'a mut Context,
    label: &'static str,
    is_active: bool,
    active_class: &'static str,
    inactive_class: &'static str,
    callback: impl Fn(&mut EventContext) + 'static,
) -> Handle<'a, Button> {
    Button::new(cx, callback, |cx| Label::new(cx, label)).class(if is_active {
        active_class
    } else {
        inactive_class
    })
}

pub fn create_momentary_button<'a, P>(
    cx: &'a mut Context,
    label: &'static str,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
    param_getter: impl Fn(&VoiceParams) -> &P + Copy + Send + Sync + 'static,
) -> Handle<'a, HStack>
where
    P: Param<Plain = bool> + 'static,
{
    let params_down = params.clone();
    let gui_down = gui.clone();
    let params_up = params;
    let gui_up = gui;

    HStack::new(cx, move |cx| {
        Label::new(cx, label).hoverable(false);
    })
    .class("small-button")
    .on_mouse_down(move |cx, btn| {
        if btn == MouseButton::Left {
            let s = ParamSetter::new(gui_down.as_ref());
            let param = param_getter(params_down.as_ref());
            s.begin_set_parameter(param);
            s.set_parameter(param, true);
            s.end_set_parameter(param);
            cx.capture();
        }
    })
    .on_mouse_up(move |cx, btn| {
        if btn == MouseButton::Left {
            let s = ParamSetter::new(gui_up.as_ref());
            let param = param_getter(params_up.as_ref());
            s.begin_set_parameter(param);
            s.set_parameter(param, false);
            s.end_set_parameter(param);
            cx.release();
        }
    })
}

// SLIDER HELPERS
pub fn create_slider<'a, P>(
    cx: &'a mut Context,
    label: &'static str,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
    id: ParamId,
    map: impl Fn(&Arc<VoiceParams>) -> &P + Copy + 'static,
) -> Handle<'a, HStack>
where
    P: Param + 'static,
{
    let p_m = params.clone();
    let g_m = gui.clone();
    let disable_macros = !matches!(
        id,
        ParamId::MacroDistance | ParamId::MacroClarity | ParamId::MacroConsistency
    );

    HStack::new(cx, move |cx| {
        Label::new(cx, label)
            .class("slider-label")
            .class("adv-label")
            .text_wrap(false);

        ZStack::new(cx, move |cx| {
            SliderVisuals::new(cx, params.clone(), id).class("fill-both");

            // Value display (centered in slider)
            let lens = ParamWidgetBase::make_lens(crate::ui::state::VoiceStudioData::params, map, |p: &P| {
                p.normalized_value_to_string(p.unmodulated_normalized_value(), true)
            });
            Label::new(cx, lens)
                .class("slider-value")
                .class("adv-value")
                .hoverable(false);

            ParamSlider::new(cx, crate::ui::state::VoiceStudioData::params, move |p| map(p))
                .class("fill-both")
                .class("input-hidden");
        })
        .class("slider-visual")
        .class("adv-slider")
        .class("fill-width")
        .on_mouse_down(move |_, _| {
            if disable_macros {
                set_macro_mode(&p_m, &g_m, false);
            }
        });
    })
    .class("slider-container")
    .class("adv-row")
}

pub fn create_macro_dial<'a, P>(
    cx: &'a mut Context,
    label: &'static str,
    params: Arc<VoiceParams>,
    id: ParamId,
    map: impl Fn(&Arc<VoiceParams>) -> &P + Copy + 'static,
) -> Handle<'a, VStack>
where
    P: Param + 'static,
{
    VStack::new(cx, move |cx| {
        Label::new(cx, label).class("dial-label");

        // Use ZStack to layer visuals behind the interactive slider
        ZStack::new(cx, move |cx| {
            // Visual representation (behind)
            DialVisuals::new(cx, params.clone(), id).class("fill-both");

            // Value display (centered in dial)
            let lens = ParamWidgetBase::make_lens(crate::ui::state::VoiceStudioData::params, map, |p: &P| {
                p.normalized_value_to_string(p.unmodulated_normalized_value(), true)
            });
            Label::new(cx, lens).class("dial-value").hoverable(false);

            // Interactive slider (in front, invisible)
            ParamSlider::new(cx, crate::ui::state::VoiceStudioData::params, move |p| map(p))
                .class("fill-both")
                .class("input-hidden")
                .z_index(1);
        })
        .class("dial-visual");
    })
    .class("dial-container")
}

// DROPDOWN HELPERS
pub fn create_dropdown<'a>(
    cx: &'a mut Context,
    label: &'static str,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, HStack> {
    HStack::new(cx, move |cx| {
        Label::new(cx, label).class("dropdown-label");

        let lens = ParamWidgetBase::make_lens(
            crate::ui::state::VoiceStudioData::params,
            |p| &p.final_output_preset,
            |p| p.normalized_value_to_string(p.unmodulated_normalized_value(), true),
        );

        Dropdown::new(
            cx,
            move |cx| Label::new(cx, lens).class("dropdown-selected"),
            move |cx| {
                let params_list = params.clone();
                let gui_list = gui.clone();

                VStack::new(cx, move |cx| {
                    for preset in crate::presets::OutputPreset::all_presets().iter() {
                        let preset_value = *preset;
                        let params_item = params_list.clone();
                        let gui_item = gui_list.clone();

                        Label::new(cx, preset_value.name())
                            .class("dropdown-option")
                            .on_press(move |cx| {
                                let setter = ParamSetter::new(gui_item.as_ref());
                                setter.begin_set_parameter(&params_item.final_output_preset);
                                setter
                                    .set_parameter(&params_item.final_output_preset, preset_value);
                                setter.end_set_parameter(&params_item.final_output_preset);
                                cx.emit(PopupEvent::Close);
                            });
                    }
                })
                .class("dropdown-options");
            },
        )
        .class("dropdown-box");
    })
    .class("dropdown-row")
    .class("output-preset-dropdown")
}

pub fn create_dsp_preset_dropdown<'a>(
    cx: &'a mut Context,
    label: &'static str,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, HStack> {
    HStack::new(cx, move |cx| {
        Label::new(cx, label).class("dropdown-label");

        let lens = ParamWidgetBase::make_lens(
            crate::ui::state::VoiceStudioData::params,
            |p| &p.dsp_preset,
            |p| p.normalized_value_to_string(p.unmodulated_normalized_value(), true),
        );

        Dropdown::new(
            cx,
            move |cx| Label::new(cx, lens).class("dropdown-selected"),
            move |cx| {
                let params_list = params.clone();
                let gui_list = gui.clone();

                VStack::new(cx, move |cx| {
                    for preset in [
                        crate::presets::DspPreset::Manual,
                        crate::presets::DspPreset::PodcastNoisy,
                        crate::presets::DspPreset::VoiceoverStudio,
                        crate::presets::DspPreset::MudFree,
                        crate::presets::DspPreset::InterviewOutdoor,
                        crate::presets::DspPreset::BroadcastClean,
                    ]
                    .iter()
                    {
                        let preset_value = *preset;
                        let params_item = params_list.clone();
                        let gui_item = gui_list.clone();

                        Label::new(cx, preset_value.name())
                            .class("dropdown-option")
                            .on_press(move |cx| {
                                let setter = ParamSetter::new(gui_item.as_ref());

                                // Set the preset parameter itself
                                setter.begin_set_parameter(&params_item.dsp_preset);
                                setter.set_parameter(&params_item.dsp_preset, preset_value);
                                setter.end_set_parameter(&params_item.dsp_preset);

                                // Apply preset values to DSP parameters
                                if let Some(values) = preset_value.get_values() {
                                    // Set advanced parameters
                                    setter.begin_set_parameter(&params_item.noise_reduction);
                                    setter.set_parameter(
                                        &params_item.noise_reduction,
                                        values.noise_reduction,
                                    );
                                    setter.end_set_parameter(&params_item.noise_reduction);

                                    setter.begin_set_parameter(&params_item.reverb_reduction);
                                    setter.set_parameter(
                                        &params_item.reverb_reduction,
                                        values.reverb_reduction,
                                    );
                                    setter.end_set_parameter(&params_item.reverb_reduction);

                                    setter.begin_set_parameter(&params_item.proximity);
                                    setter.set_parameter(&params_item.proximity, values.proximity);
                                    setter.end_set_parameter(&params_item.proximity);

                                    setter.begin_set_parameter(&params_item.clarity);
                                    setter.set_parameter(&params_item.clarity, values.clarity);
                                    setter.end_set_parameter(&params_item.clarity);

                                    setter.begin_set_parameter(&params_item.de_esser);
                                    setter.set_parameter(&params_item.de_esser, values.de_esser);
                                    setter.end_set_parameter(&params_item.de_esser);

                                    setter.begin_set_parameter(&params_item.leveler);
                                    setter.set_parameter(&params_item.leveler, values.leveler);
                                    setter.end_set_parameter(&params_item.leveler);

                                    setter.begin_set_parameter(&params_item.breath_control);
                                    setter.set_parameter(
                                        &params_item.breath_control,
                                        values.breath_control,
                                    );
                                    setter.end_set_parameter(&params_item.breath_control);

                                    setter.begin_set_parameter(&params_item.macro_clean);
                                    setter.set_parameter(
                                        &params_item.macro_clean,
                                        values.macro_clean,
                                    );
                                    setter.end_set_parameter(&params_item.macro_clean);

                                    setter.begin_set_parameter(&params_item.macro_enhance);
                                    setter.set_parameter(
                                        &params_item.macro_enhance,
                                        values.macro_enhance,
                                    );
                                    setter.end_set_parameter(&params_item.macro_enhance);

                                    setter.begin_set_parameter(&params_item.macro_control);
                                    setter.set_parameter(
                                        &params_item.macro_control,
                                        values.macro_control,
                                    );
                                    setter.end_set_parameter(&params_item.macro_control);
                                }

                                cx.emit(PopupEvent::Close);
                            });
                    }
                })
                .class("dropdown-options");
            },
        )
        .class("dropdown-box");
    })
    .class("dropdown-row")
    .class("dsp-preset-dropdown")
}

// CUSTOM VISUAL WIDGETS
pub struct SliderVisuals {
    params: Arc<VoiceParams>,
    param_id: ParamId,
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
        if matches!(
            self.param_id,
            ParamId::MacroDistance | ParamId::MacroClarity | ParamId::MacroConsistency
        ) {
            return;
        }

        let b = cx.bounds();
        let val = match self.param_id {
            ParamId::NoiseReduction => self.params.noise_reduction.modulated_normalized_value(),
            ParamId::RumbleAmount => self.params.rumble_amount.modulated_normalized_value(),
            ParamId::HissAmount => self.params.hiss_amount.modulated_normalized_value(),
            ParamId::NoiseLearnAmount => {
                self.params.noise_learn_amount.modulated_normalized_value()
            }
            ParamId::ReverbReduction => self.params.reverb_reduction.modulated_normalized_value(),
            ParamId::Clarity => self.params.clarity.modulated_normalized_value(),
            ParamId::Proximity => self.params.proximity.modulated_normalized_value(),
            ParamId::DeEsser => self.params.de_esser.modulated_normalized_value(),
            ParamId::Leveler => self.params.leveler.modulated_normalized_value(),
            ParamId::OutputGain => self.params.output_gain.modulated_normalized_value(),
            ParamId::BreathControl => self.params.breath_control.modulated_normalized_value(),
            ParamId::MacroDistance => self.params.macro_clean.modulated_normalized_value(),
            ParamId::MacroClarity => self.params.macro_enhance.modulated_normalized_value(),
            ParamId::MacroConsistency => self.params.macro_control.modulated_normalized_value(),
        };

        let mut bg = nih_plug_vizia::vizia::vg::Path::new();
        bg.rounded_rect(b.x, b.y, b.w, b.h, 3.0);
        canvas.fill_path(&bg, &nih_plug_vizia::vizia::vg::Paint::color(nih_plug_vizia::vizia::vg::Color::rgb(30, 41, 59)));
        canvas.stroke_path(
            &bg,
            &nih_plug_vizia::vizia::vg::Paint::color(nih_plug_vizia::vizia::vg::Color::rgb(51, 65, 85)).with_line_width(1.0),
        );

        if val > 0.0 {
            let mut f = nih_plug_vizia::vizia::vg::Path::new();
            f.rounded_rect(b.x, b.y, b.w * val, b.h, 3.0);
            canvas.fill_path(&f, &nih_plug_vizia::vizia::vg::Paint::color(nih_plug_vizia::vizia::vg::Color::rgba(59, 130, 246, 200)));
        }
    }
}

pub struct DialVisuals {
    params: Arc<VoiceParams>,
    param_id: ParamId,
}

impl DialVisuals {
    pub fn new(cx: &mut Context, params: Arc<VoiceParams>, param_id: ParamId) -> Handle<'_, Self> {
        Self { params, param_id }.build(cx, |_| {})
    }
}

impl View for DialVisuals {
    fn element(&self) -> Option<&'static str> {
        Some("dial-visuals")
    }

    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let b = cx.bounds();

        let val = match self.param_id {
            ParamId::MacroDistance => self.params.macro_clean.modulated_normalized_value(),
            ParamId::MacroClarity => self.params.macro_enhance.modulated_normalized_value(),
            ParamId::MacroConsistency => self.params.macro_control.modulated_normalized_value(),
            _ => 0.0,
        }
        .clamp(0.0, 1.0);

        let size = b.w.min(b.h);
        let radius = size * 0.35;
        let cx0 = b.x + b.w * 0.5;
        let cy0 = b.y + b.h * 0.5;

        let start_angle = -225.0_f32.to_radians();
        let end_angle = 45.0_f32.to_radians();
        let current_angle = start_angle + (end_angle - start_angle) * val;

        let mut track = nih_plug_vizia::vizia::vg::Path::new();
        track.arc(cx0, cy0, radius, start_angle, end_angle, nih_plug_vizia::vizia::vg::Solidity::Hole);
        canvas.stroke_path(
            &track,
            &nih_plug_vizia::vizia::vg::Paint::color(nih_plug_vizia::vizia::vg::Color::rgb(30, 41, 59))
                .with_line_width(8.0)
                .with_line_cap(nih_plug_vizia::vizia::vg::LineCap::Round),
        );

        let mut active = nih_plug_vizia::vizia::vg::Path::new();
        active.arc(
            cx0,
            cy0,
            radius,
            start_angle,
            current_angle,
            nih_plug_vizia::vizia::vg::Solidity::Hole,
        );
        canvas.stroke_path(
            &active,
            &nih_plug_vizia::vizia::vg::Paint::color(nih_plug_vizia::vizia::vg::Color::rgb(59, 130, 246))
                .with_line_width(8.0)
                .with_line_cap(nih_plug_vizia::vizia::vg::LineCap::Round),
        );

        let knob_radius = size * 0.32;
        let mut knob = nih_plug_vizia::vizia::vg::Path::new();
        knob.circle(cx0, cy0, knob_radius);
        canvas.fill_path(&knob, &nih_plug_vizia::vizia::vg::Paint::color(nih_plug_vizia::vizia::vg::Color::rgb(15, 23, 42)));
        canvas.stroke_path(
            &knob,
            &nih_plug_vizia::vizia::vg::Paint::color(nih_plug_vizia::vizia::vg::Color::rgb(51, 65, 85)).with_line_width(2.0),
        );

        let marker_radius = 3.0;
        let marker_r = knob_radius - 6.0;
        let mx = cx0 + current_angle.cos() * marker_r;
        let my = cy0 + current_angle.sin() * marker_r;

        let mut marker = nih_plug_vizia::vizia::vg::Path::new();
        marker.circle(mx, my, marker_radius);
        canvas.fill_path(&marker, &nih_plug_vizia::vizia::vg::Paint::color(nih_plug_vizia::vizia::vg::Color::white()));
    }
}
