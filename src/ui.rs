#[cfg(feature = "debug")]
use crate::vs_log;

use crate::macro_controller;
use crate::meters::Meters;
use crate::presets::{DspPreset, OutputPreset};
use crate::version::{spawn_version_check, VersionEvent, VersionStatus, VersionUiState};
use crate::VoiceParams;

use nih_plug::params::Param;
use nih_plug::prelude::{GuiContext, ParamSetter};
use nih_plug_vizia::vizia::binding::Map;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::vizia::vg;
use nih_plug_vizia::widgets::param_base::ParamWidgetBase;
use nih_plug_vizia::widgets::*;

use std::sync::{Arc, Mutex};
#[cfg(feature = "debug")]
use std::time::Duration;

// ============================================================================
// STYLES
// ============================================================================

const STYLE: &str = include_str!("ui.css");

// ============================================================================
// DATA MODEL
// ============================================================================

#[derive(Lens, Clone)]
pub struct VoiceStudioData {
    pub params: Arc<VoiceParams>,
    pub advanced_tab: AdvancedTab,
    pub version_info: VersionUiState,
    #[cfg(feature = "debug")]
    pub css_temp_path: Arc<Mutex<std::path::PathBuf>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Data)]
pub enum AdvancedTab {
    CleanRepair,
    ShapePolish,
}

