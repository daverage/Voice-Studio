//! Advanced mode UI builders
//!
//! Provides tab-based UI for detailed parameter control.
//!
//! Tabs:
//! - Clean & Repair: Static and adaptive noise reduction
//! - Shape & Polish: Proximity and clarity shaping

use nih_plug::prelude::GuiContext;
use nih_plug_vizia::vizia::prelude::*;
use std::sync::Arc;
use crate::VoiceParams;
use crate::meters::Meters;
use crate::ui::ParamId;
use crate::ui::components::{create_slider, create_momentary_button};

pub fn build_clean_repair_tab(
    cx: &mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
    meters: Arc<Meters>,
) -> Handle<'_, HStack> {
    HStack::new(cx, move |cx| {
        // Column 1: Static Cleanup
        VStack::new(cx, |cx| {
            create_slider(
                cx,
                "Rumble",
                params.clone(),
                gui.clone(),
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
                params.clone(),
                gui.clone(),
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
                    params.clone(),
                    gui.clone(),
                    ParamId::NoiseLearnAmount,
                    |p| &p.noise_learn_amount,
                );

                HStack::new(cx, |cx| {
                    HStack::new(cx, |cx| {
                        create_momentary_button(
                            cx,
                            "Learn",
                            params.clone(),
                            gui.clone(),
                            |p| &p.noise_learn_trigger,
                        );

                        create_momentary_button(
                            cx,
                            "Clear",
                            params.clone(),
                            gui.clone(),
                            |p| &p.noise_learn_clear,
                        );
                    })
                    .class("output-actions");

                    VStack::new(cx, |cx| {
                        Label::new(cx, "Quality").class("mini-label");
                        crate::ui::meters::NoiseLearnQualityMeter::new(cx, meters.clone())
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
                params.clone(),
                gui.clone(),
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
                params.clone(),
                gui.clone(),
                ParamId::ReverbReduction,
                |p| &p.reverb_reduction,
            )
            .tooltip(|cx| {
                Label::new(cx, "Reduces room reflections and resonant coloration.");
            });

            create_slider(
                cx,
                "Breath Control",
                params.clone(),
                gui.clone(),
                ParamId::BreathControl,
                |p| &p.breath_control,
            )
            .tooltip(|cx| {
                Label::new(
                    cx,
                    "Automatically attenuates breaths and mouth noise between words.",
                );
            });
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
