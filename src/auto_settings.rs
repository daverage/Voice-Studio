use crate::dsp::LinkedCompressor;

// Constants: unless marked "Must not change", these are tunable for behavior.

// Total learn window length in seconds (full capture before Apply).
// Increasing: longer learn time; decreasing: shorter capture.
// Must not change: UI copy and workflow assume 10 seconds.
const CAPTURE_SECONDS: f32 = 10.0;
// Envelope follower attack for fast RMS tracking.
// Increasing: faster response; decreasing: smoother response.
const FAST_TAU_SEC: f32 = 0.02;
// Envelope follower release for slow RMS tracking.
// Increasing: smoother, less reactive; decreasing: more reactive.
const SLOW_TAU_SEC: f32 = 0.25;
// Minimum consecutive non-zero samples before we declare audio present.
// Increasing: harder to start learning; decreasing: easier to start.
const AUTO_START_MIN_SAMPLES: usize = 1;
// Start gate level threshold.
// Increasing: harder to start learning; decreasing: easier to start.
// Must not change: learning should start as soon as audio is present.
const AUTO_START_LEVEL_THRESHOLD: f32 = 0.0;
// Duration (seconds) below which confidence is reduced.
// Increasing: more conservative on short captures; decreasing: less conservative.
const AUTO_SHORT_CAPTURE_SEC: f32 = 1.5;
// Confidence multiplier applied to short captures.
// Increasing: higher confidence for short captures; decreasing: lower confidence.
const AUTO_SHORT_CAPTURE_CONF_SCALE: f32 = 0.6;
// Small epsilon for safe ratio/log calculations.
// Increasing: more conservative at extremes; decreasing: closer to raw values.
const AUTO_EPS: f32 = 1e-6;
// SNR (dB) above which no noise reduction is suggested.
// Increasing: requires cleaner audio before suggesting zero NR; decreasing: zero NR sooner.
const AUTO_SNR_DB_CLEAN: f32 = 30.0;
// SNR (dB) at which NR reaches maximum suggestion.
// Increasing: stronger NR at cleaner audio; decreasing: stronger NR only when very noisy.
const AUTO_SNR_DB_NOISY: f32 = 12.0;
// Maximum noise reduction suggestion cap.
// Increasing: allows more NR; decreasing: caps NR lower.
const AUTO_NOISE_MAX: f32 = 0.7;
// Reverberance ratio (dB) below which no de-verb is suggested.
// Increasing: de-verb triggers sooner; decreasing: requires more reverb to trigger.
const AUTO_REVERB_DB_DRY: f32 = -24.0;
// Reverberance ratio (dB) at which de-verb reaches maximum suggestion.
// Increasing: full de-verb at wetter audio; decreasing: full de-verb sooner.
const AUTO_REVERB_DB_WET: f32 = -12.0;
// Maximum de-verb suggestion cap.
// Increasing: allows more de-verb; decreasing: caps de-verb lower.
const AUTO_REVERB_MAX: f32 = 0.6;
// De-esser ratio (must match DSP).
// Increasing: stronger reduction per dB over threshold; decreasing: gentler.
// Must not change: this must match the DSP ratio for consistency.
const AUTO_DE_ESS_RATIO: f32 = 6.0;
// Maximum de-esser reduction in dB (must match DSP: 18 dB per amount=1.0).
// Must not change: this is the DSP contract.
const AUTO_DE_ESS_MAX_REDUCTION_DB: f32 = 18.0;
// Maximum de-esser suggestion cap.
// Increasing: allows more de-essing; decreasing: caps de-essing lower.
const AUTO_DE_ESS_MAX: f32 = 0.8;
// Leveler target reduction (dB) at which amount reaches 1.0.
// Increasing: more conservative leveler suggestion; decreasing: stronger suggestion.
const AUTO_LEVELER_TARGET_REDUCTION_DB: f32 = 8.0;
// Leveler max suggestion cap.
// Increasing: allows more leveler; decreasing: caps leveler lower.
const AUTO_LEVELER_MAX: f32 = 0.6;
// Metric stability threshold (std dev) for confidence drop-off (unitless metrics).
// Increasing: more tolerant of unstable metrics; decreasing: more conservative.
const AUTO_METRIC_STD_MAX: f32 = 0.2;
// Sibilance over-threshold stability threshold (dB).
// Increasing: more tolerant; decreasing: more conservative.
const AUTO_SIBILANCE_OVER_DB_STD_MAX: f32 = 6.0;
// Leveler stability threshold in dB for confidence drop-off.
// Increasing: more tolerant; decreasing: more conservative.
const AUTO_LEVELER_STD_DB_MAX: f32 = 6.0;
// Smoothstep polynomial coefficients.
// Must not change: defines the cubic smoothstep curve.
const SMOOTHSTEP_A: f32 = 3.0;
const SMOOTHSTEP_B: f32 = 2.0;
// dB scale factor for amplitude (20*log10).
// Must not change: dB conversion constant.
const DB_SCALE: f32 = 20.0;

