use std::borrow::Cow;
use std::cell::{Cell, Ref, RefCell};

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use html5ever::tree_builder::{
    create_element, Attribute, ElementFlags, NodeOrText, QuirksMode, TreeBuilder, TreeBuilderOpts, TreeSink,
};
use html5ever::{local_name, ns, LocalName, Namespace, QualName};

use crate::scan::Spans;
use crate::{
    tag_index, Flat, FlatAttr, FlatNode, Limits, Span, NODE_COMMENT, NODE_DOCUMENT, NODE_ELEMENT, NODE_TEMPLATE,
    NODE_TEXT, NODE_WHITESPACE, NONE, NS_HTML, NS_MATHML, NS_SVG, QUIRKS_LIMITED, QUIRKS_NO, QUIRKS_YES,
};

const DOCUMENT: usize = 0;
const DISCARD: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    Start,
    End,
    Comment,
    Doctype,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Raw {
    Data,
    Rcdata,
    Rawtext,
    Script,
    Plaintext,
}

#[derive(Debug)]
pub struct Mark {
    pub kind: MarkKind,
    pub name: String,
    pub attrs: Vec<String>,
    pub raw: Raw,
    pub comment: String,
}

#[derive(Debug)]
enum Kind {
    Document,
    Fragment,
    Element { attrs: Vec<Attribute>, contents: Option<usize>, integration: bool, mark: Option<u32> },
    Text { text: String, run: Option<u32>, multi: bool },
    Comment(String),
}

#[derive(Debug)]
struct Node {
    kind: Kind,
    name: QualName,
    parent: Option<usize>,
    children: Vec<usize>,
}

pub struct Sink {
    nodes: RefCell<Vec<Node>>,
    quirks: Cell<QuirksMode>,
    pending: Cell<Option<u32>>,
    pending_name: RefCell<String>,
    run: Cell<u32>,
    limits: Limits,
    truncated: Cell<bool>,
}

fn blank_name() -> QualName {
    QualName::new(None, ns!(), LocalName::from(""))
}

impl Sink {
    fn new(limits: Limits) -> Sink {
        let document = Node { kind: Kind::Document, name: blank_name(), parent: None, children: Vec::new() };
        let discard = Node {
            kind: Kind::Element { attrs: Vec::new(), contents: None, integration: false, mark: None },
            name: QualName::new(None, ns!(html), local_name!("div")),
            parent: None,
            children: Vec::new(),
        };
        Sink {
            nodes: RefCell::new(vec![document, discard]),
            quirks: Cell::new(QuirksMode::NoQuirks),
            pending: Cell::new(None),
            pending_name: RefCell::new(String::new()),
            run: Cell::new(0),
            limits,
            truncated: Cell::new(false),
        }
    }

    fn add(&self, kind: Kind, name: QualName) -> usize {
        let mut nodes = self.nodes.borrow_mut();
        if nodes.len() >= self.limits.max_nodes {
            self.truncated.set(true);
            return DISCARD;
        }
        nodes.push(Node { kind, name, parent: None, children: Vec::new() });
        nodes.len() - 1
    }

    fn detach(nodes: &mut [Node], child: usize) {
        if let Some(parent) = nodes[child].parent.take() {
            let list = &mut nodes[parent].children;
            if let Some(i) = list.iter().position(|&c| c == child) {
                list.remove(i);
            }
        }
    }

    fn note_run(&self, kind: &mut Kind) {
        if let Kind::Text { run, multi, .. } = kind {
            let now = self.run.get();
            match run {
                None => *run = Some(now),
                Some(r) if *r != now => *multi = true,
                _ => {}
            }
        }
    }

    fn insert(&self, parent: usize, at: Option<usize>, child: NodeOrText<usize>) {
        if parent == DISCARD {
            return;
        }
        match child {
            NodeOrText::AppendNode(node) => {
                if node == DISCARD || node == parent {
                    return;
                }
                let mut nodes = self.nodes.borrow_mut();
                Sink::detach(&mut nodes, node);
                let list = &mut nodes[parent].children;
                let index = at.map(|a| a.min(list.len())).unwrap_or(list.len());
                list.insert(index, node);
                nodes[node].parent = Some(parent);
            }
            NodeOrText::AppendText(text) => {
                {
                    let mut nodes = self.nodes.borrow_mut();
                    let len = nodes[parent].children.len();
                    let index = at.map(|a| a.min(len)).unwrap_or(len);
                    if index > 0 {
                        let previous = nodes[parent].children[index - 1];
                        let now = self.run.get();
                        if let Kind::Text { text: existing, run, multi } = &mut nodes[previous].kind {
                            existing.push_str(&text);
                            match run {
                                None => *run = Some(now),
                                Some(r) if *r != now => *multi = true,
                                _ => {}
                            }
                            return;
                        }
                    }
                }
                let mut kind = Kind::Text { text: text.to_string(), run: None, multi: false };
                self.note_run(&mut kind);
                let node = self.add(kind, blank_name());
                if node == DISCARD {
                    return;
                }
                let mut nodes = self.nodes.borrow_mut();
                let len = nodes[parent].children.len();
                let index = at.map(|a| a.min(len)).unwrap_or(len);
                nodes[parent].children.insert(index, node);
                nodes[node].parent = Some(parent);
            }
        }
    }
}