impl Model for VoiceStudioData {
    #[allow(unused_variables)]
    fn event(&mut self, cx: &mut EventContext, event: &mut Event) {
        event.map(|advanced_tab_event, _| match advanced_tab_event {
            AdvancedTabEvent::SetTab(tab) => self.advanced_tab = *tab,
        });

        #[cfg(feature = "debug")]
        event.map(|css_event, _| match css_event {
            CssEditorEvent::OpenExternalEditor => {
                let theme_path = resolve_theme_css_path();
                let css_path = if let Some(path) = theme_path {
                    if path.exists() {
                        vs_log!("[CSS Editor] Opening existing file: {:?}", path);
                        path
                    } else {
                        vs_log!("[CSS Editor] Creating new file: {:?}", path);
                        if let Some(parent) = path.parent() {
                            if let Err(e) = std::fs::create_dir_all(parent) {
                                vs_log!("[CSS Editor] Failed to create directory: {}", e);
                            }
                        }
                        if let Err(e) = std::fs::write(&path, STYLE) {
                            vs_log!("[CSS Editor] Failed to write CSS file: {}", e);
                        } else {
                            vs_log!("[CSS Editor] CSS file written successfully");
                        }
                        path
                    }
                } else {
                    let temp_path = std::env::temp_dir().join("voice_studio_ui.css");
                    vs_log!("[CSS Editor] Using temp file: {:?}", temp_path);
                    if let Err(e) = std::fs::write(&temp_path, STYLE) {
                        vs_log!("[CSS Editor] Failed to write temp CSS file: {}", e);
                    }
                    temp_path
                };

                // Store the path for reload
                if let Ok(mut path) = self.css_temp_path.lock() {
                    *path = css_path.clone();
                }

                vs_log!("[CSS Editor] Attempting to open: {:?}", css_path);

                // Open in system default editor
                #[cfg(target_os = "macos")]
                {
                    match std::process::Command::new("open")
                        .arg("-t")
                        .arg(&css_path)
                        .spawn()
                    {
                        Ok(_) => vs_log!("[CSS Editor] Editor launched successfully"),
                        Err(e) => vs_log!("[CSS Editor] Failed to open editor: {}", e),
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    match std::process::Command::new("xdg-open")
                        .arg(&css_path)
                        .spawn()
                    {
                        Ok(_) => vs_log!("[CSS Editor] Editor launched successfully"),
                        Err(e) => vs_log!("[CSS Editor] Failed to open editor: {}", e),
                    }
                }
                #[cfg(target_os = "windows")]
                {
                    match std::process::Command::new("notepad").arg(&css_path).spawn() {
                        Ok(_) => vs_log!("[CSS Editor] Editor launched successfully"),
                        Err(e) => vs_log!("[CSS Editor] Failed to open editor: {}", e),
                    }
                }
            }
            CssEditorEvent::ReloadStyles => {
                // Reload stylesheets (re-reads any file-based styles)
                if let Err(e) = cx.reload_styles() {
                    vs_log!("Failed to reload styles: {}", e);
                }
                cx.needs_relayout();
                cx.needs_redraw();
            }
        });
        event.map(|version_event, _| match version_event {
            VersionEvent::Update(info) => {
                self.version_info = info.clone();
                cx.needs_redraw();
            }
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AdvancedTabEvent {
    SetTab(AdvancedTab),
}

#[cfg(feature = "debug")]
#[derive(Debug, Clone, PartialEq)]
pub enum CssEditorEvent {
    OpenExternalEditor,
    ReloadStyles,
}

// NOTE: kept from your file (even if unused)
#[allow(dead_code)]
type VoiceParamsBoolLens = Map<Wrapper<voice_studio_data_derived_lenses::params>, bool>;

// ============================================================================
// METERS
// ============================================================================

#[cfg(feature = "debug")]
fn resolve_theme_css_path() -> Option<std::path::PathBuf> {
    // Get the path to the VST binary
    // macOS: .../vxcleaner/vxcleaner.vst3/Contents/MacOS/vxcleaner
    // Linux: ~/.vst3/vxcleaner.vst3/x86_64-linux/vxcleaner.so
    // Windows: .../VST3/vxcleaner.vst3/Contents/x86_64-win/vxcleaner.vst3
    let exe_path = std::env::current_exe().ok()?;

    // Navigate up to find the bundle root (vxcleaner folder or vxcleaner.vst3)
    // On macOS we need to go: MacOS -> Contents -> vxcleaner.vst3 -> vxcleaner (parent)
    #[cfg(target_os = "macos")]
    {
        let macos_dir = exe_path.parent()?; // Contents/MacOS
        let contents_dir = macos_dir.parent()?; // Contents
        let vst_bundle = contents_dir.parent()?; // vxcleaner.vst3
        let bundle_root = vst_bundle.parent()?; // vxcleaner (outer folder)
        Some(bundle_root.join("themes").join("default").join("ui.css"))
    }

    #[cfg(target_os = "linux")]
    {
        let arch_dir = exe_path.parent()?; // x86_64-linux
        let vst_bundle = arch_dir.parent()?; // vxcleaner.vst3
        Some(vst_bundle.join("themes").join("default").join("ui.css"))
    }

    #[cfg(target_os = "windows")]
    {
        let arch_dir = exe_path.parent()?; // x86_64-win
        let contents_dir = arch_dir.parent()?; // Contents
        let vst_bundle = contents_dir.parent()?; // vxcleaner.vst3
        Some(vst_bundle.join("themes").join("default").join("ui.css"))
    }
}

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
    GainReduction,
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
        let b = cx.bounds();
        let is_gr = matches!(self.meter_type, MeterType::GainReduction);

        let level = match self.meter_type {
            MeterType::InputL => self.meters.get_input_peak_l(),
            MeterType::InputR => self.meters.get_input_peak_r(),
            MeterType::OutputL => self.meters.get_output_peak_l(),
            MeterType::OutputR => self.meters.get_output_peak_r(),
            MeterType::GainReduction => {
                0.5 * (self.meters.get_gain_reduction_l() + self.meters.get_gain_reduction_r())
            }
        };

        let norm = if is_gr {
            (level / 20.0).clamp(0.0, 1.0)
        } else {
            ((level + 60.0) / 60.0).clamp(0.0, 1.0)
        };

        // background
        let mut bg = vg::Path::new();
        bg.rect(b.x, b.y, b.w, b.h);
        canvas.fill_path(&bg, &vg::Paint::color(vg::Color::rgb(15, 23, 42)));
        canvas.stroke_path(
            &bg,
            &vg::Paint::color(vg::Color::rgb(51, 65, 85)).with_line_width(1.0),
        );

        // fill
        if norm > 0.001 {
            let fh = b.h * norm;
            let fy = b.y + (b.h - fh);

            let mut f = vg::Path::new();
            f.rect(b.x + 1.0, fy, b.w - 2.0, fh);

            let paint = if is_gr {
                vg::Paint::linear_gradient(
                    b.x,
                    b.y,
                    b.x,
                    b.y + b.h,
                    vg::Color::rgb(239, 68, 68),
                    vg::Color::rgb(249, 115, 22),
                )
            } else {
                vg::Paint::linear_gradient(
                    b.x,
                    b.y + b.h,
                    b.x,
                    b.y,
                    vg::Color::rgb(34, 197, 94),
                    vg::Color::rgb(239, 68, 68),
                )
            };

            canvas.fill_path(&f, &paint);
        }

        // ticks
        let mut l = vg::Path::new();
        let step = b.h / 20.0;
        for i in 1..20 {
            let y = b.y + i as f32 * step;
            l.move_to(b.x, y);
            l.line_to(b.x + b.w, y);
        }

        canvas.stroke_path(
            &l,
            &vg::Paint::color(vg::Color::rgba(0, 0, 0, 100)).with_line_width(1.0),
        );
    }
}

// ============================================================================
// NOISE LEARN QUALITY METER
// ============================================================================

pub struct NoiseLearnQualityMeter {
    meters: Arc<Meters>,
}

impl NoiseLearnQualityMeter {
    pub fn new(cx: &mut Context, meters: Arc<Meters>) -> Handle<'_, Self> {
        Self { meters }.build(cx, |_| {})
    }
}

impl View for NoiseLearnQualityMeter {
    fn element(&self) -> Option<&'static str> {
        Some("noise-learn-meter")
    }

    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let b = cx.bounds();
        let quality = self.meters.get_noise_learn_quality().clamp(0.0, 1.0);

        // Background
        let mut bg = vg::Path::new();
        bg.rounded_rect(b.x, b.y, b.w, b.h, 2.0);
        canvas.fill_path(&bg, &vg::Paint::color(vg::Color::rgb(30, 41, 59)));

        // Fill based on quality
        if quality > 0.01 {
            let mut fill = vg::Path::new();
            fill.rounded_rect(b.x, b.y, b.w * quality, b.h, 2.0);

            // Color logic: < 0.3 grey, 0.3-0.7 yellow, > 0.7 green
            let color = if quality < 0.3 {
                vg::Color::rgb(100, 116, 139) // Slate-500
            } else if quality < 0.7 {
                vg::Color::rgb(234, 179, 8) // Yellow-500
            } else {
                vg::Color::rgb(34, 197, 94) // Green-500
            };

            canvas.fill_path(&fill, &vg::Paint::color(color));
        }

        // Border
        canvas.stroke_path(
            &bg,
            &vg::Paint::color(vg::Color::rgb(71, 85, 105)).with_line_width(1.0),
        );
    }
}