// Start learning as soon as any non-zero signal is detected.

#[derive(Clone, Copy, Default)]
#[allow(dead_code)]
pub struct SuggestProgress {
    pub seconds: f32,
    pub target_seconds: f32,
    pub active: bool,
}

#[derive(Clone, Copy, Default)]
pub struct AutoSuggestValue {
    pub value: f32,
    pub confidence: f32,
}

/// Auto-Suggest may only influence restoration and dynamics bounds.
/// These targets are the only permissible outputs for analysis.
#[derive(Clone, Copy, Default)]
pub struct AutoSuggestTargets {
    pub noise_reduction: AutoSuggestValue,
    pub reverb_reduction: AutoSuggestValue,
    pub de_esser: AutoSuggestValue,
    pub leveler: AutoSuggestValue,
}

#[derive(Clone, Copy, Default)]
pub struct AutoSuggestDebug {
    pub env_fast: f32,
    pub env_slow: f32,
    pub adaptive_thr: f32,
    pub started: bool,
    pub heard_samples: usize,
    pub start_samples: usize,
    pub seen: usize,
    pub capture_samples: usize,
    pub noise_metric: f32,
    pub reverb_metric: f32,
    pub sibilance_metric: f32,
    pub sibilance_over_db: f32,
    pub leveler_reduction_db: f32,
}

pub struct AutoSettingsAnalyzer {
    sample_rate: f32,
    capture_samples: usize,
    seen: usize,
    started: bool,
    silence_samples: usize,
    heard_samples: usize,

    env_fast: f32,
    env_slow: f32,

    // Running stats (captured region only)
    noise_metric_sum: f32,
    noise_metric_sum_sq: f32,
    reverb_metric_sum: f32,
    reverb_metric_sum_sq: f32,
    sibilance_metric_sum: f32,
    sibilance_metric_sum_sq: f32,
    sibilance_over_db_sum: f32,
    sibilance_over_db_sum_sq: f32,
    metric_count: usize,

    // Leveler analysis (captured region only)
    leveler_analyzer: LinkedCompressor,
    leveler_reduction_db_sum: f32,
    leveler_reduction_db_sum_sq: f32,
    leveler_count: usize,

    // Placeholder filters removed: no band-energy analysis required for suggestions.
}

impl AutoSettingsAnalyzer {
    pub fn new(sample_rate: f32) -> Self {
        let mut analyzer = Self {
            sample_rate,
            capture_samples: 0,
            seen: 0,
            started: false,
            silence_samples: 0,
            heard_samples: 0,

            env_fast: 0.0,
            env_slow: 0.0,

            noise_metric_sum: 0.0,
            noise_metric_sum_sq: 0.0,
            reverb_metric_sum: 0.0,
            reverb_metric_sum_sq: 0.0,
            sibilance_metric_sum: 0.0,
            sibilance_metric_sum_sq: 0.0,
            sibilance_over_db_sum: 0.0,
            sibilance_over_db_sum_sq: 0.0,
            metric_count: 0,

            leveler_analyzer: LinkedCompressor::new(sample_rate),
            leveler_reduction_db_sum: 0.0,
            leveler_reduction_db_sum_sq: 0.0,
            leveler_count: 0,

        };
        analyzer.reset(sample_rate);
        analyzer
    }