impl TreeSink for Sink {
    type Handle = usize;
    type Output = Sink;
    type ElemName<'a> = Ref<'a, QualName>;

    fn finish(self) -> Sink {
        self
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> usize {
        DOCUMENT
    }

    fn elem_name<'a>(&'a self, target: &'a usize) -> Ref<'a, QualName> {
        Ref::map(self.nodes.borrow(), |n| &n[*target].name)
    }

    fn create_element(&self, name: QualName, mut attrs: Vec<Attribute>, flags: ElementFlags) -> usize {
        let mut mark = None;
        if let Some(pending) = self.pending.get() {
            if self.pending_name.borrow().eq_ignore_ascii_case(&name.local) {
                mark = Some(pending);
                self.pending.set(None);
            }
        }
        if attrs.len() > self.limits.max_attrs {
            attrs.truncate(self.limits.max_attrs);
            self.truncated.set(true);
        }
        let contents = if flags.template { Some(self.add(Kind::Fragment, blank_name())) } else { None };
        self.add(
            Kind::Element { attrs, contents, integration: flags.mathml_annotation_xml_integration_point, mark },
            name,
        )
    }

    fn create_comment(&self, text: StrTendril) -> usize {
        self.add(Kind::Comment(text.to_string()), blank_name())
    }

    fn create_pi(&self, _target: StrTendril, data: StrTendril) -> usize {
        self.add(Kind::Comment(data.to_string()), blank_name())
    }

    fn append(&self, parent: &usize, child: NodeOrText<usize>) {
        self.insert(*parent, None, child);
    }

    fn append_based_on_parent_node(&self, element: &usize, prev_element: &usize, child: NodeOrText<usize>) {
        let has_parent = self.nodes.borrow()[*element].parent.is_some();
        if has_parent {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(&self, _name: StrTendril, _public_id: StrTendril, _system_id: StrTendril) {}

    fn get_template_contents(&self, target: &usize) -> usize {
        match &self.nodes.borrow()[*target].kind {
            Kind::Element { contents: Some(c), .. } => *c,
            _ => DISCARD,
        }
    }

    fn same_node(&self, x: &usize, y: &usize) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.quirks.set(mode);
    }

    fn append_before_sibling(&self, sibling: &usize, new_node: NodeOrText<usize>) {
        let (parent, index) = {
            let nodes = self.nodes.borrow();
            let Some(parent) = nodes[*sibling].parent else { return };
            let Some(index) = nodes[parent].children.iter().position(|c| c == sibling) else { return };
            (parent, index)
        };
        if let NodeOrText::AppendNode(node) = &new_node {
            let nodes = self.nodes.borrow();
            if nodes[*node].parent == Some(parent) {
                let old = nodes[parent].children.iter().position(|c| c == node);
                drop(nodes);
                let target = match old {
                    Some(o) if o < index => index - 1,
                    _ => index,
                };
                self.insert(parent, Some(target), new_node);
                return;
            }
        }
        self.insert(parent, Some(index), new_node);
    }

    fn add_attrs_if_missing(&self, target: &usize, attrs: Vec<Attribute>) {
        let mut nodes = self.nodes.borrow_mut();
        let max = self.limits.max_attrs;
        if let Kind::Element { attrs: existing, .. } = &mut nodes[*target].kind {
            for attr in attrs {
                if existing.len() >= max {
                    break;
                }
                if !existing.iter().any(|a| a.name == attr.name) {
                    existing.push(attr);
                }
            }
        }
    }

    fn remove_from_parent(&self, target: &usize) {
        let mut nodes = self.nodes.borrow_mut();
        Sink::detach(&mut nodes, *target);
    }

    fn reparent_children(&self, node: &usize, new_parent: &usize) {
        if *new_parent == DISCARD || *node == DISCARD {
            return;
        }
        let mut nodes = self.nodes.borrow_mut();
        let moved = std::mem::take(&mut nodes[*node].children);
        for &child in &moved {
            nodes[child].parent = Some(*new_parent);
        }
        nodes[*new_parent].children.extend(moved);
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &usize) -> bool {
        matches!(self.nodes.borrow()[*handle].kind, Kind::Element { integration: true, .. })
    }
}

struct Wrap {
    tb: TreeBuilder<usize, Sink>,
    marks: RefCell<Vec<Mark>>,
}

impl TokenSink for Wrap {
    type Handle = usize;

    fn process_token(&self, token: Token, line_number: u64) -> TokenSinkResult<usize> {
        let mut start = None;
        match &token {
            Token::TagToken(tag) => {
                let mut marks = self.marks.borrow_mut();
                let kind = if tag.kind == TagKind::StartTag { MarkKind::Start } else { MarkKind::End };
                marks.push(Mark {
                    kind,
                    name: tag.name.to_string().to_ascii_lowercase(),
                    attrs: tag.attrs.iter().map(|a| a.name.local.to_string().to_ascii_lowercase()).collect(),
                    raw: Raw::Data,
                    comment: String::new(),
                });
                let index = (marks.len() - 1) as u32;
                if kind == MarkKind::Start {
                    self.tb.sink.pending.set(Some(index));
                    *self.tb.sink.pending_name.borrow_mut() = tag.name.to_string();
                    start = Some(index as usize);
                }
            }
            Token::CommentToken(text) => self.marks.borrow_mut().push(Mark {
                kind: MarkKind::Comment,
                name: String::new(),
                attrs: Vec::new(),
                raw: Raw::Data,
                comment: text.to_string(),
            }),
            Token::DoctypeToken(_) => self.marks.borrow_mut().push(Mark {
                kind: MarkKind::Doctype,
                name: String::new(),
                attrs: Vec::new(),
                raw: Raw::Data,
                comment: String::new(),
            }),
            _ => {}
        }
        let markup = matches!(token, Token::TagToken(_) | Token::CommentToken(_) | Token::DoctypeToken(_));
        if !markup {
            self.tb.sink.run.set(self.marks.borrow().len() as u32);
        }
        let result = self.tb.process_token(token, line_number);
        self.tb.sink.pending.set(None);
        if markup {
            self.tb.sink.run.set(self.marks.borrow().len() as u32);
        }
        if let Some(index) = start {
            let raw = match &result {
                TokenSinkResult::Continue => Raw::Data,
                TokenSinkResult::Script(_) => Raw::Script,
                TokenSinkResult::Plaintext => Raw::Plaintext,
                TokenSinkResult::RawData(kind) => match kind {
                    html5ever::tokenizer::states::RawKind::Rcdata => Raw::Rcdata,
                    html5ever::tokenizer::states::RawKind::Rawtext => Raw::Rawtext,
                    _ => Raw::Script,
                },
            };
            self.marks.borrow_mut()[index].raw = raw;
        }
        result
    }

    fn end(&self) {
        self.tb.end();
    }

    fn adjusted_current_node_present_but_not_in_html_namespace(&self) -> bool {
        self.tb.adjusted_current_node_present_but_not_in_html_namespace()
    }
}

pub struct Tree {
    sink: Sink,
    pub marks: Vec<Mark>,
}

pub fn run(text: &str, fragment: bool, limits: Limits) -> Tree {
    let sink = Sink::new(limits);
    let opts = TreeBuilderOpts { scripting_enabled: false, ..Default::default() };
    let (tb, initial_state) = if fragment {
        let context = create_element(&sink, QualName::new(None, ns!(html), local_name!("body")), Vec::new());
        let tb = TreeBuilder::new_for_fragment(sink, context, None, opts);
        let state = tb.tokenizer_state_for_context_elem(false);
        (tb, Some(state))
    } else {
        (TreeBuilder::new(sink, opts), None)
    };
    let tokenizer =
        Tokenizer::new(Wrap { tb, marks: RefCell::new(Vec::new()) }, TokenizerOpts { initial_state, ..Default::default() });
    let queue = BufferQueue::default();
    queue.push_back(StrTendril::from_slice(text));
    while !matches!(tokenizer.feed(&queue), html5ever::TokenizerResult::Done) {}
    tokenizer.end();
    let wrap = tokenizer.sink;
    Tree { marks: wrap.marks.into_inner(), sink: wrap.tb.sink }
}

fn is_whitespace(text: &str) -> bool {
    text.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0c))
}

