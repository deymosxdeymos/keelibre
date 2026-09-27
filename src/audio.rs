use std::{
    collections::HashMap,
    f32::consts::FRAC_PI_4,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::{Mutex, RwLock};

use crate::{
    SAMPLE_RATE,
    config::Config,
    keymap::KeyGroup,
    sound::{Phase, Profile, Sample},
};

const MAX_VOICES: usize = 64;

#[derive(Clone)]
pub struct AudioEngine {
    mixer: Arc<Mutex<Mixer>>,
    state: Arc<RwLock<EngineState>>,
    muted: Arc<AtomicBool>,
}

struct EngineState {
    settings: Config,
    profile: Option<Arc<Profile>>,
}

impl AudioEngine {
    #[must_use]
    pub fn new(settings: Config) -> Self {
        Self {
            mixer: Arc::new(Mutex::new(Mixer::default())),
            state: Arc::new(RwLock::new(EngineState {
                settings,
                profile: None,
            })),
            muted: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_profile(&self, profile: Profile) {
        let old = self.state.write().profile.replace(Arc::new(profile));
        drop(old);
        self.mixer.lock().round_robin.clear();
    }

    pub fn apply(&self, settings: Config, profile: Option<Profile>) {
        let changed = profile.is_some();
        let mut state = self.state.write();
        state.settings = settings;
        let old = profile.and_then(|profile| state.profile.replace(Arc::new(profile)));
        drop(state);
        drop(old);
        if changed {
            self.mixer.lock().round_robin.clear();
        }
    }

    #[must_use]
    pub fn profile(&self) -> Option<Arc<Profile>> {
        self.state.read().profile.clone()
    }

    #[must_use]
    pub fn settings(&self) -> Config {
        self.state.read().settings.clone()
    }

    #[must_use]
    pub fn muted(&self) -> bool {
        self.muted.load(Ordering::Relaxed)
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
        if muted {
            self.mixer.lock().voices.clear();
        }
    }

    #[must_use]
    pub fn toggle_muted(&self) -> bool {
        let muted = !self.muted.fetch_xor(true, Ordering::Relaxed);
        if muted {
            self.mixer.lock().voices.clear();
        }
        muted
    }

    pub fn play(&self, group: KeyGroup, phase: Phase, pan: f32, feel: f32) {
        let state = self.state.read();
        let config = &state.settings;
        if !config.enabled || self.muted() || config.mute_modifiers && group == KeyGroup::Modifier {
            return;
        }
        let Some(profile) = &state.profile else {
            return;
        };
        let actual_group = if profile.variations(group, phase).is_some() {
            group
        } else {
            KeyGroup::Alpha
        };
        let Some(variations) = profile.variations(actual_group, phase) else {
            return;
        };
        let index = self
            .mixer
            .lock()
            .next_variation(actual_group, phase, variations.len());
        let feel = if config.per_key_feel {
            if feel < 1.0 {
                1.0 + (feel - 1.0) * config.home_row_softness
            } else {
                feel
            }
        } else {
            1.0
        };
        let normalization = if config.volume_normalization {
            profile.normalization_gain
        } else {
            1.0
        };
        let voice = Voice::new(
            Arc::clone(&variations[index]),
            if config.spatial_audio { pan } else { 0.0 },
            feel * normalization * config.master_volume,
            config.tone_lpf,
            config.tone_pitch,
        );
        drop(state);
        self.mixer.lock().submit(voice);
    }

    pub fn preview(&self, sample: Sample) {
        let state = self.state.read();
        let config = &state.settings;
        let voice = Voice::new(
            sample,
            0.0,
            config.master_volume,
            config.tone_lpf,
            config.tone_pitch,
        );
        drop(state);
        self.mixer.lock().submit(voice);
    }

    pub fn play_mouse(&self, phase: Phase) {
        let state = self.state.read();
        let config = &state.settings;
        if !config.enabled || self.muted() || config.mouse_sound.is_empty() {
            return;
        }
        let variation = match config.mouse_sound.as_str() {
            "default" => 0,
            "soft" => 1,
            "crisp" => 2,
            _ => return,
        };
        let Some(profile) = &state.profile else {
            return;
        };
        let Some(sample) = profile
            .variations(KeyGroup::Mouse, phase)
            .and_then(|set| set.get(variation))
        else {
            return;
        };
        let voice = Voice::new(
            Arc::clone(sample),
            0.0,
            config.master_volume * config.mouse_volume,
            config.mouse_tone_lpf,
            config.mouse_tone_pitch,
        );
        drop(state);
        self.mixer.lock().submit(voice);
    }

    pub fn preview_mouse(&self) {
        self.play_mouse(Phase::Down);
    }

    pub fn play_enter_overlay(&self) {
        let state = self.state.read();
        let config = &state.settings;
        if !config.enabled || self.muted() || config.enter_sound.is_empty() {
            return;
        }
        let Some(profile) = &state.profile else {
            return;
        };
        let Some(sample) = profile.overlay(&config.enter_sound) else {
            return;
        };
        let voice = Voice::new(
            Arc::clone(sample),
            0.0,
            config.master_volume * config.enter_volume,
            config.enter_tone_lpf,
            config.enter_tone_pitch,
        );
        drop(state);
        self.mixer.lock().submit(voice);
    }

    pub fn mix_into(&self, output: &mut [f32]) {
        self.mixer.lock().mix(output);
    }
}

pub struct AudioOutput {
    _stream: cpal::Stream,
}

impl AudioOutput {
    pub fn open(engine: &AudioEngine) -> Result<Option<Self>> {
        open_stream(Arc::clone(&engine.mixer), &engine.muted)
            .map(|stream| stream.map(|stream| Self { _stream: stream }))
    }
}

fn open_stream(mixer: Arc<Mutex<Mixer>>, muted: &Arc<AtomicBool>) -> Result<Option<cpal::Stream>> {
    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        tracing::warn!("no audio output device; running silently");
        return Ok(None);
    };
    let supported = device.supported_output_configs()?.find(|range| {
        range.channels() == 2
            && range.sample_format() == cpal::SampleFormat::F32
            && range.min_sample_rate().0 <= SAMPLE_RATE
            && range.max_sample_rate().0 >= SAMPLE_RATE
    });
    let Some(supported) = supported else {
        tracing::warn!("audio device has no 44.1 kHz stereo f32 mode; running silently");
        return Ok(None);
    };
    let config = supported
        .with_sample_rate(cpal::SampleRate(SAMPLE_RATE))
        .config();
    let muted_in_callback = Arc::clone(muted);
    let stream = device
        .build_output_stream(
            &config,
            move |output: &mut [f32], _| {
                output.fill(0.0);
                if !muted_in_callback.load(Ordering::Relaxed) {
                    mixer.lock().mix(output);
                }
            },
            |error| tracing::error!(%error, "audio stream failed"),
            None,
        )
        .context("could not create audio stream")?;
    stream.play().context("could not start audio stream")?;
    tracing::info!(device = %device.name().unwrap_or_else(|_| "unknown".into()), "audio output ready");
    Ok(Some(stream))
}

#[derive(Default)]
struct Mixer {
    voices: Vec<Voice>,
    round_robin: HashMap<(KeyGroup, Phase), usize>,
}

impl Mixer {
    fn next_variation(&mut self, group: KeyGroup, phase: Phase, count: usize) -> usize {
        let cursor = self.round_robin.entry((group, phase)).or_default();
        let selected = *cursor % count;
        *cursor = cursor.wrapping_add(1);
        selected
    }

    fn submit(&mut self, voice: Voice) {
        self.voices.retain(|voice| !voice.finished());
        if self.voices.len() >= MAX_VOICES {
            return;
        }
        self.voices.push(voice);
    }

    fn mix(&mut self, output: &mut [f32]) {
        for voice in &mut self.voices {
            voice.mix(output);
        }
        self.voices.retain(|voice| !voice.finished());
    }
}

struct Voice {
    sample: Sample,
    position: f32,
    rate: f32,
    gain_left: f32,
    gain_right: f32,
    volume: f32,
    lpf_alpha: f32,
    lpf_makeup: f32,
    dry: f32,
    lpf_state: f32,
}

impl Voice {
    fn new(sample: Sample, pan: f32, volume: f32, cutoff: f32, pitch: f32) -> Self {
        let cutoff = cutoff.clamp(0.0, 1.0);
        let theta = (pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
        Self {
            sample,
            position: 0.0,
            rate: pitch.clamp(0.1, 4.0),
            gain_left: theta.cos(),
            gain_right: theta.sin(),
            volume: volume.clamp(0.0, 4.0),
            lpf_alpha: if cutoff < 0.99 {
                (cutoff * cutoff).max(0.06)
            } else {
                1.0
            },
            lpf_makeup: 1.0 + (1.0 - cutoff).powi(2) * 3.0,
            dry: (1.0 - cutoff) * 0.45,
            lpf_state: 0.0,
        }
    }

    fn finished(&self) -> bool {
        self.position as usize + 1 >= self.sample.len()
    }

    fn mix(&mut self, output: &mut [f32]) {
        for frame in output.chunks_exact_mut(2) {
            if self.finished() {
                break;
            }
            let index = self.position as usize;
            let fraction = self.position - index as f32;
            let raw = self.sample[index] * (1.0 - fraction) + self.sample[index + 1] * fraction;
            self.lpf_state += self.lpf_alpha * (raw - self.lpf_state);
            let sample = if self.lpf_alpha < 1.0 {
                self.lpf_state * self.lpf_makeup + raw * self.dry
            } else {
                raw
            };
            frame[0] += sample * self.gain_left * self.volume;
            frame[1] += sample * self.gain_right * self.volume;
            self.position += self.rate;
        }
    }
}
