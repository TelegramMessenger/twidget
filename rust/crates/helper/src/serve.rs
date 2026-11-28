//! tdesktop's `_other/widget_helper_renderer.cpp`: how the helper serves its stream, widget-sandbox's hosting served
//! with this crate's glue as every hosted widget's renderer, through the Debug tap when the helper is a Debug one. With
//! it the helper's hosting is Rust from the stream's bytes to the platform's: the OS glue (entry, channel adoption,
//! lockdown) gives [`serve`] the stream, the OS's clocks and randomness, the tap's clock in a Debug helper, and the
//! lockdown to run before `Ready`.

use std::sync::Arc;

use twidget_bundle_stamp::SandboxResources;
use twidget_document::Process;
use twidget_host::{HostedPlatform, HostedRendererFactory, RendererLink};
use twidget_js::Platform as EnginePlatform;
use twidget_stream::Stream;

use crate::glue::{HostedOptions, make_hosted_renderer, prepare_hosted_renderers};
use crate::seams::{Link, Platform, PlatformObserver, Renderer};
use crate::tap::{SteadyMicros, tap_helper_renderer, untapped_helper_renderer};
use crate::wire;

const _: () = assert!(wire::MAX_MESSAGE_BYTES == twidget_stream::REMOTE_PLATFORM_LIMIT);

/// The served host's platform as the glue and the tap see it.
impl Platform for HostedPlatform {
    fn send(&self, message: &[u8]) {
        HostedPlatform::send(self, message);
    }
    fn request(&self, message: &[u8]) -> Option<Vec<u8>> {
        HostedPlatform::request(self, message)
    }
    fn set_on_message(&self, observer: PlatformObserver) {
        HostedPlatform::set_on_message(self, move |message: &[u8]| observer(message));
    }
}

/// A helper renderer as the served host holds one: its `onMessage` and `onDiag` the host's link.
struct Served(Box<dyn Renderer>);

impl twidget_host::Renderer for Served {
    fn connect(&self, link: RendererLink) {
        let diags = link.clone();
        self.0.connect(Link::new(move |json: &[u8]| link.message(json), move |kind: &[u8], detail: &[u8]| diags.diag(kind, detail)));
    }
    fn load(&self) {
        self.0.load();
    }
    fn send(&self, json: &[u8]) {
        self.0.send(json);
    }
    fn set_theme(&self, theme: &[u8]) {
        self.0.set_theme(theme);
    }
    fn set_width(&self, width: f64) {
        self.0.set_width(width);
    }
    fn destroy(&self) {
        self.0.destroy();
    }
}

/// The hosted renderers' factory (`WidgetHelper::Serve`'s): each widget's glue on its platform, through the tap when
/// it has the tap's clock (a Debug helper: test_widget_helper_tap.cpp's `SteadyMicros`, the OS glue's), on litehtml's
/// state of the process and the engine platform's steady clock.
pub fn hosted_renderers(platform: Arc<dyn EnginePlatform>, tap: Option<SteadyMicros>) -> HostedRendererFactory {
    let options = HostedOptions { process: Arc::new(Process::default()), clock: Arc::new(move || platform.steady_seconds() * 1000.0) };
    Arc::new(move |_resources: Arc<SandboxResources>, scheme: &[u8], width: f64, hosted: Arc<HostedPlatform>| {
        let build = |given: Option<Arc<dyn Platform>>| make_hosted_renderer(scheme, width, given, &options);
        let given: Option<Arc<dyn Platform>> = Some(hosted);
        let renderer = match &tap {
            Some(micros) => tap_helper_renderer(given, scheme, Arc::clone(micros), &build),
            None => untapped_helper_renderer(given, &build),
        };
        Ok(renderer.map(|r| Box::new(Served(r)) as Box<dyn twidget_host::Renderer>))
    })
}

/// `WidgetHelper::Serve`: the stream served until it ends, on the calling thread; why it ended (empty when it ended
/// as it should). `build` is the stamp's build, `lockdown` runs once the opening was accepted and says why it could
/// not lock the process down (empty when it did), `platform` answers what only the OS does, and `tap` is the tap's
/// clock in a Debug helper, whose renderers are tapped, None in a Release one.
pub fn serve(
    stream: Arc<dyn Stream>,
    build: &[u8],
    lockdown: Option<&dyn Fn() -> Vec<u8>>,
    platform: Arc<dyn EnginePlatform>,
    tap: Option<SteadyMicros>,
) -> Vec<u8> {
    prepare_hosted_renderers();
    let renderers = hosted_renderers(Arc::clone(&platform), tap);
    twidget_host::serve_hosting(stream, build, lockdown, platform, Some(renderers))
}