fn namespace_code(ns: &Namespace) -> u8 {
    if *ns == ns!(svg) {
        NS_SVG
    } else if *ns == ns!(mathml) {
        NS_MATHML
    } else {
        NS_HTML
    }
}

fn attr_namespace(name: &QualName) -> u32 {
    if name.ns == ns!(xlink) {
        1
    } else if name.ns == ns!(xml) {
        2
    } else if name.ns == ns!(xmlns) {
        3
    } else {
        0
    }
}

pub fn flatten(tree: &Tree, spans: &Spans, source: &str, limits: Limits) -> Flat {
    let nodes = tree.sink.nodes.borrow();
    let mut flat = Flat { truncated: tree.sink.truncated.get(), ..Default::default() };
    flat.quirks = match tree.sink.quirks.get() {
        QuirksMode::NoQuirks => QUIRKS_NO,
        QuirksMode::Quirks => QUIRKS_YES,
        QuirksMode::LimitedQuirks => QUIRKS_LIMITED,
    };
    let children_of = |id: usize| -> &[usize] {
        match &nodes[id].kind {
            Kind::Element { contents: Some(c), .. } => &nodes[*c].children,
            _ => &nodes[id].children,
        }
    };
    let mut stack: Vec<(usize, u32, usize)> = vec![(DOCUMENT, NONE, 0)];
    let mut order: Vec<(usize, u32)> = Vec::new();
    while let Some((id, parent, depth)) = stack.pop() {
        if depth > limits.max_depth {
            flat.truncated = true;
            continue;
        }
        let index = flat.nodes.len() as u32;
        order.push((id, parent));
        let node = &nodes[id];
        let mut out = FlatNode {
            kind: NODE_DOCUMENT,
            ns: NS_HTML,
            tag: TAGS_UNKNOWN(),
            parent,
            first_child: 0,
            child_count: 0,
            first_attr: 0,
            attr_count: 0,
            name: Span::NONE,
            text: Span::NONE,
            source: Span::NONE,
        };
        match &node.kind {
            Kind::Document | Kind::Fragment => {}
            Kind::Element { attrs, contents, mark, .. } => {
                out.kind = if contents.is_some() { NODE_TEMPLATE } else { NODE_ELEMENT };
                out.ns = namespace_code(&node.name.ns);
                out.tag = tag_index(&node.name.local);
                out.name = flat.push_str(&node.name.local);
                let mark_spans = mark.and_then(|m| spans.tags.get(m as usize)).and_then(|s| s.as_ref());
                if let Some(ms) = mark_spans {
                    out.source = ms.tag;
                }
                out.first_attr = flat.attrs.len() as u32;
                out.attr_count = attrs.len() as u32;
                for attr in attrs {
                    let ns = attr_namespace(&attr.name);
                    let raw_name = match &attr.name.prefix {
                        Some(prefix) if !prefix.is_empty() => format!("{}:{}", prefix, attr.name.local),
                        _ => attr.name.local.to_string(),
                    };
                    let source = mark_spans
                        .and_then(|ms| ms.attrs.iter().find(|(n, _)| n.eq_ignore_ascii_case(&raw_name)))
                        .map(|(_, s)| *s)
                        .unwrap_or(Span::NONE);
                    let name = flat.push_str(&attr.name.local);
                    let value = flat.push_str(&attr.value);
                    flat.attrs.push(FlatAttr { ns, name, value, source });
                }
            }
            Kind::Text { text, run, multi } => {
                out.kind = if is_whitespace(text) { NODE_WHITESPACE } else { NODE_TEXT };
                out.text = flat.push_str(text);
                if !*multi {
                    if let Some(span) = run.and_then(|r| spans.runs.get(r as usize)) {
                        if let Some(found) = crate::scan::text_span(source, *span, text) {
                            out.source = found;
                        }
                    }
                }
            }
            Kind::Comment(text) => {
                out.kind = NODE_COMMENT;
                out.text = flat.push_str(text);
            }
        }
        flat.nodes.push(out);
        let kids = children_of(id);
        for &child in kids.iter().rev() {
            stack.push((child, index, depth + 1));
        }
    }
    let mut counts = vec![0u32; flat.nodes.len()];
    for node in &flat.nodes {
        if node.parent != NONE {
            counts[node.parent as usize] += 1;
        }
    }
    let mut next = 0u32;
    for (i, node) in flat.nodes.iter_mut().enumerate() {
        node.first_child = next;
        node.child_count = counts[i];
        next += counts[i];
    }
    flat.children = vec![0; next as usize];
    let mut fill = vec![0u32; flat.nodes.len()];
    for i in 0..flat.nodes.len() {
        let parent = flat.nodes[i].parent;
        if parent != NONE {
            let p = parent as usize;
            let slot = flat.nodes[p].first_child + fill[p];
            flat.children[slot as usize] = i as u32;
            fill[p] += 1;
        }
    }
    flat.root = NONE;
    let doc = &flat.nodes[0];
    for k in 0..doc.child_count {
        let c = flat.children[(doc.first_child + k) as usize];
        if flat.nodes[c as usize].kind == NODE_ELEMENT {
            flat.root = c;
        }
    }
    let _ = order;
    let _ = NODE_TEXT;
    flat
}

#[allow(non_snake_case)]
fn TAGS_UNKNOWN() -> u16 {
    crate::TAGS.len() as u16
}
