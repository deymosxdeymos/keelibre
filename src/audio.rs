use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use parking_lot::{Mutex, RwLock};

use crate::{
    config::Config,
    keymap::KeyGroup,
    sound::{Phase, Profile, Sample},
};

mod mixer;
mod output;
use mixer::{Mixer, Voice};
pub use output::AudioOutput;

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
        self.mixer.lock().reset_variations();
    }

    pub fn apply(&self, settings: Config, profile: Option<Profile>) {
        let changed = profile.is_some();
        let mut state = self.state.write();
        state.settings = settings;
        let old = profile.and_then(|profile| state.profile.replace(Arc::new(profile)));
        drop(state);
        drop(old);
        if changed {
            self.mixer.lock().reset_variations();
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
