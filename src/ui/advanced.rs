//! Advanced mode UI builders
//!
//! Provides tab-based UI for detailed parameter control.
//!
//! Tabs:
//! - Clean & Repair: Static and adaptive noise reduction
//! - Shape & Polish: Proximity and clarity shaping

use crate::meters::Meters;
use crate::ui::components::{create_momentary_button, create_slider, create_toggle_button};
use crate::ui::state::VoiceStudioData;
use crate::ui::ParamId;
use crate::VoiceParams;
use nih_plug::prelude::{GuiContext, ParamSetter};
use nih_plug_vizia::vizia::prelude::*;
use std::sync::Arc;

pub fn build_clean_repair_tab(
    cx: &mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
    meters: Arc<Meters>,
) -> Handle<'_, HStack> {
    let params_root = params.clone();
    let gui_root = gui.clone();
    let meters_root = meters.clone();
    HStack::new(cx, move |cx| {
        let params_left = params_root.clone();
        let gui_left = gui_root.clone();
        let meters_left = meters_root.clone();
        let params_right = params_root.clone();
        let gui_right = gui_root.clone();
        // Column 1: Static Cleanup
        VStack::new(cx, |cx| {
            create_slider(
                cx,
                "Rumble",
                params_left.clone(),
                gui_left.clone(),
                ParamId::RumbleAmount,
                |p| &p.rumble_amount,
            )
            .tooltip(|cx| {
                Label::new(
                    cx,
                    "Removes low-frequency rumble and vibration below the voice.",
                );
            });

            create_slider(
                cx,
                "Hiss",
                params_left.clone(),
                gui_left.clone(),
                ParamId::HissAmount,
                |p| &p.hiss_amount,
            )
            .tooltip(|cx| {
                Label::new(
                    cx,
                    "Reduces high-frequency hiss and air noise without affecting speech clarity.",
                );
            });

            VStack::new(cx, |cx| {
                create_slider(
                    cx,
                    "Static Noise",
                    params_left.clone(),
                    gui_left.clone(),
                    ParamId::NoiseLearnAmount,
                    |p| &p.noise_learn_amount,
                )
                .tooltip(|cx| {
                    Label::new(
                        cx,
                        "Blends learned static noise removal in/out. Learning is automatic when enabled.",
                    );
                });

                let params_actions = params_left.clone();
                let gui_actions = gui_left.clone();
                let meters_actions = meters_left.clone();
                HStack::new(cx, move |cx| {
                    let params_actions = params_actions.clone();
                    let gui_actions = gui_actions.clone();

                    HStack::new(cx, move |cx| {
                        create_momentary_button(
                            cx,
                            "Re-learn",
                            params_actions.clone(),
                            gui_actions.clone(),
                            |p| &p.noise_learn_trigger,
                        )
                        .tooltip(|cx| {
                            Label::new(
                                cx,
                                "Clears the profile and latches a short re-learn window during playback.",
                            );
                        });

                        create_momentary_button(
                            cx,
                            "Clear",
                            params_actions.clone(),
                            gui_actions.clone(),
                            |p| &p.noise_learn_clear,
                        );
                    })
                    .class("output-actions");

                    VStack::new(cx, |cx| {
                        Label::new(cx, "Quality").class("mini-label");
                        crate::ui::meters::NoiseLearnQualityMeter::new(cx, meters_actions.clone())
                            .height(Pixels(8.0)) // Slightly taller for visibility
                            .width(Pixels(60.0));
                    })
                    .class("quality-meter-container");
                })
                .class("output-row");

            })
            .class("group-container");
        })
        .class("tab-column")
        .class("adv-column");

        // Column 2: Adaptive Cleanup
        VStack::new(cx, |cx| {
            create_slider(
                cx,
                "Noise Reduction",
                params_right.clone(),
                gui_right.clone(),
                ParamId::NoiseReduction,
                |p| &p.noise_reduction,
            )
            .tooltip(|cx| {
                Label::new(
                    cx,
                    "Reduces steady background noise using adaptive hybrid suppression.",
                );
            });

            create_slider(
                cx,
                "De-Verb",
                params_right.clone(),
                gui_right.clone(),
                ParamId::ReverbReduction,
                |p| &p.reverb_reduction,
            )
            .tooltip(|cx| {
                Label::new(cx, "Reduces room reflections and resonant coloration.");
            });

            create_slider(
                cx,
                "Breath Control",
                params_right.clone(),
                gui_right.clone(),
                ParamId::BreathControl,
                |p| &p.breath_control,
            )
            .tooltip(|cx| {
                Label::new(
                    cx,
                    "Automatically attenuates breaths and mouth noise between words.",
                );
            });

            let params_toggles = params_right.clone();
            let gui_toggles = gui_right.clone();
            Binding::new(
                cx,
                VoiceStudioData::params.map(|p| {
                    (
                        p.post_noise_hf_bias.value(),
                        p.hidden_tone_fx_bypass.value(),
                        p.low_end_protect.value(),
                    )
                }),
                move |cx, lens| {
                    let (hf_bias, bypass_hidden, low_end_protect) = lens.get(cx);
                    let hidden_on = !bypass_hidden;
                    let p = params_toggles.clone();
                    let g = gui_toggles.clone();

                    HStack::new(cx, move |cx| {
                        let p1 = p.clone();
                        let g1 = g.clone();
                        create_toggle_button(
                            cx,
                            "HF Bias",
                            hf_bias,
                            "small-button-active",
                            "small-button",
                            move |_| {
                                let s = ParamSetter::new(g1.as_ref());
                                let param = &p1.post_noise_hf_bias;
                                s.begin_set_parameter(param);
                                s.set_parameter(param, !hf_bias);
                                s.end_set_parameter(param);
                            },
                        )
                        .class("hf-bias-toggle")
                        .tooltip(|cx| {
                            Label::new(
                                cx,
                                "Applies gentle HF-focused cleanup in the post-noise pass.",
                            );
                        });

                        let p2 = p.clone();
                        let g2 = g.clone();
                        create_toggle_button(
                            cx,
                            "Hidden FX",
                            hidden_on,
                            "small-button-active",
                            "small-button",
                            move |_| {
                                let s = ParamSetter::new(g2.as_ref());
                                let param = &p2.hidden_tone_fx_bypass;
                                s.begin_set_parameter(param);
                                s.set_parameter(param, hidden_on);
                                s.end_set_parameter(param);
                            },
                        )
                        .class("hidden-fx-toggle")
                        .tooltip(|cx| {
                            Label::new(
                                cx,
                                "Hidden tone stages on. Toggle off to bypass (pink bias, recovery, post-cleanup, guardrails).",
                            );
                        });

                        let p3 = p.clone();
                        let g3 = g.clone();
                        create_toggle_button(
                            cx,
                            "Low End",
                            low_end_protect,
                            "small-button-active",
                            "small-button",
                            move |_| {
                                let s = ParamSetter::new(g3.as_ref());
                                let param = &p3.low_end_protect;
                                s.begin_set_parameter(param);
                                s.set_parameter(param, !low_end_protect);
                                s.end_set_parameter(param);
                            },
                        )
                        .class("low-end-toggle")
                        .tooltip(|cx| {
                            Label::new(
                                cx,
                                "Protects low-end voiced energy inside the denoiser (disable to avoid bass bump).",
                            );
                        });
                    })
                    .class("output-actions");
                },
            );
        })
        .class("tab-column")
        .class("adv-column");
    })
    .class("adv-columns")
    .class("tab-content")
    .class("tab-clean-repair")
}

