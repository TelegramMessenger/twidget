//! The helper's serving as widget_helper_renderer.cpp wires it: widget-sandbox's hosting with this crate's glue as
//! the renderers' factory, the tap's or not. A stream the host ends before its opening ends the serving at once, as an
//! end the host chose, no lockdown run and no engine started.

use std::sync::Arc;

use twidget_helper::serve;
use twidget_stream::Stream;

struct Ended;

impl Stream for Ended {
    fn read(&self, _out: &mut [u8]) -> usize {
        0
    }
    fn write(&self, _bytes: &[u8]) -> bool {
        false
    }
    fn shutdown(&self) {}
}

struct Os;

impl twidget_js::Platform for Os {
    fn thread_cpu_seconds(&self) -> f64 {
        0.0
    }
    fn time_zone_offset(&self, _epoch_seconds: i64) -> i32 {
        0
    }
    fn random(&self, bytes: &mut [u8]) {
        bytes.fill(4);
    }
}

#[test]
fn an_ended_stream_ends_the_serving() {
    for tapped in [false, true] {
        let lockdown = || -> Vec<u8> { panic!("no lockdown before an opening") };
        let reason = serve(Arc::new(Ended), b"1", Some(&lockdown), Arc::new(Os), tapped);
        assert!(reason.is_empty(), "the host's end: {}", String::from_utf8_lossy(&reason));
    }
}
