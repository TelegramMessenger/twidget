//! Gumbo, the HTML5 parser the fork parses with (`litehtml/src/gumbo`, its header `litehtml/gumbo-include/gumbo.h`):
//! Gumbo 0.10 as twidget carries it, `<search>` and the limit of 400 open elements and of cloned bytes included.
//! One module per C file; `gumbo_parse_with_options` is the entry point and its [`GumboOutput`] the tree litehtml
//! walks.
//!
//! The C tree is nodes linked by pointers; here the nodes live in [`GumboOutput::nodes`] and link by index
//! ([`NodeId`]), the document first. A `GumboStringPiece` that points into the parsed text is an offset into it
//! ([`GumboStringPiece::data`]), `None` where the C has NULL; a C string (`const char*`) is the bytes up to where
//! the C buffer ends, which can hold a NUL the C reader stops at ([`c_str`]). Fields the C leaves uninitialised
//! (an attribute's value positions when it has no value, a fragment parse's quirks mode, the parts of an
//! error's union its kind does not set) are zero here.

pub mod char_ref;
pub mod error;
pub mod parser;
pub mod tag;
#[rustfmt::skip]
pub mod tables;
#[rustfmt::skip]
mod tag_enum;
pub mod tokenizer;
pub mod utf8;

pub use error::GumboError;
pub use parser::gumbo_parse_with_options;
pub use tag::{gumbo_normalized_tagname, gumbo_tag_enum, gumbo_tag_from_original_text, gumbo_tagn_enum};
pub use tag_enum::*;

/// An index into [`GumboOutput::nodes`]: what a `GumboNode*` is in the C.
pub type NodeId = usize;

/// The C's `size_t` -1, the `index_within_parent` of a node that has no parent.
pub const NO_INDEX: usize = usize::MAX;

pub type GumboTag = u32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GumboSourcePosition {
    pub line: u32,
    pub column: u32,
    pub offset: u32,
}

pub const kGumboEmptySourcePosition: GumboSourcePosition = GumboSourcePosition { line: 0, column: 0, offset: 0 };

/// A piece of the parsed text: its offset (`None` for the C's NULL) and length.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GumboStringPiece {
    pub data: Option<usize>,
    pub length: usize,
}

pub const kGumboEmptyString: GumboStringPiece = GumboStringPiece { data: None, length: 0 };

impl GumboStringPiece {
    /// The bytes of the piece in `input`, the text it was parsed from (empty for NULL).
    pub fn bytes<'a>(&self, input: &'a [u8]) -> &'a [u8] {
        match self.data {
            Some(at) => &input[at.min(input.len())..at.saturating_add(self.length).min(input.len())],
            None => &[],
        }
    }
}

/// A C string as a C reader sees it: up to its first NUL.
pub fn c_str(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|c| *c == 0).unwrap_or(bytes.len())]
}

pub type GumboAttributeNamespaceEnum = u32;
pub const GUMBO_ATTR_NAMESPACE_NONE: GumboAttributeNamespaceEnum = 0;
pub const GUMBO_ATTR_NAMESPACE_XLINK: GumboAttributeNamespaceEnum = 1;
pub const GUMBO_ATTR_NAMESPACE_XML: GumboAttributeNamespaceEnum = 2;
pub const GUMBO_ATTR_NAMESPACE_XMLNS: GumboAttributeNamespaceEnum = 3;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GumboAttribute {
    pub attr_namespace: GumboAttributeNamespaceEnum,
    pub name: Vec<u8>,
    pub original_name: GumboStringPiece,
    pub value: Vec<u8>,
    pub original_value: GumboStringPiece,
    pub name_start: GumboSourcePosition,
    pub name_end: GumboSourcePosition,
    pub value_start: GumboSourcePosition,
    pub value_end: GumboSourcePosition,
}

/// `gumbo_get_attribute`: the first attribute named `name`, ASCII case ignored, as `strcasecmp` compares.
pub fn gumbo_get_attribute<'a>(attributes: &'a [GumboAttribute], name: &[u8]) -> Option<&'a GumboAttribute> {
    attributes.iter().find(|a| strcasecmp(&a.name, name))
}

