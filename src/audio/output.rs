//! CPAL streams stay on one worker; device probing never blocks the control API.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use cpal::{
    FromSample, SampleFormat, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use tokio::sync::oneshot;

use super::AudioEngine;
use crate::SAMPLE_RATE;

pub struct AudioOutput {
    // Dropping the sender wakes the worker so it drops its stream on that thread.
    _stop: mpsc::Sender<()>,
    ready: oneshot::Receiver<()>,
}

impl AudioOutput {
    pub fn start(engine: &AudioEngine) -> std::io::Result<Self> {
        let engine = engine.clone();
        let (stop, stopped) = mpsc::channel();
        let (ready, receiver) = oneshot::channel();
        thread::Builder::new()
            .name("keebyd-audio".into())
            .spawn(move || {
                let mut ready = Some(ready);
                loop {
                    if !matches!(stopped.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                        break;
                    }
                    let failed = Arc::new(AtomicBool::new(false));
                    match open_stream(&engine, &failed) {
                        Ok(stream) => {
                            if let Some(ready) = ready.take() {
                                let _ = ready.send(());
                            }
                            loop {
                                if stopped.recv_timeout(Duration::from_millis(100))
                                    != Err(mpsc::RecvTimeoutError::Timeout)
                                {
                                    return;
                                }
                                if failed.load(Ordering::Relaxed) {
                                    break;
                                }
                            }
                            drop(stream);
                        }
                        Err(error) => tracing::warn!(%error, "audio unavailable; retrying"),
                    }
                    if stopped.recv_timeout(Duration::from_secs(1))
                        != Err(mpsc::RecvTimeoutError::Timeout)
                    {
                        break;
                    }
                }
            })?;
        Ok(Self {
            _stop: stop,
            ready: receiver,
        })
    }

    /// Wait for the first successful output connection (used by CLI previews).
    pub async fn ready(&mut self) -> Result<()> {
        (&mut self.ready)
            .await
            .context("audio worker stopped before connecting")
    }
}

fn open_stream(engine: &AudioEngine, failed: &Arc<AtomicBool>) -> Result<cpal::Stream> {
    let device = cpal::default_host()
        .default_output_device()
        .context("no audio output device")?;
    let native = device.supported_output_configs()?.find(|range| {
        range.channels() == 2
            && range.sample_format() == SampleFormat::F32
            && range.min_sample_rate().0 <= SAMPLE_RATE
            && range.max_sample_rate().0 >= SAMPLE_RATE
    });
    let supported = match native {
        Some(range) => range.with_sample_rate(cpal::SampleRate(SAMPLE_RATE)),
        None => device.default_output_config()?,
    };
    let config = supported.config();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_stream::<f32>(&device, &config, engine, failed),
        SampleFormat::F64 => build_stream::<f64>(&device, &config, engine, failed),
        SampleFormat::I16 => build_stream::<i16>(&device, &config, engine, failed),
        SampleFormat::I32 => build_stream::<i32>(&device, &config, engine, failed),
        SampleFormat::U16 => build_stream::<u16>(&device, &config, engine, failed),
        format => anyhow::bail!("unsupported audio sample format: {format}"),
    }?;
    stream.play().context("could not start audio stream")?;
    tracing::info!(
        rate = config.sample_rate.0,
        channels = config.channels,
        "audio output ready"
    );
    Ok(stream)
}

fn build_stream<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    engine: &AudioEngine,
    failed: &Arc<AtomicBool>,
) -> Result<cpal::Stream> {
    let engine = engine.clone();
    let failed = Arc::clone(failed);
    let channels = usize::from(config.channels);
    let mut adapter = OutputAdapter::new(config.sample_rate.0);
    device
        .build_output_stream(
            config,
            move |output: &mut [T], _| {
                output.fill(T::EQUILIBRIUM);
                if engine.muted() {
                    adapter.reset();
                    return;
                }
                let mut mixer = engine.mixer.lock();
                adapter.fill(output, channels, |buffer| mixer.mix(buffer));
            },
            move |error| {
                tracing::error!(%error, "audio stream failed");
                failed.store(true, Ordering::Relaxed);
            },
            None,
        )
        .context("could not create audio stream")
}

