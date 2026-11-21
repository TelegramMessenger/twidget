//! TwidgetCore's paint values and their binary forms (`core/src/wire.h`, `wire.cpp`), for a renderer and
//! a painter in two processes: a display list, damage, a batch of canvas ops and font metrics. The forms
//! are lossless (every member, each float with its bits, payload sharing), versioned, and their
//! decoders take hostile bytes. `docs/TWIDGET_CORE.md`, "Binary forms", lays out every byte.

#![forbid(unsafe_code)]

mod codec;
mod values;

pub use codec::{
    Caps, Damage, Form, VERSION, decode_batch, decode_damage, decode_list, decode_metrics, encode_batch, encode_damage, encode_list,
    encode_list_with_starts, encode_metrics, version_1_command, version_1_op, version_1_paint, version_1_path,
};
pub use values::*;