// ============================================================================
// EFFECT ACTIVITY LEDS (shows how much processing is happening)
// ============================================================================

pub struct NoiseFloorLeds {
    meters: Arc<Meters>,
}

impl NoiseFloorLeds {
    pub fn new(cx: &mut Context, meters: Arc<Meters>) -> Handle<'_, Self> {
        Self { meters }.build(cx, |_| {})
    }
}

impl View for NoiseFloorLeds {
    fn element(&self) -> Option<&'static str> {
        Some("nf-leds")
    }

    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let b = cx.bounds();

        // Read gain reduction from compressor/leveler (in dB, positive values)
        // Shows how much the plugin is actively processing/reducing the signal
        let gr_l = self.meters.get_gain_reduction_l();
        let gr_r = self.meters.get_gain_reduction_r();
        let gr_db = gr_l.max(gr_r); // Use max for linked stereo

        let radius = b.h / 2.0 - 1.0;
        let spacing = 6.0;
        let start_x = b.x + (b.w - (radius * 2.0 * 3.0 + spacing * 2.0)) / 2.0 + radius;
        let cy = b.y + b.h / 2.0;

        let dark_green = vg::Color::rgb(20, 83, 45);
        let bright_green = vg::Color::rgb(34, 225, 94);
        let dark_yellow = vg::Color::rgb(113, 63, 18);
        let bright_yellow = vg::Color::rgb(250, 204, 21);
        let dark_red = vg::Color::rgb(127, 29, 29);
        let bright_red = vg::Color::rgb(239, 68, 68);

        // Green = idle/minimal processing (0-2dB GR)
        // Yellow = moderate processing (2-6dB GR)
        // Red = heavy processing (>6dB GR)
        let c1 = if gr_db > 0.5 {
            bright_green
        } else {
            dark_green
        };
        let c2 = if gr_db > 2.0 {
            bright_yellow
        } else {
            dark_yellow
        };
        let c3 = if gr_db > 6.0 { bright_red } else { dark_red };

        let colors = [c1, c2, c3];

        for (i, col) in colors.iter().enumerate() {
            let cx0 = start_x + (i as f32 * (radius * 2.0 + spacing));
            let mut path = vg::Path::new();
            path.circle(cx0, cy, radius);
            canvas.fill_path(&path, &vg::Paint::color(*col));

            if col.g > 0.5 || col.r > 0.5 {
                canvas.global_composite_operation(vg::CompositeOperation::Lighter);
                let mut glow = vg::Path::new();
                glow.circle(cx0, cy, radius * 1.5);
                canvas.fill_path(
                    &glow,
                    &vg::Paint::color(vg::Color::rgba(col.r as u8, col.g as u8, col.b as u8, 100)),
                );
                canvas.global_composite_operation(vg::CompositeOperation::SourceOver);
            }
        }
    }
}

