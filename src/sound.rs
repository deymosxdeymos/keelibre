use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use symphonia::core::{
    audio::AudioBufferRef,
    codecs::DecoderOptions,
    formats::FormatOptions,
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
    probe::Hint,
};

use crate::{SAMPLE_RATE, keymap::KeyGroup};

pub type Sample = Arc<[f32]>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Phase {
    Down,
    Up,
}

#[derive(Clone, Debug)]
pub struct Profile {
    pub name: String,
    pub directory: PathBuf,
    pub normalization_gain: f32,
    samples: HashMap<(KeyGroup, Phase), Vec<Sample>>,
}

impl Profile {
    pub fn load(directory: &Path) -> Result<Self> {
        let name = directory
            .file_name()
            .and_then(|name| name.to_str())
            .context("profile path has no UTF-8 directory name")?
            .to_owned();
        let normalization_gain = read_normalization_gain(directory).unwrap_or(1.0);
        let mut files = fs::read_dir(directory)
            .with_context(|| format!("could not read profile {}", directory.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
            })
            .collect::<Vec<_>>();
        if let Some(root) = directory.parent() {
            files.extend(
                fs::read_dir(root.join("_shared"))
                    .into_iter()
                    .flatten()
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| {
                        path.extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
                            && path
                                .file_stem()
                                .and_then(|stem| stem.to_str())
                                .is_some_and(|stem| stem.starts_with("mouse_"))
                    }),
            );
        }
        files.sort_unstable();

        let mut samples: HashMap<_, Vec<_>> = HashMap::new();
        for path in files {
            let Some((group, phase)) = parse_sample_name(&path) else {
                continue;
            };
            samples
                .entry((group, phase))
                .or_default()
                .push(decode(&path)?);
        }
        if samples.is_empty() {
            bail!("profile {name} contains no recognized samples");
        }
        Ok(Self {
            name,
            directory: directory.to_owned(),
            normalization_gain,
            samples,
        })
    }

    pub fn variations(&self, group: KeyGroup, phase: Phase) -> Option<&[Sample]> {
        self.samples.get(&(group, phase)).map(Vec::as_slice)
    }
}

pub fn decode(path: &Path) -> Result<Sample> {
    let file =
        fs::File::open(path).with_context(|| format!("could not open {}", path.display()))?;
    let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .with_context(|| format!("unsupported audio file {}", path.display()))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .context("audio file has no default track")?;
    let track_id = track.id;
    let source_rate = track.codec_params.sample_rate.unwrap_or(SAMPLE_RATE);
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let mut mono = Vec::new();
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }
        let buffer = decoder.decode(&packet)?;
        append_mono(buffer, &mut mono);
    }
    if mono.is_empty() {
        bail!("{} decoded to zero samples", path.display());
    }
    Ok(if source_rate == SAMPLE_RATE {
        mono.into()
    } else {
        resample(&mono, source_rate).into()
    })
}

fn append_mono(buffer: AudioBufferRef<'_>, output: &mut Vec<f32>) {
    let spec = *buffer.spec();
    let mut converted =
        symphonia::core::audio::SampleBuffer::<f32>::new(buffer.capacity() as u64, spec);
    converted.copy_interleaved_ref(buffer);
    let channels = spec.channels.count();
    for frame in converted.samples().chunks_exact(channels) {
        output.push(frame.iter().sum::<f32>() / channels as f32);
    }
}

fn resample(input: &[f32], source_rate: u32) -> Vec<f32> {
    let step = f64::from(source_rate) / f64::from(SAMPLE_RATE);
    let len = (input.len() as f64 / step) as usize;
    (0..len)
        .map(|index| {
            let source = index as f64 * step;
            let left = source as usize;
            let fraction = (source - left as f64) as f32;
            let right = (left + 1).min(input.len() - 1);
            input[left] * (1.0 - fraction) + input[right] * fraction
        })
        .collect()
}

fn parse_sample_name(path: &Path) -> Option<(KeyGroup, Phase)> {
    let stem = path.file_stem()?.to_str()?;
    let (prefix, variation) = stem.rsplit_once('_')?;
    variation.parse::<u8>().ok()?;
    let (group, phase) = prefix.rsplit_once('_')?;
    let group = KeyGroup::ALL
        .into_iter()
        .find(|candidate| candidate.as_str() == group)?;
    let phase = match phase {
        "down" => Phase::Down,
        "up" => Phase::Up,
        _ => return None,
    };
    Some((group, phase))
}

fn read_normalization_gain(directory: &Path) -> Option<f32> {
    fs::read_to_string(directory.join("profile.conf"))
        .ok()?
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "normalization_gain")
                .then(|| value.trim().parse().ok())
                .flatten()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_pack_filename() {
        assert_eq!(
            parse_sample_name(Path::new("alpha_down_03.wav")),
            Some((KeyGroup::Alpha, Phase::Down))
        );
        assert_eq!(
            parse_sample_name(Path::new("mouse_up_02.wav")),
            Some((KeyGroup::Mouse, Phase::Up))
        );
        assert_eq!(parse_sample_name(Path::new("README.md")), None);
    }
}
