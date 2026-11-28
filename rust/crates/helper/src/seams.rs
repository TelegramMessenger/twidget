//! What the helper's glue is written against, as the C++ glue is against widget-sandbox's headers: a widget's
//! renderer (`twidget/renderer.h`'s `Renderer`, with its `onMessage` and `onDiag` as a [`Link`]) and its platform
//! (`twidget/remote_hosted.h`'s `SandboxHostedPlatform`). The served host's own (`twidget_host::Renderer`,
//! `twidget_host::HostedPlatform`) are concrete there; these traits let the tap put itself between them and the
//! glue, as the C++ tap subclasses and wraps them.

use std::fmt;
use std::sync::Arc;

/// Renderer → host messages (JSON).
pub type MessageSink = Arc<dyn Fn(&[u8]) + Send + Sync>;
/// Renderer diagnostics: kind, detail.
pub type DiagSink = Arc<dyn Fn(&[u8], &[u8]) + Send + Sync>;

/// `onMessage` and `onDiag`.
#[derive(Clone)]
pub struct Link {
    pub message: MessageSink,
    pub diag: DiagSink,
}

impl fmt::Debug for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Link")
    }
}

impl Link {
    pub fn new(message: impl Fn(&[u8]) + Send + Sync + 'static, diag: impl Fn(&[u8], &[u8]) + Send + Sync + 'static) -> Link {
        Link { message: Arc::new(message), diag: Arc::new(diag) }
    }
}

/// One widget's renderer in the helper (renderer.h's `Renderer`): every call on the served host's glue thread.
pub trait Renderer: Send + Sync {
    /// Sets `onMessage` and `onDiag`: what the renderer reports from then on goes there, read when reported.
    fn connect(&self, link: Link);
    /// Starts the renderer; it announces itself with `{"type":"ready"}`.
    fn load(&self);
    /// Host → renderer message (JSON).
    fn send(&self, json: &[u8]);
    /// The chat theme changed (`prefers-color-scheme` inside the renderer).
    fn set_theme(&self, theme: &[u8]);
    /// Width of the bubble in CSS px changed.
    fn set_width(&self, width: f64);
    fn destroy(&self);
}

/// The host's platform messages for the renderer (`onMessage`).
pub type PlatformObserver = MessageSink;

/// The renderer's traffic with the host's platform (`SandboxHostedPlatform`): opaque messages both ways, from any
/// thread, and the host's messages to the observer on the glue thread.
pub trait Platform: Send + Sync {
    /// A message for the host's platform; the host gets them in the order they were sent.
    fn send(&self, message: &[u8]);
    /// A request the renderer waits for: the host's answer, or None once the runtime closed or the transport
    /// ended.
    fn request(&self, message: &[u8]) -> Option<Vec<u8>>;
    /// Sets `onMessage`: the host's platform messages for the renderer, in order, on the glue thread.
    fn set_on_message(&self, observer: PlatformObserver);
}
