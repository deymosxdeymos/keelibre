use std::{
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use clap::Parser;
use keebyd::{
    SAMPLE_RATE,
    audio::{AudioEngine, AudioOutput},
    config::{Config, default_config_path},
    input::{Hotkey, InputMonitor, MonitorEvent},
    keymap::{self, KeyGroup},
    sound::{Phase, Profile},
    web::{KeyEvent, WebState},
};

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Parser)]
#[command(version, about = "Mechanical keyboard sounds for Linux")]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    profile: Option<String>,
    #[arg(long)]
    preview: bool,
    #[arg(long)]
    list: bool,
    #[arg(long)]
    devices: bool,
    #[arg(long, value_name = "OUT.wav")]
    render: Option<PathBuf>,
    /// Run the sound engine without opening the desktop panel.
    #[arg(long)]
    headless: bool,
    #[arg(long, hide = true)]
    window: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "keebyd=info".into()),
        )
        .with_target(false)
        .compact()
        .init();
    let cli = Cli::parse();
    if let Some(url) = cli.window.as_deref() {
        return keebyd::desktop::run(url);
    }
    let config_path = cli.config.unwrap_or_else(default_config_path);
    let mut config = match Config::load(&config_path) {
        Ok(config) => config,
        Err(keebyd::config::ConfigError::Read(error))
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            tracing::info!(path = %config_path.display(), "using default configuration");
            Config::default()
        }
        Err(error) => return Err(error.into()),
    };
    if let Some(profile) = cli.profile {
        config.profile = profile;
    }

    if cli.list {
        return list_profiles(&config.sounds_dir);
    }
    if cli.devices {
        list_devices();
        return Ok(());
    }

    let profile = Profile::load(&config.sounds_dir.join(&config.profile))
        .with_context(|| format!("could not load profile '{}'", config.profile))?;
    if let Some(output) = cli.render {
        return render(&output, config, profile);
    }

    let engine = Arc::new(AudioEngine::new(config.clone()));
    let _output = AudioOutput::open(&engine).unwrap_or_else(|error| {
        tracing::warn!(%error, "audio setup failed; running silently");
        None
    });
    engine.set_profile(profile);
    if cli.preview {
        for _ in 0..3 {
            engine.play(KeyGroup::Alpha, Phase::Down, 0.0, 1.0);
            tokio::time::sleep(Duration::from_millis(85)).await;
            engine.play(KeyGroup::Alpha, Phase::Up, 0.0, 1.0);
            tokio::time::sleep(Duration::from_millis(95)).await;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        return Ok(());
    }

    run_daemon(engine, config_path, cli.headless).await
}

async fn run_daemon(engine: Arc<AudioEngine>, config_path: PathBuf, headless: bool) -> Result<()> {
    let config = engine.settings();
    let hotkey = Hotkey {
        key: config.hotkey_key,
        taps: config.hotkey_taps,
        ctrl: config.hotkey_ctrl,
    };
    let (_monitor, mut input) = InputMonitor::start(hotkey)?;
    let web_state = WebState::new(Arc::clone(&engine), config_path.clone());
    let server_state = web_state.clone();
    let server =
        tokio::spawn(async move { keebyd::web::serve(config.ui_port, server_state).await });
    let mut desktop = if headless || !graphical_session_available() {
        None
    } else {
        tokio::time::sleep(Duration::from_millis(120)).await;
        Some(DesktopProcess::spawn(config.ui_port)?)
    };
    let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let mut user = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::user_defined1())?;
    tracing::info!("running; Ctrl+K three times or SIGUSR1 toggles mute");

    loop {
        tokio::select! {
            event = input.recv() => match event {
                Some(MonitorEvent::ToggleMute) => { tracing::info!(muted = engine.toggle_muted(), "hotkey"); }
                Some(MonitorEvent::Key { code, phase }) => {
                    let position = keymap::lookup(code);
                    engine.play(position.group, phase, position.pan, position.feel);
                    web_state.publish(KeyEvent { code, phase: u8::from(matches!(phase, Phase::Up)) });
                }
                None => break,
            },
            _ = user.recv() => { tracing::info!(muted = engine.toggle_muted(), "SIGUSR1"); }
            _ = hangup.recv() => match Config::load(&config_path) {
                Ok(config) => {
                    match Profile::load(&config.sounds_dir.join(&config.profile)) {
                        Ok(profile) => engine.set_profile(profile),
                        Err(error) => tracing::error!(%error, "could not reload profile"),
                    }
                    engine.apply_settings(config);
                    tracing::info!("configuration reloaded");
                }
                Err(error) => tracing::error!(%error, "could not reload configuration"),
            },
            result = tokio::signal::ctrl_c() => { result?; break; }
        }
    }
    server.abort();
    drop(desktop.take());
    Ok(())
}

struct DesktopProcess(Child);

impl DesktopProcess {
    fn spawn(port: u16) -> Result<Self> {
        let child = Command::new(std::env::current_exe()?)
            .arg("--window")
            .arg(format!("http://127.0.0.1:{port}"))
            .spawn()
            .context("could not open desktop control panel")?;
        Ok(Self(child))
    }
}

impl Drop for DesktopProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn graphical_session_available() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
}

fn list_profiles(directory: &Path) -> Result<()> {
    println!("profiles in {}:", directory.display());
    let mut names = std::fs::read_dir(directory)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    for name in names {
        println!("  {name}");
    }
    Ok(())
}

fn list_devices() {
    println!("input devices:");
    for (path, name) in keebyd::input::devices() {
        println!("  {path:<24} {name}");
    }
}

fn render(output: &Path, config: Config, profile: Profile) -> Result<()> {
    const PATTERN: &[(KeyGroup, f32, f32)] = &[
        (KeyGroup::Alpha, -0.72, 0.40),
        (KeyGroup::Alpha, 0.60, 1.00),
        (KeyGroup::Alpha, 0.00, 1.85),
        (KeyGroup::Space, 0.00, 1.00),
        (KeyGroup::Enter, 0.95, 1.00),
        (KeyGroup::Backspace, 0.95, 1.00),
        (KeyGroup::Tab, -0.90, 1.00),
        (KeyGroup::Arrow, 0.70, 1.00),
        (KeyGroup::Modifier, -0.80, 1.00),
    ];
    let engine = AudioEngine::new(config);
    engine.set_profile(profile);
    let lead = SAMPLE_RATE as usize / 10;
    let gap = SAMPLE_RATE as usize * 3 / 10;
    let up_offset = SAMPLE_RATE as usize * 235 / 1000;
    let frames = lead + gap * PATTERN.len() + SAMPLE_RATE as usize / 2;
    let mut events = PATTERN
        .iter()
        .enumerate()
        .flat_map(|(index, &(group, pan, feel))| {
            let at = lead + index * gap;
            [
                (at, group, Phase::Down, pan, feel),
                (at + up_offset, group, Phase::Up, pan, feel),
            ]
        })
        .collect::<Vec<_>>();
    events.sort_by_key(|event| event.0);
    let mut rendered = vec![0.0_f32; frames * 2];
    let mut cursor = 0;
    for (at, group, phase, pan, feel) in events {
        engine.mix_into(&mut rendered[cursor * 2..at * 2]);
        engine.play(group, phase, pan, feel);
        cursor = at;
    }
    engine.mix_into(&mut rendered[cursor * 2..]);
    let specification = hound::WavSpec {
        channels: 2,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(output, specification)?;
    for sample in rendered {
        writer.write_sample((sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)?;
    }
    writer.finalize()?;
    tracing::info!(path = %output.display(), "benchmark rendered");
    Ok(())
}
