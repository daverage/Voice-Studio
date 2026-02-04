//! Layout builders for the Voice Studio UI
//!
//! This module provides functions for building the high-level UI structure:
//! - Header with title and mode toggle
//! - Body with levels, macro/advanced sections, and output
//! - Footer with help, reset, and debug buttons

use crate::meters::Meters;
use crate::ui::advanced::{build_clean_repair_tab, build_shape_polish_tab};
use crate::ui::components::{
    create_button, create_dropdown, create_dsp_preset_dropdown, create_macro_dial, create_slider,
    create_toggle_button,
};
use crate::ui::state::{AdvancedTab, AdvancedTabEvent, VoiceStudioData};
use crate::ui::ParamId;
use crate::VoiceParams;
use nih_plug::prelude::GuiContext;
use nih_plug_vizia::vizia::prelude::ContextProxy;
use nih_plug_vizia::vizia::prelude::*;
use std::sync::Arc;

pub fn build_header<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, HStack> {
    HStack::new(cx, move |cx| {
        VStack::new(cx, move |cx| {
            Label::new(cx, "VxCLEANER").class("header-title");
            Label::new(cx, "Vocal Restoration").class("header-sub");
        })
        .class("header-title-stack");

        Element::new(cx).class("fill-width");

        Binding::new(
            cx,
            VoiceStudioData::params.map(|p| p.macro_mode.value()),
            move |cx, lens| {
                let m = lens.get(cx);

                // Clone inside Binding so we do not move captured Arcs into nested move closures
                let params_local = params.clone();
                let gui_local = gui.clone();

                HStack::new(cx, move |cx| {
                    // Each button gets its own clones so nothing is consumed
                    let p1 = params_local.clone();
                    let g1 = gui_local.clone();
                    create_toggle_button(
                        cx,
                        "Simple",
                        m,
                        "mode-button-active",
                        "mode-button",
                        move |_| crate::ui::state::set_macro_mode(&p1, &g1, true),
                    );

                    let p2 = params_local.clone();
                    let g2 = gui_local.clone();
                    create_toggle_button(
                        cx,
                        "Advanced",
                        !m,
                        "mode-button-active",
                        "mode-button",
                        move |_| crate::ui::state::set_macro_mode(&p2, &g2, false),
                    );
                })
                .class("mode-group");
            },
        );
    })
    .class("header")
}