// Fixed storage, continuous interpolation, and no per-callback allocation.
struct OutputAdapter {
    buffer: [f32; 256],
    cursor: usize,
    left: [f32; 2],
    right: [f32; 2],
    position: u64,
    rate: u32,
    initialized: bool,
}

impl OutputAdapter {
    const fn new(rate: u32) -> Self {
        Self {
            buffer: [0.0; 256],
            cursor: 256,
            left: [0.0; 2],
            right: [0.0; 2],
            position: 0,
            rate,
            initialized: false,
        }
    }

    const fn reset(&mut self) {
        self.cursor = self.buffer.len();
        self.position = 0;
        self.initialized = false;
    }

    fn next(&mut self, mix: &mut impl FnMut(&mut [f32])) -> [f32; 2] {
        if self.cursor == self.buffer.len() {
            self.buffer.fill(0.0);
            mix(&mut self.buffer);
            self.cursor = 0;
        }
        let frame = [self.buffer[self.cursor], self.buffer[self.cursor + 1]];
        self.cursor += 2;
        frame
    }

    fn fill<T: SizedSample + FromSample<f32>>(
        &mut self,
        output: &mut [T],
        channels: usize,
        mut mix: impl FnMut(&mut [f32]),
    ) {
        if !self.initialized {
            self.left = self.next(&mut mix);
            self.right = self.next(&mut mix);
            self.initialized = true;
        }
        for frame in output.chunks_exact_mut(channels) {
            for _ in 0..self.position / u64::from(self.rate) {
                self.left = self.right;
                self.right = self.next(&mut mix);
            }
            self.position %= u64::from(self.rate);
            let fraction = self.position as f32 / self.rate as f32;
            let [left, right] = std::array::from_fn::<_, 2, _>(|i| {
                self.left[i] * (1.0 - fraction) + self.right[i] * fraction
            });
            frame.fill(T::EQUILIBRIUM);
            if channels == 1 {
                frame[0] = T::from_sample(f32::midpoint(left, right).clamp(-1.0, 1.0));
            } else {
                frame[0] = T::from_sample(left.clamp(-1.0, 1.0));
                frame[1] = T::from_sample(right.clamp(-1.0, 1.0));
            }
            self.position += u64::from(SAMPLE_RATE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_preserves_time_channels_and_callback_continuity() {
        for rate in [22_050, 44_100, 48_000, 96_000] {
            let mut adapter = OutputAdapter::new(rate);
            let mut index = 0;
            let mut mix = |buffer: &mut [f32]| {
                for frame in buffer.as_chunks_mut::<2>().0 {
                    frame[0] = index as f32 / 10_000.0;
                    frame[1] = -(index as f32) / 20_000.0;
                    index += 1;
                }
            };
            let mut output = [0.0_f32; 1200];
            let (first, second) = output.split_at_mut(346);
            adapter.fill(first, 2, &mut mix);
            adapter.fill(second, 2, &mut mix);
            for (index, frame) in output.as_chunks::<2>().0.iter().enumerate() {
                let expected = index as f32 * 44_100.0 / rate as f32 / 10_000.0;
                assert!((frame[0] - expected).abs() < 1e-6);
                assert!((frame[1] + expected * 0.5).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn converts_mono_integer_silence_and_resets_buffered_audio() {
        let mut adapter = OutputAdapter::new(SAMPLE_RATE);
        let mut output = [0_i16; 3];
        adapter.fill(&mut output, 1, |buffer| {
            for frame in buffer.as_chunks_mut::<2>().0 {
                *frame = [0.75, -0.25];
            }
        });
        assert_eq!(output, [8192; 3]);
        adapter.reset();
        let mut silence = [0_u16; 6];
        adapter.fill(&mut silence, 3, |_| {});
        assert_eq!(silence, [32768; 6]);
    }
}