    pub fn reset(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.capture_samples = (self.sample_rate * CAPTURE_SECONDS) as usize;

        self.seen = 0;
        self.started = false;
        self.silence_samples = 0;
        self.heard_samples = 0;

        self.env_fast = 0.0;
        self.env_slow = 0.0;

        self.noise_metric_sum = 0.0;
        self.noise_metric_sum_sq = 0.0;
        self.reverb_metric_sum = 0.0;
        self.reverb_metric_sum_sq = 0.0;
        self.sibilance_metric_sum = 0.0;
        self.sibilance_metric_sum_sq = 0.0;
        self.sibilance_over_db_sum = 0.0;
        self.sibilance_over_db_sum_sq = 0.0;
        self.metric_count = 0;

        self.leveler_analyzer = LinkedCompressor::new(self.sample_rate);
        self.leveler_reduction_db_sum = 0.0;
        self.leveler_reduction_db_sum_sq = 0.0;
        self.leveler_count = 0;

    }

    pub fn process_sample(&mut self, sample: f32) {
        if self.capture_samples == 0 || self.seen >= self.capture_samples {
            return;
        }

        let abs = sample.abs();

        // Envelope followers
        let alpha_fast = 1.0 - (-1.0 / (FAST_TAU_SEC * self.sample_rate)).exp();
        let alpha_slow = 1.0 - (-1.0 / (SLOW_TAU_SEC * self.sample_rate)).exp();

        self.env_fast += (abs - self.env_fast) * alpha_fast;
        self.env_slow += (abs - self.env_slow) * alpha_slow;

        // ------------------------------------------------------------
        // Robust start/stop gating
        // ------------------------------------------------------------
        let adaptive_thr = AUTO_START_LEVEL_THRESHOLD;
        let start_samples = AUTO_START_MIN_SAMPLES;
        if abs > AUTO_START_LEVEL_THRESHOLD {
            self.heard_samples = self.heard_samples.saturating_add(1);
        } else {
            self.heard_samples = 0;
        }
        let above_threshold = abs > adaptive_thr && self.heard_samples >= start_samples;

        if above_threshold {
            self.started = true;
            self.silence_samples = 0;
        } else if self.started {
            self.silence_samples += 1;
        }

        // ------------------------------------------------------------
        // Capture stats only after start
        // ------------------------------------------------------------
        if self.started {
            let _ = self.leveler_analyzer.compute_gain(sample, sample, 1.0);
            let reduction_db = self.leveler_analyzer.last_total_reduction_db();
            self.leveler_reduction_db_sum += reduction_db;
            self.leveler_reduction_db_sum_sq += reduction_db * reduction_db;
            self.leveler_count = self.leveler_count.saturating_add(1);
            self.seen += 1;
        }
    }

    pub fn is_done(&self) -> bool {
        if self.capture_samples == 0 {
            return false;
        }
        if self.seen >= self.capture_samples {
            return true;
        }
        false
    }

    #[allow(dead_code)]
    pub fn has_data(&self) -> bool {
        self.seen > 0
    }

    pub fn progress(&self) -> SuggestProgress {
        let seconds = self.seen as f32 / self.sample_rate.max(1.0);
        SuggestProgress {
            seconds,
            target_seconds: CAPTURE_SECONDS,
            active: self.started,
        }
    }