/// The index of [`gumbo_get_attribute`]'s answer.
pub fn gumbo_get_attribute_index(attributes: &[GumboAttribute], name: &[u8]) -> Option<usize> {
    attributes.iter().position(|a| strcasecmp(&a.name, name))
}

/// `strcasecmp(a, b) == 0` of two C strings, in the C locale.
pub fn strcasecmp(a: &[u8], b: &[u8]) -> bool {
    c_str(a).eq_ignore_ascii_case(c_str(b))
}

/// `strcmp(a, b) == 0` of two C strings.
pub fn strcmp(a: &[u8], b: &[u8]) -> bool {
    c_str(a) == c_str(b)
}

pub type GumboNodeType = u32;
pub const GUMBO_NODE_DOCUMENT: GumboNodeType = 0;
pub const GUMBO_NODE_ELEMENT: GumboNodeType = 1;
pub const GUMBO_NODE_TEXT: GumboNodeType = 2;
pub const GUMBO_NODE_CDATA: GumboNodeType = 3;
pub const GUMBO_NODE_COMMENT: GumboNodeType = 4;
pub const GUMBO_NODE_WHITESPACE: GumboNodeType = 5;
pub const GUMBO_NODE_TEMPLATE: GumboNodeType = 6;

pub type GumboQuirksModeEnum = u32;
pub const GUMBO_DOCTYPE_NO_QUIRKS: GumboQuirksModeEnum = 0;
pub const GUMBO_DOCTYPE_QUIRKS: GumboQuirksModeEnum = 1;
pub const GUMBO_DOCTYPE_LIMITED_QUIRKS: GumboQuirksModeEnum = 2;

pub type GumboNamespaceEnum = u32;
pub const GUMBO_NAMESPACE_HTML: GumboNamespaceEnum = 0;
pub const GUMBO_NAMESPACE_SVG: GumboNamespaceEnum = 1;
pub const GUMBO_NAMESPACE_MATHML: GumboNamespaceEnum = 2;