// ============================================================================
// SLIDER VISUALS + DIAL VISUALS
// ============================================================================

#[derive(Clone, Copy, PartialEq, Data)]
pub(crate) enum ParamId {
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

        let mut bg = vg::Path::new();
        bg.rounded_rect(b.x, b.y, b.w, b.h, 3.0);
        canvas.fill_path(&bg, &vg::Paint::color(vg::Color::rgb(30, 41, 59)));
        canvas.stroke_path(
            &bg,
            &vg::Paint::color(vg::Color::rgb(51, 65, 85)).with_line_width(1.0),
        );

        if val > 0.0 {
            let mut f = vg::Path::new();
            f.rounded_rect(b.x, b.y, b.w * val, b.h, 3.0);
            canvas.fill_path(&f, &vg::Paint::color(vg::Color::rgba(59, 130, 246, 200)));
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

        let mut track = vg::Path::new();
        track.arc(cx0, cy0, radius, start_angle, end_angle, vg::Solidity::Hole);
        canvas.stroke_path(
            &track,
            &vg::Paint::color(vg::Color::rgb(30, 41, 59))
                .with_line_width(8.0)
                .with_line_cap(vg::LineCap::Round),
        );

        let mut active = vg::Path::new();
        active.arc(
            cx0,
            cy0,
            radius,
            start_angle,
            current_angle,
            vg::Solidity::Hole,
        );
        canvas.stroke_path(
            &active,
            &vg::Paint::color(vg::Color::rgb(59, 130, 246))
                .with_line_width(8.0)
                .with_line_cap(vg::LineCap::Round),
        );

        let knob_radius = size * 0.32;
        let mut knob = vg::Path::new();
        knob.circle(cx0, cy0, knob_radius);
        canvas.fill_path(&knob, &vg::Paint::color(vg::Color::rgb(15, 23, 42)));
        canvas.stroke_path(
            &knob,
            &vg::Paint::color(vg::Color::rgb(51, 65, 85)).with_line_width(2.0),
        );

        let marker_radius = 3.0;
        let marker_r = knob_radius - 6.0;
        let mx = cx0 + current_angle.cos() * marker_r;
        let my = cy0 + current_angle.sin() * marker_r;

        let mut marker = vg::Path::new();
        marker.circle(mx, my, marker_radius);
        canvas.fill_path(&marker, &vg::Paint::color(vg::Color::white()));
    }
}

