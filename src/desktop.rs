use std::io::{Read, Write};
use std::net::TcpStream;

use anyhow::{Context, Result};
#[cfg(target_os = "linux")]
use ksni::blocking::TrayMethods;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use tao::event::StartCause;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::WindowBuilder,
};
use wry::WebViewBuilder;

#[derive(Clone, Copy)]
enum DesktopEvent {
    Open,
    ToggleMute,
    Quit,
}

#[cfg(target_os = "linux")]
struct KeebydTray {
    proxy: EventLoopProxy<DesktopEvent>,
}

#[cfg(target_os = "linux")]
impl ksni::Tray for KeebydTray {
    fn id(&self) -> String {
        "keebyd".into()
    }

    fn title(&self) -> String {
        "Keebyd".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![tray_icon()]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.proxy.send_event(DesktopEvent::Open);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;

        let open = self.proxy.clone();
        let mute = self.proxy.clone();
        let quit = self.proxy.clone();
        vec![
            StandardItem {
                label: "Open Keebyd".into(),
                icon_name: "window-new".into(),
                activate: Box::new(move |_| {
                    let _ = open.send_event(DesktopEvent::Open);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Toggle mute".into(),
                icon_name: "audio-volume-muted".into(),
                activate: Box::new(move |_| {
                    let _ = mute.send_event(DesktopEvent::ToggleMute);
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Quit panel".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(move |_| {
                    let _ = quit.send_event(DesktopEvent::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Opens the control panel and keeps it available from the system tray.
pub fn run(url: &str) -> Result<()> {
    let event_loop = EventLoopBuilder::<DesktopEvent>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title("Keebyd")
        .with_inner_size(LogicalSize::new(1180.0, 820.0))
        .with_min_inner_size(LogicalSize::new(760.0, 560.0))
        .build(&event_loop)
        .context("could not create desktop window")?;

    let builder = WebViewBuilder::new()
        .with_url(url)
        .with_devtools(cfg!(debug_assertions));
    #[cfg(target_os = "linux")]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder
            .build_gtk(
                window
                    .default_vbox()
                    .context("window has no GTK container")?,
            )
            .context("could not create desktop webview")?
    };
    #[cfg(not(target_os = "linux"))]
    let _webview = builder
        .build(&window)
        .context("could not create desktop webview")?;

    #[cfg(target_os = "linux")]
    let tray = KeebydTray {
        proxy: event_loop.create_proxy(),
    }
    .spawn()
    .context("could not create system tray icon")?;
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let mut tray = None;
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let proxy = event_loop.create_proxy();
    let endpoint = url.to_owned();
    event_loop.run(move |event, _, control_flow| {
        #[cfg(target_os = "linux")]
        let _keep_tray_alive = &tray;
        *control_flow = ControlFlow::Wait;
        match event {
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            Event::NewEvents(StartCause::Init) => match native_tray(proxy.clone()) {
                Ok(native_tray) => tray = Some(native_tray),
                Err(error) => {
                    tracing::error!(%error, "could not create system tray icon");
                }
            },
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                #[cfg(target_os = "linux")]
                window.set_visible(false);
                #[cfg(any(target_os = "windows", target_os = "macos"))]
                if tray.is_some() {
                    window.set_visible(false);
                } else {
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(DesktopEvent::Open) => {
                window.set_visible(true);
                window.set_focus();
            }
            Event::UserEvent(DesktopEvent::ToggleMute) => {
                if let Err(error) = post(&endpoint, "/api/toggle-mute") {
                    tracing::error!(%error, "could not toggle mute from tray");
                }
            }
            Event::UserEvent(DesktopEvent::Quit) => *control_flow = ControlFlow::Exit,
            _ => {}
        }
    });
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn native_tray(proxy: EventLoopProxy<DesktopEvent>) -> Result<tray_icon::TrayIcon> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};

    let menu = Menu::new();
    let open = MenuItem::new("Open Keebyd", true, None);
    let mute = MenuItem::new("Toggle mute", true, None);
    let quit = MenuItem::new("Quit panel", true, None);
    menu.append_items(&[&open, &mute, &PredefinedMenuItem::separator(), &quit])?;

    let open_id = open.id().clone();
    let mute_id = mute.id().clone();
    let quit_id = quit.id().clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let desktop_event = if event.id == open_id {
            Some(DesktopEvent::Open)
        } else if event.id == mute_id {
            Some(DesktopEvent::ToggleMute)
        } else if event.id == quit_id {
            Some(DesktopEvent::Quit)
        } else {
            None
        };
        if let Some(event) = desktop_event {
            let _ = proxy.send_event(event);
        }
    }));

    let rgba = tray_icon_bytes()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| [pixel[1], pixel[2], pixel[3], pixel[0]])
        .collect();
    let icon = tray_icon::Icon::from_rgba(rgba, 32, 32)?;
    tray_icon::TrayIconBuilder::new()
        .with_tooltip("Keebyd")
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .build()
        .context("could not build native tray icon")
}

#[cfg(target_os = "linux")]
fn tray_icon() -> ksni::Icon {
    const SIZE: i32 = 32;
    ksni::Icon {
        width: SIZE,
        height: SIZE,
        data: tray_icon_bytes(),
    }
}

fn tray_icon_bytes() -> Vec<u8> {
    const SIZE: i32 = 32;
    let mut data = vec![0_u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let offset = ((y * SIZE + x) * 4) as usize;
            let inside = (x - 16).pow(2) + (y - 16).pow(2) < 14_i32.pow(2);
            let letter = (10..=13).contains(&x) && (8..=24).contains(&y)
                || (x >= 13 && (27..=31).contains(&(x + y)))
                || (x >= 13 && (x - 2..=x + 2).contains(&y));
            let color = if letter {
                [255, 20, 15, 23]
            } else if inside {
                [255, 216, 156, 255]
            } else {
                [0, 0, 0, 0]
            };
            data[offset..offset + 4].copy_from_slice(&color);
        }
    }
    data
}

fn post(base_url: &str, path: &str) -> Result<()> {
    let authority = base_url
        .strip_prefix("http://")
        .context("unsupported control URL")?;
    let mut stream = TcpStream::connect(authority)?;
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: {authority}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    if !response.starts_with("HTTP/1.1 200") {
        anyhow::bail!("control service rejected request");
    }
    Ok(())
}