    pub fn process_metrics(
        &mut self,
        noise_metric: f32,
        reverb_metric: f32,
        sibilance_metric: f32,
        sibilance_over_db: f32,
    ) {
        if !self.started {
            return;
        }
        let noise = noise_metric.clamp(0.0, 1.0);
        let reverb = reverb_metric.max(0.0);
        let sibilance = sibilance_metric.clamp(0.0, 1.0);
        let over_db = sibilance_over_db.max(0.0);

        self.noise_metric_sum += noise;
        self.noise_metric_sum_sq += noise * noise;
        self.reverb_metric_sum += reverb;
        self.reverb_metric_sum_sq += reverb * reverb;
        self.sibilance_metric_sum += sibilance;
        self.sibilance_metric_sum_sq += sibilance * sibilance;
        self.sibilance_over_db_sum += over_db;
        self.sibilance_over_db_sum_sq += over_db * over_db;
        self.metric_count = self.metric_count.saturating_add(1);
    }

    pub fn finish(&mut self) -> AutoSuggestTargets {
        let metric_count = self.metric_count.max(1) as f32;
        let avg_noise_metric = (self.noise_metric_sum / metric_count).clamp(0.0, 1.0);
        let avg_reverb_metric = (self.reverb_metric_sum / metric_count).max(0.0);
        let avg_sibilance_metric = (self.sibilance_metric_sum / metric_count).clamp(0.0, 1.0);
        let avg_sibilance_over_db = (self.sibilance_over_db_sum / metric_count).max(0.0);

        let leveler_count = self.leveler_count.max(1) as f32;
        let avg_leveler_reduction_db =
            (self.leveler_reduction_db_sum / leveler_count).max(0.0);

        // ------------------------------------------------------------
        // Noise reduction suggestion (SNR-based)
        // ------------------------------------------------------------
        // noise_metric ~ noise proportion in the spectrum.
        let noise_ratio = avg_noise_metric.clamp(0.0, 1.0);
        let snr_db = DB_SCALE
            * ((1.0 - noise_ratio).max(AUTO_EPS) / (noise_ratio + AUTO_EPS)).log10();
        let noise_t = ((AUTO_SNR_DB_CLEAN - snr_db) / (AUTO_SNR_DB_CLEAN - AUTO_SNR_DB_NOISY))
            .clamp(0.0, 1.0);
        let noise_reduction = (smoothstep01(noise_t) * AUTO_NOISE_MAX).clamp(0.0, AUTO_NOISE_MAX);

        // ------------------------------------------------------------
        // Reverb reduction suggestion (late/direct ratio in dB)
        // ------------------------------------------------------------
        let reverb_ratio = avg_reverb_metric.max(AUTO_EPS);
        let reverb_db = DB_SCALE * reverb_ratio.log10();
        let reverb_t = ((reverb_db - AUTO_REVERB_DB_DRY)
            / (AUTO_REVERB_DB_WET - AUTO_REVERB_DB_DRY))
            .clamp(0.0, 1.0);
        let reverb_reduction =
            (smoothstep01(reverb_t) * AUTO_REVERB_MAX).clamp(0.0, AUTO_REVERB_MAX);

        // ------------------------------------------------------------
        // De-esser suggestion (over-threshold dB, weighted by sibilance)
        // ------------------------------------------------------------
        let target_reduction_db =
            (avg_sibilance_over_db * (AUTO_DE_ESS_RATIO - 1.0) * avg_sibilance_metric)
                .clamp(0.0, AUTO_DE_ESS_MAX_REDUCTION_DB);
        let de_esser = (target_reduction_db / AUTO_DE_ESS_MAX_REDUCTION_DB)
            .clamp(0.0, AUTO_DE_ESS_MAX);

        // ------------------------------------------------------------
        // Leveler suggestion (estimated gain reduction)
        // ------------------------------------------------------------
        let leveler = (avg_leveler_reduction_db / AUTO_LEVELER_TARGET_REDUCTION_DB)
            .clamp(0.0, AUTO_LEVELER_MAX);

        let duration_conf = if self.capture_samples > 0 {
            (self.seen as f32 / self.capture_samples as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let short_conf = if self.seen < (self.sample_rate * AUTO_SHORT_CAPTURE_SEC) as usize {
            AUTO_SHORT_CAPTURE_CONF_SCALE
        } else {
            1.0
        };
        let duration_conf = (duration_conf * short_conf).clamp(0.0, 1.0);

        let noise_std = std_from_sums(
            self.noise_metric_sum,
            self.noise_metric_sum_sq,
            metric_count,
        );
        let reverb_std = std_from_sums(
            self.reverb_metric_sum,
            self.reverb_metric_sum_sq,
            metric_count,
        );
        let sibilance_over_std = std_from_sums(
            self.sibilance_over_db_sum,
            self.sibilance_over_db_sum_sq,
            metric_count,
        );
        let leveler_std = std_from_sums(
            self.leveler_reduction_db_sum,
            self.leveler_reduction_db_sum_sq,
            leveler_count,
        );

        let noise_conf = duration_conf * stability_conf(noise_std, AUTO_METRIC_STD_MAX);
        let reverb_conf = duration_conf * stability_conf(reverb_std, AUTO_METRIC_STD_MAX);
        let de_esser_conf =
            duration_conf * stability_conf(sibilance_over_std, AUTO_SIBILANCE_OVER_DB_STD_MAX);
        let leveler_conf = duration_conf * stability_conf(leveler_std, AUTO_LEVELER_STD_DB_MAX);

        // Reset for next run
        self.reset(self.sample_rate);

        AutoSuggestTargets {
            noise_reduction: AutoSuggestValue {
                value: noise_reduction,
                confidence: noise_conf,
            },
            reverb_reduction: AutoSuggestValue {
                value: reverb_reduction,
                confidence: reverb_conf,
            },
            de_esser: AutoSuggestValue {
                value: de_esser,
                confidence: de_esser_conf,
            },
            leveler: AutoSuggestValue {
                value: leveler,
                confidence: leveler_conf,
            },
        }
    }

    pub fn debug_snapshot(&self) -> AutoSuggestDebug {
        let adaptive_thr = AUTO_START_LEVEL_THRESHOLD;
        let metric_count = self.metric_count.max(1) as f32;
        let noise_metric = (self.noise_metric_sum / metric_count).clamp(0.0, 1.0);
        let reverb_metric = (self.reverb_metric_sum / metric_count).max(0.0);
        let sibilance_metric = (self.sibilance_metric_sum / metric_count).clamp(0.0, 1.0);
        let sibilance_over_db = (self.sibilance_over_db_sum / metric_count).max(0.0);
        let leveler_count = self.leveler_count.max(1) as f32;
        let leveler_reduction_db =
            (self.leveler_reduction_db_sum / leveler_count).max(0.0);

        AutoSuggestDebug {
            env_fast: self.env_fast,
            env_slow: self.env_slow,
            adaptive_thr,
            started: self.started,
            heard_samples: self.heard_samples,
            start_samples: AUTO_START_MIN_SAMPLES,
            seen: self.seen,
            capture_samples: self.capture_samples,
            noise_metric,
            reverb_metric,
            sibilance_metric,
            sibilance_over_db,
            leveler_reduction_db,
        }
    }
}

fn smoothstep01(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (SMOOTHSTEP_A - SMOOTHSTEP_B * x)
}

fn std_from_sums(sum: f32, sum_sq: f32, count: f32) -> f32 {
    if count <= 0.0 {
        return 0.0;
    }
    let mean = sum / count;
    let var = (sum_sq / count - mean * mean).max(0.0);
    var.sqrt()
}

fn stability_conf(std: f32, max_std: f32) -> f32 {
    if max_std <= 0.0 {
        return 0.0;
    }
    (1.0 - (std / max_std)).clamp(0.0, 1.0)
}
