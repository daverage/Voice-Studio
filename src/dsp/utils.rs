
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let denom = (edge1 - edge0).max(1e-12);
    let t = ((x - edge0) / denom).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn max3(a: f32, b: f32, c: f32) -> f32 {
    a.max(b).max(c)
}

pub fn db_to_gain(db: f32) -> f32 {
    (10.0f32).powf(db / 20.0)
}

/// Autocorrelation-based F0 estimation (lightweight, speech-focused).
/// Returns (periodicity 0..1, f0_hz).
pub fn estimate_f0_autocorr(frame: &[f32], sample_rate: f32) -> (f32, f32) {
    let n = frame.len();
    if n < 128 {
        return (0.0, 0.0);
    }

    // Remove DC + simple pre-emphasis
    let mut x: Vec<f32> = Vec::with_capacity(n);
    let mut mean = 0.0f32;
    for &v in frame {
        mean += v;
    }
    mean /= n as f32;

    let mut prev = 0.0f32;
    for &v in frame {
        let d = v - mean;
        let y = d - 0.97 * prev;
        prev = d;
        x.push(y);
    }

    // Energy gate
    let mut e0 = 0.0f32;
    for &v in &x {
        e0 += v * v;
    }
    if e0 < 1e-6 {
        return (0.0, 0.0);
    }

    // Speech-ish F0 range
    let f0_min = 70.0;
    let f0_max = 320.0;
    let lag_min = (sample_rate / f0_max).floor() as usize;
    let lag_max = (sample_rate / f0_min).ceil() as usize;

    let lag_min = lag_min.clamp(16, n / 2);
    let lag_max = lag_max.clamp(lag_min + 1, n / 2);

    let mut best_lag = 0usize;
    let mut best = 0.0f32;

    for lag in lag_min..=lag_max {
        let mut s = 0.0f32;
        let mut e1 = 0.0f32;
        let mut e2 = 0.0f32;
        for i in 0..(n - lag) {
            let a = x[i];
            let b = x[i + lag];
            s += a * b;
            e1 += a * a;
            e2 += b * b;
        }
        let denom = (e1 * e2).sqrt().max(1e-12);
        let r = (s / denom).clamp(-1.0, 1.0);
        if r > best {
            best = r;
            best_lag = lag;
        }
    }

    let periodicity = best.clamp(0.0, 1.0);
    let f0 = if best_lag > 0 {
        sample_rate / best_lag as f32
    } else {
        0.0
    };

    (periodicity, f0)
}

pub fn bell(x: f32, center: f32, width: f32) -> f32 {
    let d = (x - center) / width.max(1e-6);
    (-0.5 * d * d).exp().clamp(0.0, 1.0)
}

pub fn frame_rms(x: &[f32]) -> f32 {
    let mut s = 0.0f32;
    for &v in x {
        s += v * v;
    }
    (s / (x.len().max(1) as f32)).sqrt()
}