pub fn build_footer<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, HStack> {
    HStack::new(cx, move |cx| {
        Binding::new(
            cx,
            VoiceStudioData::version_info.map(|info| info.clone()),
            move |cx, lens| {
                let info = lens.get(cx);
                let label_text = info.label.clone();
                let detail_text = info.detail.clone();
                let status = info.status;
                let release_url = info.release_url.clone();

                VStack::new(cx, move |cx| {
                    Label::new(cx, label_text.as_str())
                        .class("version-text")
                        .class(
                            if status == crate::version::VersionStatus::UpdateAvailable {
                                "version-update"
                            } else {
                                "version-normal"
                            },
                        );
                    Label::new(cx, detail_text.as_str()).class("version-detail");
                    if let Some(url) = release_url {
                        Label::new(
                            cx,
                            &format!("→ Download {}", url.split('/').last().unwrap_or("latest")),
                        )
                        .class("version-link")
                        .on_press(move |_| open_url(&url));
                    }
                })
                .class("version-stack");
            },
        );

        Element::new(cx).class("fill-width");

        // Split clones for the footer buttons
        let params_reset = params.clone();
        let gui_reset = gui.clone();

        HStack::new(cx, move |cx| {
            create_button(cx, "Help", "footer-button", move |_| {
                open_url("https://www.marczewski.me.uk/vxcleaner/help.html");
            });

            create_button(cx, "Reset", "footer-button", move |_| {
                let s = nih_plug::prelude::ParamSetter::new(gui_reset.as_ref());
                s.begin_set_parameter(&params_reset.noise_reduction);
                s.set_parameter(&params_reset.noise_reduction, 0.0);
                s.end_set_parameter(&params_reset.noise_reduction);

                s.begin_set_parameter(&params_reset.rumble_amount);
                s.set_parameter(&params_reset.rumble_amount, 0.0);
                s.end_set_parameter(&params_reset.rumble_amount);

                s.begin_set_parameter(&params_reset.hiss_amount);
                s.set_parameter(&params_reset.hiss_amount, 0.0);
                s.end_set_parameter(&params_reset.hiss_amount);

                // Reset Static Noise Params
                s.begin_set_parameter(&params_reset.noise_learn_amount);
                s.set_parameter(&params_reset.noise_learn_amount, 0.0);
                s.end_set_parameter(&params_reset.noise_learn_amount);

                s.begin_set_parameter(&params_reset.noise_learn_trigger);
                s.set_parameter(&params_reset.noise_learn_trigger, false);
                s.end_set_parameter(&params_reset.noise_learn_trigger);

                s.begin_set_parameter(&params_reset.noise_learn_clear);
                s.set_parameter(&params_reset.noise_learn_clear, false);
                s.end_set_parameter(&params_reset.noise_learn_clear);

                s.begin_set_parameter(&params_reset.reverb_reduction);
                s.set_parameter(&params_reset.reverb_reduction, 0.0);
                s.end_set_parameter(&params_reset.reverb_reduction);

                s.begin_set_parameter(&params_reset.clarity);
                s.set_parameter(&params_reset.clarity, 0.0);
                s.end_set_parameter(&params_reset.clarity);

                s.begin_set_parameter(&params_reset.proximity);
                s.set_parameter(&params_reset.proximity, 0.0);
                s.end_set_parameter(&params_reset.proximity);

                s.begin_set_parameter(&params_reset.de_esser);
                s.set_parameter(&params_reset.de_esser, 0.0);
                s.end_set_parameter(&params_reset.de_esser);

                s.begin_set_parameter(&params_reset.leveler);
                s.set_parameter(&params_reset.leveler, 0.0);
                s.end_set_parameter(&params_reset.leveler);

                s.begin_set_parameter(&params_reset.output_gain);
                s.set_parameter(&params_reset.output_gain, 0.0);
                s.end_set_parameter(&params_reset.output_gain);

                s.begin_set_parameter(&params_reset.breath_control);
                s.set_parameter(&params_reset.breath_control, 0.25);
                s.end_set_parameter(&params_reset.breath_control);

                s.begin_set_parameter(&params_reset.use_ml);
                s.set_parameter(&params_reset.use_ml, true);
                s.end_set_parameter(&params_reset.use_ml);

                s.begin_set_parameter(&params_reset.macro_mode);
                s.set_parameter(&params_reset.macro_mode, true);
                s.end_set_parameter(&params_reset.macro_mode);

                s.begin_set_parameter(&params_reset.macro_clean);
                s.set_parameter(&params_reset.macro_clean, 0.0);
                s.end_set_parameter(&params_reset.macro_clean);

                s.begin_set_parameter(&params_reset.macro_enhance);
                s.set_parameter(&params_reset.macro_enhance, 0.0);
                s.end_set_parameter(&params_reset.macro_enhance);

                s.begin_set_parameter(&params_reset.macro_control);
                s.set_parameter(&params_reset.macro_control, 0.0);
                s.end_set_parameter(&params_reset.macro_control);

                s.begin_set_parameter(&params_reset.final_output_preset);
                s.set_parameter(
                    &params_reset.final_output_preset,
                    crate::presets::OutputPreset::None,
                );
                s.end_set_parameter(&params_reset.final_output_preset);

                s.begin_set_parameter(&params_reset.reset_all);
                s.set_parameter(&params_reset.reset_all, true);
                s.end_set_parameter(&params_reset.reset_all);

                s.begin_set_parameter(&params_reset.reset_all);
                s.set_parameter(&params_reset.reset_all, false);
                s.end_set_parameter(&params_reset.reset_all);
            });

            #[cfg(feature = "debug")]
            create_button(cx, "Log", "footer-button", move |_| {
                #[cfg(target_os = "macos")]
                {
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
            });

            #[cfg(feature = "debug")]
            create_button(cx, "Edit CSS", "footer-button", move |_| {
                // Get CSS file path in bundle folder
                if let Ok(exe_path) = std::env::current_exe() {
                    #[cfg(target_os = "macos")]
                    let css_path = {
                        if let Some(macos_dir) = exe_path.parent() {
                            if let Some(contents_dir) = macos_dir.parent() {
                                if let Some(vst_bundle) = contents_dir.parent() {
                                    Some(vst_bundle.join("ui.css"))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };

                    #[cfg(not(target_os = "macos"))]
                    let css_path = exe_path.parent().map(|p| p.join("ui.css"));

                    if let Some(path) = css_path {
                        // Create file if it doesn't exist
                        if !path.exists() {
                            let _ = std::fs::write(&path, STYLE);
                        }

                        // Open in system editor
                        #[cfg(target_os = "macos")]
                        {
                            let _ = std::process::Command::new("open")
                                .arg("-t")
                                .arg(&path)
                                .spawn();
                        }

                        #[cfg(target_os = "linux")]
                        {
                            let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
                        }

                        #[cfg(target_os = "windows")]
                        {
                            let _ = std::process::Command::new("notepad").arg(&path).spawn();
                        }
                    }
                }
            });
        })
        .class("footer-buttons");
    })
    .class("footer")
}

pub fn build_body<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
    meters: Arc<Meters>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, HStack> {
    // Root clones that stay owned by the top-level UI closure
    let params_root = params.clone();
    let meters_root = meters.clone();
    let gui_root = gui.clone();

    HStack::new(cx, move |cx| {
        build_levels(cx, meters_root.clone());

        let p = params_root.clone();
        let g = gui_root.clone();
        let m = meters_root.clone();

        VStack::new(cx, move |cx| {
            // Binding to determine if we're in simple or advanced mode
            Binding::new(
                cx,
                VoiceStudioData::params.map(|p| p.macro_mode.value()),
                move |cx, lens| {
                    let simple = lens.get(cx);

                    // Clone inside Binding so we do not move captured Arcs into nested move closures
                    let params_local = p.clone();
                    let meters_local = m.clone();
                    let gui_local = g.clone();

                    if simple {
                        build_macro(cx, params_local.clone(), gui_local.clone());
                        Element::new(cx).class("fill-width");
                    } else {
                        // Tab Headers
                        let p_tabs = params_local.clone();
                        let g_tabs = gui_local.clone();
                        let m_tabs = meters_local.clone();

                        Binding::new(cx, VoiceStudioData::advanced_tab, move |cx, tab_lens| {
                            let current_tab = tab_lens.get(cx);
                            HStack::new(cx, |cx| {
                                create_toggle_button(
                                    cx,
                                    "Clean & Repair",
                                    current_tab == AdvancedTab::CleanRepair,
                                    "tab-header-active",
                                    "tab-header",
                                    |ex| {
                                        ex.emit(AdvancedTabEvent::SetTab(AdvancedTab::CleanRepair))
                                    },
                                );

                                create_toggle_button(
                                    cx,
                                    "Shape & Polish",
                                    current_tab == AdvancedTab::ShapePolish,
                                    "tab-header-active",
                                    "tab-header",
                                    |ex| {
                                        ex.emit(AdvancedTabEvent::SetTab(AdvancedTab::ShapePolish))
                                    },
                                );
                            })
                            .class("tabs-container");
                        });

                        // Tab Content
                        Binding::new(cx, VoiceStudioData::advanced_tab, move |cx, tab_lens| {
                            let current_tab = tab_lens.get(cx);
                            match current_tab {
                                AdvancedTab::CleanRepair => {
                                    build_clean_repair_tab(
                                        cx,
                                        p_tabs.clone(),
                                        g_tabs.clone(),
                                        m_tabs.clone(),
                                    );
                                }
                                AdvancedTab::ShapePolish => {
                                    build_shape_polish_tab(cx, p_tabs.clone(), g_tabs.clone());
                                }
                            }
                        });
                    }

                    // Always visible Output Section
                    build_output(cx, params_local.clone(), gui_local.clone());
                },
            );
        })
        .class("columns-container");
    })
    .class("main-view")
}

pub fn build_levels<'a>(cx: &'a mut Context, meters: Arc<Meters>) -> Handle<'a, VStack> {
    // IMPORTANT: break Arc<Meters> into independent clones so nested move closures don't "consume" it
    let meters_in = meters.clone();
    let meters_gr = meters.clone();
    let meters_out = meters.clone();
    let meters_floor = meters.clone();

    VStack::new(cx, move |cx| {
        Label::new(cx, "LEVELS")
            .class("column-header")
            .class("col-levels");

        HStack::new(cx, move |cx| {
            let mi = meters_in.clone();
            VStack::new(cx, move |cx| {
                Label::new(cx, "IN").class("meter-label");
                let mi2 = mi.clone();
                HStack::new(cx, |cx| {
                    crate::ui::meters::LevelMeter::new(
                        cx,
                        mi2.clone(),
                        crate::ui::meters::MeterType::InputL,
                    )
                    .class("meter-track");
                    crate::ui::meters::LevelMeter::new(
                        cx,
                        mi2.clone(),
                        crate::ui::meters::MeterType::InputR,
                    )
                    .class("meter-track");
                })
                .class("meter-pair");
            })
            .class("meter-col");

            let mg = meters_gr.clone();
            VStack::new(cx, move |cx| {
                Label::new(cx, "GR").class("meter-label");
                crate::ui::meters::LevelMeter::new(
                    cx,
                    mg.clone(),
                    crate::ui::meters::MeterType::GainReduction,
                )
                .class("meter-track")
                .class("fill-height");
            })
            .class("meter-col");

            let mo = meters_out.clone();
            VStack::new(cx, move |cx| {
                Label::new(cx, "OUT").class("meter-label");
                let mo2 = mo.clone();
                HStack::new(cx, |cx| {
                    crate::ui::meters::LevelMeter::new(
                        cx,
                        mo2.clone(),
                        crate::ui::meters::MeterType::OutputL,
                    )
                    .class("meter-track");
                    crate::ui::meters::LevelMeter::new(
                        cx,
                        mo2.clone(),
                        crate::ui::meters::MeterType::OutputR,
                    )
                    .class("meter-track");
                })
                .class("meter-pair");
            })
            .class("meter-col");
        })
        .class("meter-grid");

        Element::new(cx).class("spacer");

        let mf = meters_floor.clone();
        HStack::new(cx, move |cx| {
            Label::new(cx, "ACTIVITY").class("meter-label");
            crate::ui::meters::NoiseFloorLeds::new(cx, mf.clone()).class("noise-floor-leds");
        })
        .class("noise-floor-row");
    })
    .class("levels-column")
}

pub fn build_macro<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, VStack> {
    // Split clones to avoid "use after move" across multiple nested closures
    let params_dials = params.clone();
    let params_dropdown = params.clone();
    let gui_dropdown = gui.clone();
    let params_sync = params.clone();
    let gui_sync = gui.clone();

    VStack::new(cx, move |cx| {
        Binding::new(
            cx,
            VoiceStudioData::params.map(|p| {
                (
                    p.macro_mode.value(),
                    p.macro_clean.value(),
                    p.macro_enhance.value(),
                    p.macro_control.value(),
                )
            }),
            move |cx, lens| {
                let (macro_mode, _, _, _) = lens.get(cx);
                if macro_mode {
                    crate::ui::state::sync_advanced_from_macros(&params_sync, gui_sync.clone());
                }
                Element::new(cx).height(Pixels(0.0)).width(Pixels(0.0));
            },
        );

        Label::new(cx, "EASY CONTROLS")
            .class("column-header")
            .class("col-clean");

        create_dsp_preset_dropdown(
            cx,
            "DSP PRESET",
            params_dropdown.clone(),
            gui_dropdown.clone(),
        );

        Element::new(cx).class("fill-height");

        HStack::new(cx, move |cx| {
            let p = params_dials.clone();
            create_macro_dial(cx, "CLEAN", p.clone(), ParamId::MacroDistance, |pp| {
                &pp.macro_clean
            });
            create_macro_dial(cx, "ENHANCE", p.clone(), ParamId::MacroClarity, |pp| {
                &pp.macro_enhance
            });
            create_macro_dial(cx, "CONTROL", p.clone(), ParamId::MacroConsistency, |pp| {
                &pp.macro_control
            });
        })
        .class("dials-container");

        Element::new(cx).class("fill-height");
    })
    .class("macro-column")
    .class("simple-container")
}

pub fn build_output<'a>(
    cx: &'a mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, VStack> {
    VStack::new(cx, move |cx| {
        Label::new(cx, "OUTPUT")
            .class("column-header")
            .class("output-accent");

        create_slider(
            cx,
            "Gain",
            params.clone(),
            gui.clone(),
            ParamId::OutputGain,
            |p| &p.output_gain,
        );
        create_dropdown(cx, "FINAL OUTPUT", params.clone(), gui.clone());
    })
    .class("output-section")
}

fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .arg("/c")
            .arg("start")
            .arg("")
            .arg(url)
            .spawn();
    }
}

