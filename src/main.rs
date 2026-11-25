use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::WebViewBuilder;

fn main() -> wry::Result<()> {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://sober.vinegarhq.org/".to_owned());

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("DCL")
        .with_inner_size(tao::dpi::LogicalSize::new(1280.0, 800.0))
        .build(&event_loop)
        .expect("falha ao criar a janela");

    let _webview = WebViewBuilder::new().with_url(&url).build(&window)?;

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
