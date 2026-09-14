use anyhow::{Context, Result};
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::WebViewBuilder;

/// Opens the control panel in a native system webview.
pub fn run(url: &str) -> Result<()> {
    let event_loop = EventLoop::new();
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

        let container = window
            .default_vbox()
            .context("window has no GTK container")?;
        builder
            .build_gtk(container)
            .context("could not create desktop webview")?
    };

    #[cfg(not(target_os = "linux"))]
    let _webview = builder
        .build(&window)
        .context("could not create desktop webview")?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if matches!(
            event,
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
        ) {
            *control_flow = ControlFlow::Exit;
        }
    });
}