pub type GumboParseFlags = u32;
pub const GUMBO_INSERTION_NORMAL: GumboParseFlags = 0;
pub const GUMBO_INSERTION_BY_PARSER: GumboParseFlags = 1 << 0;
pub const GUMBO_INSERTION_IMPLICIT_END_TAG: GumboParseFlags = 1 << 1;
pub const GUMBO_INSERTION_IMPLIED: GumboParseFlags = 1 << 3;
pub const GUMBO_INSERTION_CONVERTED_FROM_END_TAG: GumboParseFlags = 1 << 4;
pub const GUMBO_INSERTION_FROM_ISINDEX: GumboParseFlags = 1 << 5;
pub const GUMBO_INSERTION_FROM_IMAGE: GumboParseFlags = 1 << 6;
pub const GUMBO_INSERTION_RECONSTRUCTED_FORMATTING_ELEMENT: GumboParseFlags = 1 << 7;
pub const GUMBO_INSERTION_ADOPTION_AGENCY_CLONED: GumboParseFlags = 1 << 8;
pub const GUMBO_INSERTION_ADOPTION_AGENCY_MOVED: GumboParseFlags = 1 << 9;
pub const GUMBO_INSERTION_FOSTER_PARENTED: GumboParseFlags = 1 << 10;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GumboDocument {
    pub children: Vec<NodeId>,
    pub has_doctype: bool,
    /// NULL (`None`) until a doctype sets it; a parse that ends sets each that is still NULL to "".
    pub name: Option<Vec<u8>>,
    pub public_identifier: Option<Vec<u8>>,
    pub system_identifier: Option<Vec<u8>>,
    pub doc_type_quirks_mode: GumboQuirksModeEnum,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GumboText {
    pub text: Vec<u8>,
    pub original_text: GumboStringPiece,
    pub start_pos: GumboSourcePosition,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GumboElement {
    pub children: Vec<NodeId>,
    pub tag: GumboTag,
    pub tag_namespace: GumboNamespaceEnum,
    pub original_tag: GumboStringPiece,
    pub original_end_tag: GumboStringPiece,
    pub start_pos: GumboSourcePosition,
    pub end_pos: GumboSourcePosition,
    pub attributes: Vec<GumboAttribute>,
}

/// The C node's union: `document` for GUMBO_NODE_DOCUMENT, `element` for GUMBO_NODE_ELEMENT and
/// GUMBO_NODE_TEMPLATE, `text` for the others.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GumboNodeData {
    Document(GumboDocument),
    Element(GumboElement),
    Text(GumboText),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GumboNode {
    pub ty: GumboNodeType,
    pub parent: Option<NodeId>,
    pub index_within_parent: usize,
    pub parse_flags: GumboParseFlags,
    pub v: GumboNodeData,
}

impl GumboNode {
    pub fn is_element(&self) -> bool {
        self.ty == GUMBO_NODE_ELEMENT || self.ty == GUMBO_NODE_TEMPLATE
    }

    /// `v.element` of an element or template node.
    pub fn element(&self) -> &GumboElement {
        match &self.v {
            GumboNodeData::Element(e) => e,
            _ => panic!("a node of type {} read as an element", self.ty),
        }
    }

    pub fn element_mut(&mut self) -> &mut GumboElement {
        match &mut self.v {
            GumboNodeData::Element(e) => e,
            _ => panic!("a node read as an element"),
        }
    }

    /// `v.text` of a text, whitespace, CDATA or comment node.
    pub fn text(&self) -> &GumboText {
        match &self.v {
            GumboNodeData::Text(t) => t,
            _ => panic!("a node of type {} read as text", self.ty),
        }
    }

    pub fn document(&self) -> &GumboDocument {
        match &self.v {
            GumboNodeData::Document(d) => d,
            _ => panic!("a node of type {} read as the document", self.ty),
        }
    }

    /// The children of a document, element or template node (the vector both sides of the C union start with).
    pub fn children(&self) -> Option<&Vec<NodeId>> {
        match &self.v {
            GumboNodeData::Document(d) => Some(&d.children),
            GumboNodeData::Element(e) => Some(&e.children),
            GumboNodeData::Text(_) => None,
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<NodeId>> {
        match &mut self.v {
            GumboNodeData::Document(d) => Some(&mut d.children),
            GumboNodeData::Element(e) => Some(&mut e.children),
            GumboNodeData::Text(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GumboOptions {
    /// The C's allocator, deallocator and userdata have no counterpart: the port allocates as Rust does.
    pub tab_stop: i32,
    pub stop_on_first_error: bool,
    pub max_errors: i32,
    pub fragment_context: GumboTag,
    pub fragment_namespace: GumboNamespaceEnum,
}

pub const kGumboDefaultOptions: GumboOptions = GumboOptions {
    tab_stop: 8,
    stop_on_first_error: false,
    max_errors: -1,
    fragment_context: GUMBO_TAG_LAST,
    fragment_namespace: GUMBO_NAMESPACE_HTML,
};

/// What a parse reached, for the comparison's coverage: no counterpart in the C, and nothing it decides reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reached {
    /// Tokens handled, by insertion mode (the one `handle_html_content` dispatched on, or 23 for foreign
    /// content) and token type.
    pub modes: [[u32; tokenizer::GUMBO_TOKEN_EOF as usize + 1]; parser::GUMBO_INSERTION_MODE_AFTER_AFTER_FRAMESET as usize + 2],
    /// Characters the tokenizer's dispatch looked at, by state.
    pub states: [u32; tokenizer::GUMBO_LEX_CDATA as usize + 1],
}

impl Default for Reached {
    fn default() -> Reached {
        Reached { modes: Default::default(), states: [0; tokenizer::GUMBO_LEX_CDATA as usize + 1] }
    }
}

/// `GumboOutput`: the tree, the errors and whether the parse stopped at a limit.
#[derive(Clone, Debug, Default)]
pub struct GumboOutput {
    /// Every node the parse made, the document first; a node removed from the tree stays here unreachable.
    pub nodes: Vec<GumboNode>,
    pub document: NodeId,
    pub root: Option<NodeId>,
    pub errors: Vec<GumboError>,
    pub truncated: bool,
    pub reached: Reached,
}

impl GumboOutput {
    pub fn node(&self, id: NodeId) -> &GumboNode {
        &self.nodes[id]
    }
}
