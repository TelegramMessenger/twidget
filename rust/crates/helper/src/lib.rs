//! The widget helper's tdesktop glue (rust/PLAN.md, stages 1.4 and H1): what tdesktop compiles into
//! `TelegramWidgetHelper` besides widget-sandbox's engine end and TwidgetCore's renderer, the OS glue aside.
//!
//! - [`wire`]: `tgw/tgw_hosted_wire.h`, the `0xFF` controls of a hosted renderer's platform;
//! - [`rules`]: `tgw/tgw_renderer_rules.h`, what the hosted glue and Telegram's renderer decide alike;
//! - [`forward`] and [`tap`]: `test/test_widget_renderer_forward.h` and `test/test_widget_helper_tap.{h,cpp}`, the
//!   Debug helper's tap and its work records;
//! - [`seams`]: the renderer and platform the glue is written against (widget-sandbox's `renderer.h` and
//!   `remote_hosted.h`).

#![forbid(unsafe_code)]

pub mod forward;
pub mod rules;
pub mod seams;
pub mod tap;
pub mod wire;

pub use forward::{
    ForwardingRenderer, HELPER_WORK_RECORD_BYTES, HelperWorkKind, HelperWorkRecord, MAX_HELPER_INTERVAL, RendererCall, RendererObserver,
    decode_helper_work, encode_helper_work,
};
pub use seams::{DiagSink, Link, MessageSink, Platform, PlatformObserver, Renderer};
pub use tap::{HelperRendererBuild, SteadyMicros, tap_helper_renderer, untapped_helper_renderer};