// ============================================================================
// WIDGET HELPERS
// ============================================================================

fn create_slider<'a, P>(
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
            let lens = ParamWidgetBase::make_lens(VoiceStudioData::params, map, |p: &P| {
                p.normalized_value_to_string(p.unmodulated_normalized_value(), true)
            });
            Label::new(cx, lens)
                .class("slider-value")
                .class("adv-value")
                .hoverable(false);

            ParamSlider::new(cx, VoiceStudioData::params, move |p| map(p))
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

fn create_macro_dial<'a, P>(
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
            let lens = ParamWidgetBase::make_lens(VoiceStudioData::params, map, |p: &P| {
                p.normalized_value_to_string(p.unmodulated_normalized_value(), true)
            });
            Label::new(cx, lens).class("dial-value").hoverable(false);

            // Interactive slider (in front, invisible)
            ParamSlider::new(cx, VoiceStudioData::params, move |p| map(p))
                .class("fill-both")
                .class("input-hidden")
                .z_index(1);
        })
        .class("dial-visual");
    })
    .class("dial-container")
}

fn create_dropdown<'a>(
    cx: &'a mut Context,
    label: &'static str,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, HStack> {
    HStack::new(cx, move |cx| {
        Label::new(cx, label).class("dropdown-label");

        let lens = ParamWidgetBase::make_lens(
            VoiceStudioData::params,
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
                    for preset in OutputPreset::all_presets().iter() {
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
}

fn create_dsp_preset_dropdown<'a>(
    cx: &'a mut Context,
    label: &'static str,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
) -> Handle<'a, HStack> {
    HStack::new(cx, move |cx| {
        Label::new(cx, label).class("dropdown-label");

        let lens = ParamWidgetBase::make_lens(
            VoiceStudioData::params,
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
                        DspPreset::Manual,
                        DspPreset::PodcastNoisy,
                        DspPreset::VoiceoverStudio,
                        DspPreset::InterviewOutdoor,
                        DspPreset::BroadcastClean,
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

fn set_macro_mode(params: &Arc<VoiceParams>, gui_context: &Arc<dyn GuiContext>, enabled: bool) {
    let setter = ParamSetter::new(gui_context.as_ref());
    setter.begin_set_parameter(&params.macro_mode);
    setter.set_parameter(&params.macro_mode, enabled);
    setter.end_set_parameter(&params.macro_mode);
}

fn sync_advanced_from_macros(params: &Arc<VoiceParams>, gui_context: &Arc<dyn GuiContext>) {
    let setter = ParamSetter::new(gui_context.as_ref());
    macro_controller::apply_simple_macros(params.as_ref(), &setter);
}

// ============================================================================
// BUILDERS
// ============================================================================

fn build_levels(cx: &mut Context, meters: Arc<Meters>) {
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
                    LevelMeter::new(cx, mi2.clone(), MeterType::InputL).class("meter-track");
                    LevelMeter::new(cx, mi2.clone(), MeterType::InputR).class("meter-track");
                })
                .class("meter-pair");
            })
            .class("meter-col");

            let mg = meters_gr.clone();
            VStack::new(cx, move |cx| {
                Label::new(cx, "GR").class("meter-label");
                LevelMeter::new(cx, mg.clone(), MeterType::GainReduction)
                    .class("meter-track")
                    .class("fill-height");
            })
            .class("meter-col");

            let mo = meters_out.clone();
            VStack::new(cx, move |cx| {
                Label::new(cx, "OUT").class("meter-label");
                let mo2 = mo.clone();
                HStack::new(cx, |cx| {
                    LevelMeter::new(cx, mo2.clone(), MeterType::OutputL).class("meter-track");
                    LevelMeter::new(cx, mo2.clone(), MeterType::OutputR).class("meter-track");
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
            NoiseFloorLeds::new(cx, mf.clone()).class("noise-floor-leds");
        })
        .class("noise-floor-row");
    })
    .class("levels-column");
}

fn build_macro(cx: &mut Context, params: Arc<VoiceParams>, gui: Arc<dyn GuiContext>) {
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
                    sync_advanced_from_macros(&params_sync, &gui_sync);
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
    .class("simple-container");
}

fn build_clean_repair_tab(
    cx: &mut Context,
    params: Arc<VoiceParams>,
    gui: Arc<dyn GuiContext>,
    meters: Arc<Meters>,
) {
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
                        let params_down = params.clone();
                        let gui_down = gui.clone();
                        let params_up = params.clone();
                        let gui_up = gui.clone();

                        HStack::new(cx, |cx| {
                            Label::new(cx, "Learn").hoverable(false);
                        })
                        .class("small-button")
                        .on_mouse_down(move |cx, btn| {
                            if btn == MouseButton::Left {
                                let s = ParamSetter::new(gui_down.as_ref());
                                s.begin_set_parameter(&params_down.noise_learn_trigger);
                                s.set_parameter(&params_down.noise_learn_trigger, true);
                                s.end_set_parameter(&params_down.noise_learn_trigger);
                                cx.capture(); // Critical for momentary behavior
                            }
                        })
                        .on_mouse_up(move |cx, btn| {
                            if btn == MouseButton::Left {
                                let s = ParamSetter::new(gui_up.as_ref());
                                s.begin_set_parameter(&params_up.noise_learn_trigger);
                                s.set_parameter(&params_up.noise_learn_trigger, false);
                                s.end_set_parameter(&params_up.noise_learn_trigger);
                                cx.release();
                            }
                        });

                        let params_clear_down = params.clone();
                        let gui_clear_down = gui.clone();
                        let params_clear_up = params.clone();
                        let gui_clear_up = gui.clone();

                        HStack::new(cx, |cx| {
                            Label::new(cx, "Clear").hoverable(false);
                        })
                        .class("small-button")
                        .on_mouse_down(move |cx, btn| {
                            if btn == MouseButton::Left {
                                let s = ParamSetter::new(gui_clear_down.as_ref());
                                s.begin_set_parameter(&params_clear_down.noise_learn_clear);
                                s.set_parameter(&params_clear_down.noise_learn_clear, true);
                                s.end_set_parameter(&params_clear_down.noise_learn_clear);
                                cx.capture();
                            }
                        })
                        .on_mouse_up(move |cx, btn| {
                            if btn == MouseButton::Left {
                                let s = ParamSetter::new(gui_clear_up.as_ref());
                                s.begin_set_parameter(&params_clear_up.noise_learn_clear);
                                s.set_parameter(&params_clear_up.noise_learn_clear, false);
                                s.end_set_parameter(&params_clear_up.noise_learn_clear);
                                cx.release();
                            }
                        });
                    })
                    .class("output-actions");

                    VStack::new(cx, |cx| {
                        Label::new(cx, "Quality").class("mini-label");
                        NoiseLearnQualityMeter::new(cx, meters.clone())
                            .height(Pixels(8.0)) // Slightly taller for visibility
                            .width(Pixels(40.0));
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
    .class("tab-clean-repair");
}

fn build_shape_polish_tab(cx: &mut Context, params: Arc<VoiceParams>, gui: Arc<dyn GuiContext>) {
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
    .class("tab-shape-polish");
}

fn build_output(cx: &mut Context, params: Arc<VoiceParams>, gui: Arc<dyn GuiContext>) {
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
    .class("output-section");
}

fn build_header(cx: &mut Context, params: Arc<VoiceParams>, gui: Arc<dyn GuiContext>) {
    // Outer clones that live for the lifetime of this header view
    let params_for_binding = params.clone();
    let gui_for_binding = gui.clone();

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
                let params_local = params_for_binding.clone();
                let gui_local = gui_for_binding.clone();

                HStack::new(cx, move |cx| {
                    // Each button gets its own clones so nothing is consumed
                    let p1 = params_local.clone();
                    let g1 = gui_local.clone();
                    Button::new(
                        cx,
                        move |_| set_macro_mode(&p1, &g1, true),
                        |cx| Label::new(cx, "Simple"),
                    )
                    .class(if m {
                        "mode-button-active"
                    } else {
                        "mode-button"
                    });

                    let p2 = params_local.clone();
                    let g2 = gui_local.clone();
                    Button::new(
                        cx,
                        move |_| set_macro_mode(&p2, &g2, false),
                        |cx| Label::new(cx, "Advanced"),
                    )
                    .class(if m {
                        "mode-button"
                    } else {
                        "mode-button-active"
                    });
                })
                .class("mode-group");
            },
        );
    })
    .class("header");
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

