//! Thread-safe metering utilities for real-time audio processing.
//!
//! This module provides custom Vizia widgets for displaying meter data.
//! The underlying data storage is defined in `crate::meters`.

use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::vizia::vg;
use std::sync::Arc;
use crate::meters::Meters;

// ============================================================================
// CUSTOM METER WIDGETS
// ============================================================================

#[derive(Clone, Copy)]
pub enum MeterType {
    InputL,
    InputR,
    OutputL,
    OutputR,
    GainReduction,
}

pub struct LevelMeter {
    meters: Arc<Meters>,
    meter_type: MeterType,
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