pub fn build_shape_polish_tab(
    cx: &mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'_, HStack> {
    HStack::new(cx, move |cx| {
        VStack::new(cx, |cx| {
            create_slider(
                cx,
                "Proximity",
                params.clone(),
                gui.clone(),
                ParamId::Proximity,
                |p| &p.proximity,
            )
            .tooltip(|cx| {
                Label::new(
                    cx,
                    "Adjusts perceived microphone distance and vocal warmth.",
                );
            });

            create_slider(
                cx,
                "Clarity",
                params.clone(),
                gui.clone(),
                ParamId::Clarity,
                |p| &p.clarity,
            )
            .tooltip(|cx| {
                Label::new(
                    cx,
                    "Reduces low-mid muddiness to improve speech definition.",
                );
            });
        })
        .class("tab-column")
        .class("adv-column");

        VStack::new(cx, |cx| {
            create_slider(
                cx,
                "De-Ess",
                params.clone(),
                gui.clone(),
                ParamId::DeEsser,
                |p| &p.de_esser,
            );

            create_slider(
                cx,
                "Leveler",
                params.clone(),
                gui.clone(),
                ParamId::Leveler,
                |p| &p.leveler,
            );
        })
        .class("tab-column")
        .class("adv-column");
    })
    .class("adv-columns")
    .class("tab-content")
    .class("tab-shape-polish")
}