// ============================================================================
// MAIN UI ENTRY POINT
// ============================================================================

#[cfg(feature = "debug")]
use crate::vs_log;

use crate::version::{spawn_version_check, VersionUiState};
use std::sync::Mutex;
#[cfg(feature = "debug")]
use std::time::Duration;

// Include the CSS style
const STYLE: &str = include_str!("../ui.css");

pub fn build_ui(
    cx: &mut Context,
    params: Arc<VoiceParams>,
    meters: Arc<Meters>,
    _ui_proxy: Arc<Mutex<Option<ContextProxy>>>,
    gui_context: Arc<dyn GuiContext>,
) {
    #[cfg(feature = "debug")]
    {
        crate::debug::logger::init_logger();
        let timer = cx.add_timer(Duration::from_millis(250), None, |_, action| {
            if let TimerAction::Tick(_) = action {
                crate::debug::logger::drain_to_file();
            }
        });
        cx.start_timer(timer);
    }

    // In debug mode, try to load CSS from disk first (for live editing)
    #[cfg(feature = "debug")]
    let css_to_load: &'static str = {
        if let Ok(exe_path) = std::env::current_exe() {
            #[cfg(target_os = "macos")]
            let css_path = {
                if let Some(macos_dir) = exe_path.parent() {
                    if let Some(contents_dir) = macos_dir.parent() {
                        if let Some(vst_bundle) = contents_dir.parent() {
                            Some(vst_bundle.join("ui.css"))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            #[cfg(not(target_os = "macos"))]
            let css_path = exe_path.parent().map(|p| p.join("ui.css"));

            if let Some(path) = &css_path {
                // Try to read from disk
                if let Ok(disk_css) = std::fs::read_to_string(path) {
                    vs_log!(
                        "✅ CSS loaded from disk: {:?} ({} bytes)",
                        path,
                        disk_css.len()
                    );
                    // Leak the string to get 'static lifetime (acceptable for stylesheets)
                    Box::leak(disk_css.into_boxed_str())
                } else {
                    // File doesn't exist, write embedded CSS and use it
                    if let Err(e) = std::fs::write(path, STYLE) {
                        vs_log!("Failed to write CSS file: {}", e);
                    } else {
                        vs_log!("CSS file written to: {:?}", path);
                    }
                    STYLE
                }
            } else {
                STYLE
            }
        } else {
            STYLE
        }
    };

    #[cfg(not(feature = "debug"))]
    let css_to_load: &'static str = STYLE;

    // Add stylesheet with error reporting
    match cx.add_stylesheet(css_to_load) {
        Ok(_) => {
            eprintln!("✅ CSS LOADED SUCCESSFULLY - {} bytes", css_to_load.len());
        }
        Err(e) => {
            eprintln!("❌ CSS LOAD FAILED: {:?}", e);
            panic!("FATAL: CSS failed to load: {:?}", e);
        }
    }

    if let Ok(mut guard) = _ui_proxy.lock() {
        *guard = Some(cx.get_proxy());
    }
    spawn_version_check(_ui_proxy.clone());

    crate::ui::state::VoiceStudioData {
        params: params.clone(),
        advanced_tab: crate::ui::state::AdvancedTab::CleanRepair,
        version_info: VersionUiState::checking(),
    }
    .build(cx);

    VStack::new(cx, move |cx| {
        // HEADER
        build_header(cx, params.clone(), gui_context.clone()).class("header");

        // BODY
        build_body(cx, params.clone(), meters.clone(), gui_context.clone()).class("body");

        // FOOTER
        build_footer(cx, params.clone(), gui_context.clone()).class("footer");
    })
    .class("app-root");
}
