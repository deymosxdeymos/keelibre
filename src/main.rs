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
    input::InputMonitor,
    keymap::{self, KeyGroup},
    sound::{Phase, Profile},
    web::{KeyEvent, WebState},
};

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Parser)]
#[command(version, about)]
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

fn main() -> Result<()> {
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
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("could not start async runtime")?
        .block_on(run(cli))
}

async fn run(cli: Cli) -> Result<()> {
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
    engine.set_profile(profile);
    if cli.preview {
        let mut output = AudioOutput::start(&engine)?;
        output.ready().await?;
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
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", config.ui_port)).await {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse && !headless => {
            // A second desktop launch opens a panel for the running engine.
            DesktopProcess::spawn(config.ui_port)?.0.wait()?;
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    let (_monitor, mut input) = InputMonitor::start()?;
    let web_state = WebState::new(Arc::clone(&engine), config_path);
    let server_state = web_state.clone();
    let mut server = tokio::spawn(async move { keebyd::web::serve(listener, server_state).await });
    let mut desktop = if headless || !graphical_session_available() {
        None
    } else {
        Some(DesktopProcess::spawn(config.ui_port)?)
    };
    let mut signals = Box::pin(handle_signals(web_state.clone(), Arc::clone(&engine)));
    let mut desktop_check = tokio::time::interval(Duration::from_millis(250));
    let _output = AudioOutput::start(&engine)?;
    tracing::info!("running");

    loop {
        tokio::select! {
            event = input.recv() => match event {
                Some(event) => {
                    let code = event.code;
                    let phase = event.phase;
                    if matches!(code, 272..=276) {
                        engine.play_mouse(phase);
                        continue;
                    }
                    let position = keymap::lookup(code);
                    engine.play(position.group, phase, position.pan, position.feel);
                    if position.group == KeyGroup::Enter && phase == Phase::Down {
                        engine.play_enter_overlay();
                    }
                    web_state.publish(KeyEvent { code, phase: u8::from(matches!(phase, Phase::Up)) });
                }
                None => anyhow::bail!("input monitor stopped unexpectedly"),
            },
            result = &mut signals => { result?; break; }
            _ = desktop_check.tick(), if desktop.is_some() => {
                if let Some(child) = desktop.as_mut()
                    && child.0.try_wait()?.is_some() { break; }
            }
            result = &mut server => {
                result??;
                anyhow::bail!("control panel stopped unexpectedly");
            },
            result = tokio::signal::ctrl_c() => { result?; break; }
        }
    }
    server.abort();
    drop(desktop.take());
    Ok(())
}

#[cfg(target_os = "linux")]
async fn handle_signals(state: WebState, engine: Arc<AudioEngine>) -> Result<()> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut hangup = signal(SignalKind::hangup())?;
    let mut user = signal(SignalKind::user_defined1())?;
    let mut terminate = signal(SignalKind::terminate())?;
    loop {
        tokio::select! {
            _ = user.recv() => tracing::info!(muted = engine.toggle_muted(), "SIGUSR1"),
            _ = hangup.recv() => {
                let state = state.clone();
                tokio::spawn(async move {
                    match state.reload().await {
                        Ok(()) => tracing::info!("configuration reloaded"),
                        Err(error) => tracing::error!(%error, "could not reload configuration"),
                    }
                });
            }
            _ = terminate.recv() => return Ok(()),
        }
    }
}

#[cfg(not(target_os = "linux"))]
async fn handle_signals(_state: WebState, _engine: Arc<AudioEngine>) -> Result<()> {
    std::future::pending().await
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
    !cfg!(target_os = "linux")
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var_os("DISPLAY").is_some()
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
