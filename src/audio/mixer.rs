//! Deterministic DSP with no device, filesystem, or synchronization dependencies.

use std::f32::consts::FRAC_PI_4;

use crate::{
    keymap::KeyGroup,
    sound::{Phase, Sample},
};

const MAX_VOICES: usize = 64;

pub(super) struct Mixer {
    pub(super) voices: Vec<Voice>,
    round_robin: [[usize; 2]; KeyGroup::ALL.len()],
}

impl Default for Mixer {
    fn default() -> Self {
        Self {
            voices: Vec::with_capacity(MAX_VOICES),
            round_robin: [[0; 2]; KeyGroup::ALL.len()],
        }
    }
}

impl Mixer {
    pub(super) fn reset_variations(&mut self) {
        self.round_robin.fill([0; 2]);
    }

    pub(super) const fn next_variation(
        &mut self,
        group: KeyGroup,
        phase: Phase,
        count: usize,
    ) -> usize {
        let cursor = &mut self.round_robin[group as usize][phase as usize];
        let selected = *cursor % count;
        *cursor = cursor.wrapping_add(1);
        selected
    }

    pub(super) fn submit(&mut self, voice: Voice) {
        self.voices.retain(|voice| !voice.finished());
        if self.voices.len() >= MAX_VOICES {
            return;
        }
        self.voices.push(voice);
    }

    pub(super) fn mix(&mut self, output: &mut [f32]) {
        for voice in &mut self.voices {
            voice.mix(output);
        }
        self.voices.retain(|voice| !voice.finished());
    }
}

pub(super) struct Voice {
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
    pub(super) fn new(sample: Sample, pan: f32, volume: f32, cutoff: f32, pitch: f32) -> Self {
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
        for frame in output.as_chunks_mut::<2>().0 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_at_half_speed_with_asymmetric_pan() {
        let mut voice = Voice::new(vec![0.0, 1.0, -0.5, 0.0].into(), -1.0, 0.5, 1.0, 0.5);
        let mut output = [0.0; 16];
        voice.mix(&mut output);
        let expected = [0.0, 0.25, 0.5, 0.125, -0.25, -0.125, 0.0, 0.0];
        for (frame, expected) in output.as_chunks::<2>().0.iter().zip(expected) {
            assert!((frame[0] - expected).abs() < 1e-6);
            assert!(frame[1].abs() < 1e-6);
        }
        assert!(voice.finished());
    }

    #[test]
    fn filter_state_survives_callback_boundaries() {
        let mut voice = Voice::new(vec![1.0, 0.0, -1.0, 0.0].into(), -1.0, 1.0, 0.5, 1.0);
        let mut first = [0.0; 2];
        let mut rest = [0.0; 4];
        voice.mix(&mut first);
        voice.mix(&mut rest);
        // alpha=.25, makeup=1.75, dry=.225; recurrence starts at zero.
        assert!((first[0] - 0.6625).abs() < 1e-6);
        assert!((rest[0] - 0.328_125).abs() < 1e-6);
        assert!((rest[2] - -0.416_406_25).abs() < 1e-6);
    }

    #[test]
    fn voice_limit_and_reclamation_preserve_mix() {
        let mut mixer = Mixer::default();
        let storage = mixer.voices.as_ptr();
        for _ in 0..=MAX_VOICES {
            mixer.submit(Voice::new(vec![0.25, 0.0].into(), -1.0, 1.0, 1.0, 1.0));
        }
        assert_eq!(mixer.voices.len(), MAX_VOICES);
        assert_eq!(
            mixer.voices.as_ptr(),
            storage,
            "voice submission must not reallocate"
        );
        let mut output = [0.0; 4];
        mixer.mix(&mut output);
        assert!((output[0] - 16.0).abs() < 1e-6);
        assert!(output[1..].iter().all(|sample| sample.abs() < 1e-6));
        assert!(mixer.voices.is_empty());
    }

    #[test]
    fn variations_are_independent_per_group_and_phase() {
        let mut mixer = Mixer::default();
        assert_eq!(mixer.next_variation(KeyGroup::Alpha, Phase::Down, 3), 0);
        assert_eq!(mixer.next_variation(KeyGroup::Alpha, Phase::Down, 3), 1);
        assert_eq!(mixer.next_variation(KeyGroup::Alpha, Phase::Up, 3), 0);
        assert_eq!(mixer.next_variation(KeyGroup::Space, Phase::Down, 3), 0);
        assert_eq!(mixer.next_variation(KeyGroup::Alpha, Phase::Down, 3), 2);
        assert_eq!(mixer.next_variation(KeyGroup::Alpha, Phase::Down, 3), 0);
        mixer.reset_variations();
        assert_eq!(mixer.next_variation(KeyGroup::Alpha, Phase::Down, 3), 0);
        assert_eq!(mixer.next_variation(KeyGroup::Alpha, Phase::Up, 3), 0);
        assert_eq!(mixer.next_variation(KeyGroup::Space, Phase::Down, 3), 0);
    }
}