fn build_footer(cx: &mut Context, params: Arc<VoiceParams>, gui: Arc<dyn GuiContext>) {
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
                    Label::new(cx, label_text.as_str()).class("version-text").class(
                        if status == VersionStatus::UpdateAvailable {
                            "version-update"
                        } else {
                            "version-normal"
                        },
                    );
                    Label::new(cx, detail_text.as_str()).class("version-detail");
                    if let Some(url) = release_url {
                        Button::new(
                            cx,
                            move |_| open_url(&url),
                            |cx| Label::new(cx, "View Release"),
                        )
                        .class("footer-button")
                        .class("version-release-button");
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
            Button::new(
                cx,
                move |_| {
                    open_url("https://www.marczewski.me.uk/vxcleaner/help.html");
                },
                |cx| Label::new(cx, "Help"),
            )
            .class("footer-button");

            Button::new(
                cx,
                move |_| {
                    let s = ParamSetter::new(gui_reset.as_ref());
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
                    s.set_parameter(&params_reset.final_output_preset, OutputPreset::None);
                    s.end_set_parameter(&params_reset.final_output_preset);

                    s.begin_set_parameter(&params_reset.reset_all);
                    s.set_parameter(&params_reset.reset_all, true);
                    s.end_set_parameter(&params_reset.reset_all);

                    s.begin_set_parameter(&params_reset.reset_all);
                    s.set_parameter(&params_reset.reset_all, false);
                    s.end_set_parameter(&params_reset.reset_all);
                },
                |cx| Label::new(cx, "Reset"),
            )
            .class("reset-button");

            #[cfg(feature = "debug")]
            Button::new(
                cx,
                move |_| {
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
                },
                |cx| Label::new(cx, "Log"),
            )
            .class("footer-button");

            #[cfg(feature = "debug")]
            Button::new(
                cx,
                |cx| cx.emit(CssEditorEvent::OpenExternalEditor),
                |cx| Label::new(cx, "Edit CSS"),
            )
            .class("footer-button");

            #[cfg(feature = "debug")]
            Button::new(
                cx,
                |cx| cx.emit(CssEditorEvent::ReloadStyles),
                |cx| Label::new(cx, "Reload CSS"),
            )
            .class("footer-button");
        })
        .class("footer-buttons");
    })
    .class("footer");
}

