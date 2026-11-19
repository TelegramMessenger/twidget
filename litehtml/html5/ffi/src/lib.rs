use lh_html5::{parse, Flat, FlatAttr, FlatNode, Limits};

#[repr(C)]
pub struct LhParse {
    pub nodes: *const FlatNode,
    pub node_count: u32,
    pub root: u32,
    pub children: *const u32,
    pub child_count: u32,
    pub attr_count: u32,
    pub attrs: *const FlatAttr,
    pub strings: *const u8,
    pub strings_len: usize,
    pub quirks: u8,
    pub truncated: u8,
    pub positions: u8,
    flat: Flat,
}

fn boxed(flat: Flat) -> *mut LhParse {
    let mut out = Box::new(LhParse {
        nodes: std::ptr::null(),
        node_count: flat.nodes.len() as u32,
        root: flat.root,
        children: std::ptr::null(),
        child_count: flat.children.len() as u32,
        attr_count: flat.attrs.len() as u32,
        attrs: std::ptr::null(),
        strings: std::ptr::null(),
        strings_len: flat.strings.len(),
        quirks: flat.quirks,
        truncated: flat.truncated as u8,
        positions: flat.positions as u8,
        flat,
    });
    out.nodes = out.flat.nodes.as_ptr();
    out.children = out.flat.children.as_ptr();
    out.attrs = out.flat.attrs.as_ptr();
    out.strings = out.flat.strings.as_ptr();
    Box::into_raw(out)
}

/// # Safety
/// `input` must point to `len` readable bytes, or be null with `len` 0.
#[no_mangle]
pub unsafe extern "C" fn lh_html5_parse(input: *const u8, len: usize, fragment: bool) -> *mut LhParse {
    let bytes: &[u8] = if input.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: the caller guarantees `len` readable bytes at `input` for the duration of this call.
        unsafe { std::slice::from_raw_parts(input, len) }
    };
    boxed(parse(bytes, fragment, Limits::default()))
}

/// # Safety
/// `result` must be null or a pointer returned by `lh_html5_parse` and not freed before.
#[no_mangle]
pub unsafe extern "C" fn lh_html5_free(result: *mut LhParse) {
    if !result.is_null() {
        // SAFETY: the pointer came from Box::into_raw in `boxed` and is freed once.
        drop(unsafe { Box::from_raw(result) });
    }
}

#[no_mangle]
pub extern "C" fn lh_html5_tag_count() -> u32 {
    lh_html5::TAGS.len() as u32
}
