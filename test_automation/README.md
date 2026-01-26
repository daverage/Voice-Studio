# Automated VST Parameter Optimization

This directory contains tools for automatically optimizing Voice Studio plugin parameters using reference audio and objective quality metrics.

## 🎯 What It Does

The optimization system:
1. Loads your noisy audio and a professionally denoised reference
2. Tests different parameter combinations through your VST plugin
3. Uses multiple audio quality metrics to evaluate results:
   - **SI-SDR**: Scale-Invariant Signal-to-Distortion Ratio
   - **PESQ**: Perceptual Evaluation of Speech Quality (industry standard)
   - **STOI**: Short-Time Objective Intelligibility
   - **SNR**: Signal-to-Noise Ratio
   - **Spectral Convergence**: Spectral similarity
4. Uses Bayesian optimization to intelligently search the parameter space
5. Saves the best results and parameter settings

## 📦 Installation

```bash
# Create a virtual environment (recommended)
python3 -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate

# Install dependencies
pip install -r requirements.txt
```

## 🚀 Quick Start

### Step 1: Inspect Your VST Parameters

First, find out what parameters your VST exposes:

```bash
python inspect_vst.py /path/to/vxcleaner.vst3
```

Example output:
```
Available Parameters:
  Noise Reduction              = 0.0
  De-Verb                      = 0.0
  Proximity                    = 0.0
  Clarity                      = 0.0
  ...
```

### Step 2: Update Parameter Mapping

Edit `auto_tune.py` and update the `param_mapping` dictionary around line 109 with the actual parameter names from your VST:

```python
param_mapping = {
    'noise_reduction': 'Noise Reduction',  # Use exact names from inspect_vst.py
    'reverb_reduction': 'De-Verb',
    'proximity': 'Proximity',
    'clarity': 'Clarity',
    # ... etc
}
```

### Step 3: Run Optimization

```bash
python auto_tune.py \
    --noisy test_data/noisy_voice.wav \
    --reference test_data/clean_voice.wav \
    --vst /Library/Audio/Plug-Ins/VST3/vxcleaner.vst3 \
    --trials 100 \
    --output results
```

## 📊 Understanding Results

The optimization will:
- Print progress for each trial
- Save the best audio file whenever a better result is found
- Create `best_parameters.json` with optimal settings
- Generate `optimization_study.csv` with all trial data

### Example Output:
```
🎯 New best! Trial 42, Score: 0.7234
   Metrics: SI-SDR=15.23, PESQ=3.45, STOI=0.892, SNR=18.4dB
   Params: {'noise_reduction': 0.65, 'clarity': 0.42, ...}
   Saved to: results/best_trial_42_score_0.7234.wav
```

## 🎨 Customization

### Adjust Metric Weights

Edit the `weights` dictionary in `auto_tune.py` (around line 155) to prioritize different aspects:

```python
weights = {
    'si_sdr': 0.3,      # Overall quality
    'pesq': 0.25,       # Perceptual quality (what humans hear)
    'stoi': 0.25,       # Intelligibility (understanding speech)
    'snr': 0.1,         # Noise reduction
    'spectral_conv': 0.1,  # Spectral accuracy
}
```

For example:
- **Podcasts**: Increase `stoi` (intelligibility is key)
- **Music vocals**: Increase `pesq` (perceptual quality matters more)
- **Maximum noise reduction**: Increase `snr`

### Adjust Parameter Search Ranges

Edit the `objective` function (around line 182) to constrain parameters:

```python
params = {
    'noise_reduction': trial.suggest_float('noise_reduction', 0.3, 0.9),  # Only try 30-90%
    'proximity': trial.suggest_float('proximity', 0.0, 0.3),  # Limit proximity boost
    # ...
}
```

## 📁 File Structure

```
test_automation/
├── auto_tune.py          # Main optimization script
├── inspect_vst.py        # VST parameter inspector
├── requirements.txt      # Python dependencies
├── README.md            # This file
└── results/             # Output directory (created automatically)
    ├── best_trial_*.wav           # Best audio samples
    ├── best_parameters.json       # Optimal parameter settings
    └── optimization_study.csv     # All trial data for analysis
```

## 🔧 Troubleshooting

### "VST not found" or "Cannot load plugin"

- **Mac**: Ensure VST is in `/Library/Audio/Plug-Ins/VST3/`
- **Windows**: Ensure VST is in `C:\Program Files\Common Files\VST3\`
- **Linux**: Ensure VST is in `~/.vst3/`
- Check that the plugin file exists and has proper permissions

### "No parameters found"

Some VSTs don't expose parameters through pedalboard's standard interface. You may need to:
1. Use a different VST host library (try `dawdreamer`)
2. Manually control parameters via MIDI CC or automation

### Poor optimization results

- Ensure your reference audio is truly high quality
- Try increasing `--trials` (more trials = better exploration)
- Check that audio files are properly aligned (same length, no offset)
- Verify sample rates match or are properly resampled

### PESQ errors

PESQ requires specific sample rates (8kHz or 16kHz). The script auto-resamples, but if you get errors:
- Ensure your audio is at least 8kHz
- Try pre-resampling files to 16kHz before optimization

## 💡 Tips for Best Results

1. **Use representative audio**: Optimize on audio similar to what you'll actually process
2. **Multiple test files**: Run optimization on 3-5 different samples and average the parameters
3. **Start simple**: Begin with fewer parameters (just noise reduction + clarity) before optimizing all
4. **Validate manually**: Listen to the results - metrics aren't perfect!
5. **Create presets**: Save parameter sets for different use cases (podcast, music, dialogue, etc.)

## 📈 Advanced: Analyzing Results

The `optimization_study.csv` file can be loaded into pandas for analysis:

```python
import pandas as pd
import matplotlib.pyplot as plt

df = pd.read_csv('results/optimization_study.csv')

# Plot parameter importance
plt.figure(figsize=(10, 6))
df.plot(x='number', y='value', kind='scatter', alpha=0.5)
plt.xlabel('Trial Number')
plt.ylabel('Score')
plt.title('Optimization Progress')
plt.savefig('optimization_progress.png')

# Find parameters that correlate with high scores
high_score_trials = df[df['value'] > df['value'].quantile(0.9)]
print("Best parameter ranges:")
for param in ['params_noise_reduction', 'params_clarity']:
    if param in df.columns:
        print(f"{param}: {high_score_trials[param].mean():.3f} ± {high_score_trials[param].std():.3f}")
```

## 🤝 Contributing

Found a better metric? Improved the optimization algorithm? Create a pull request!

## 📝 License

Same as Voice Studio plugin