// ============================================================================
// MAIN ENTRY
// ============================================================================

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

    let _ = cx.add_stylesheet(STYLE);

    if let Ok(mut guard) = _ui_proxy.lock() {
        *guard = Some(cx.get_proxy());
    }
    spawn_version_check(_ui_proxy.clone());

    #[cfg(feature = "debug")]
    let css_temp_path = {
        let css_path = resolve_theme_css_path()
            .unwrap_or_else(|| std::env::temp_dir().join("voice_studio_ui.css"));

        if !css_path.exists() {
            if let Some(parent) = css_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&css_path, STYLE) {
                vs_log!("Failed to write CSS file: {}", e);
            }
        }

        let _ = cx.add_stylesheet(css_path.clone());
        Arc::new(Mutex::new(css_path))
    };

    VoiceStudioData {
        params: params.clone(),
        advanced_tab: AdvancedTab::CleanRepair,
        version_info: VersionUiState::checking(),
        #[cfg(feature = "debug")]
        css_temp_path,
    }
    .build(cx);

    // Root clones that stay owned by the top-level UI closure
    let params_root = params.clone();
    let meters_root = meters.clone();
    let gui_root = gui_context.clone();

    VStack::new(cx, move |cx| {
        // HEADER
        build_header(cx, params_root.clone(), gui_root.clone());

        // BODY
        let params_for_binding = params_root.clone();
        let meters_for_binding = meters_root.clone();
        let gui_for_binding = gui_root.clone();

        Binding::new(
            cx,
            VoiceStudioData::params.map(|p| p.macro_mode.value()),
            move |cx, lens| {
                let simple = lens.get(cx);

                // Clone inside Binding so we do not move captured Arcs into nested move closures
                let params_local = params_for_binding.clone();
                let meters_local = meters_for_binding.clone();
                let gui_local = gui_for_binding.clone();

                HStack::new(cx, move |cx| {
                    build_levels(cx, meters_local.clone());

                    let p = params_local.clone();
                    let g = gui_local.clone();
                    let m = meters_local.clone();

                    VStack::new(cx, move |cx| {
                        if simple {
                            build_macro(cx, p.clone(), g.clone());
                            Element::new(cx).class("fill-width");
                        } else {
                            // Tab Headers
                            let p_tabs = p.clone();
                            let g_tabs = gui_local.clone();
                            let m_tabs = m.clone();

                            Binding::new(cx, VoiceStudioData::advanced_tab, move |cx, tab_lens| {
                                let current_tab = tab_lens.get(cx);
                                HStack::new(cx, |cx| {
                                    Button::new(
                                        cx,
                                        |ex| {
                                            ex.emit(AdvancedTabEvent::SetTab(
                                                AdvancedTab::CleanRepair,
                                            ))
                                        },
                                        |cx| Label::new(cx, "Clean & Repair"),
                                    )
                                    .class(
                                        if current_tab == AdvancedTab::CleanRepair {
                                            "tab-header-active"
                                        } else {
                                            "tab-header"
                                        },
                                    );

                                    Button::new(
                                        cx,
                                        |ex| {
                                            ex.emit(AdvancedTabEvent::SetTab(
                                                AdvancedTab::ShapePolish,
                                            ))
                                        },
                                        |cx| Label::new(cx, "Shape & Polish"),
                                    )
                                    .class(
                                        if current_tab == AdvancedTab::ShapePolish {
                                            "tab-header-active"
                                        } else {
                                            "tab-header"
                                        },
                                    );
                                })
                                .class("tabs-container");
                            });

                            // Tab Content
                            Binding::new(cx, VoiceStudioData::advanced_tab, move |cx, tab_lens| {
                                let current_tab = tab_lens.get(cx);
                                match current_tab {
                                    AdvancedTab::CleanRepair => build_clean_repair_tab(
                                        cx,
                                        p_tabs.clone(),
                                        g_tabs.clone(),
                                        m_tabs.clone(),
                                    ),
                                    AdvancedTab::ShapePolish => {
                                        build_shape_polish_tab(cx, p_tabs.clone(), g_tabs.clone())
                                    }
                                }
                            });
                        }

                        // Always visible Output Section
                        build_output(cx, p.clone(), gui_local.clone());
                    })
                    .class("columns-container");
                })
                .class("main-view");
            },
        );

        // FOOTER
        build_footer(cx, params_root.clone(), gui_root.clone());
    })
    .class("app-root");
}
