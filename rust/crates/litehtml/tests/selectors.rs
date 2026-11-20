//! The Rust selectors against the fork's (`oracle/lhdump`): for a document, lhdump dumps the fork's own DOM
//! (Gumbo's tree as litehtml keeps it: tags, ids, classes, attributes, text, comments, which elements count
//! as text, the pseudo-class flags), then every selector of each sheet as the fork parses it (its compounds,
//! subclass and pseudo selectors, nested lists, combinators, specificity, order) and the match matrix: each
//! selector against each node, with pseudo-classes checked and not, and with `:hover` and `:active` set on
//! element after element. The Rust side reads the DOM into a tree implementing `SelectElements`, parses
//! the same sheets and matches them. The documents: widget-sandbox's corpus with its own sheets (and litehtml's
//! master CSS and the renderer's UA layers on some), and generated documents and selector sheets. Skipped
//! without `TWIDGET_ORACLE_DIR`.

mod common;

use common::*;
use twidget_litehtml::css_parser::{f_componentize, normalize};
use twidget_litehtml::css_selector::{DocumentMode, parse_selector_list, strict_mode};
use twidget_litehtml::html_tag::{Matcher, select_match, select_match_pseudo_class, select_match_with_after, select_no_match};
use twidget_litehtml::{Names, string_id};

#[test]
fn corpus_documents_alike() {
    let master = master_css();
    let extra = ua_extra();
    let ua = ua_stylesheet();
    let mut cases = Vec::new();
    for (k, (path, html)) in corpus_documents().into_iter().enumerate() {
        let mut doc_sheets = style_blocks(&html);
        if k % 8 == 0 {
            doc_sheets.extend([master.clone(), extra.clone(), ua.clone()]);
        }
        cases.push((match_input(true, &html, &doc_sheets), format!("corpus document {path}")));
    }
    assert!(cases.len() > 300, "found {} corpus documents", cases.len());
    check_matches("corpus documents", cases);
}

#[test]
fn generated_documents_alike() {
    let mut random = Lcg::new(0x11_4001);
    let mut cases = Vec::new();
    for i in 0..3000 {
        let html = any_document(&mut random);
        let doc_sheets: Vec<Vec<u8>> = (0..1 + random.below(2))
            .map(|_| {
                let mut sheet = any_selector_sheet(&mut random).into_bytes();
                if random.chance(6) {
                    mutate(&mut random, &mut sheet);
                }
                sheet
            })
            .collect();
        cases.push((match_input(true, html.as_bytes(), &doc_sheets), format!("generated document {i}: {html} with {doc_sheets:?}")));
    }
    check_matches("generated documents", cases);
}

#[test]
fn master_against_generated_documents_alike() {
    let master = [master_css(), ua_extra(), ua_stylesheet()];
    let mut random = Lcg::new(0x11_4002);
    let mut cases = Vec::new();
    for i in 0..60 {
        let html = any_document(&mut random);
        cases.push((match_input(true, html.as_bytes(), &master), format!("master against generated document {i}: {html}")));
    }
    check_matches("master against generated documents", cases);
}

/// A small tree for the checks below: html > body > (ul > li × 4 with text between them, p#a.x lang=en-US).
fn tree(names: &Names) -> TestTree {
    let mut tree = TestTree::default();
    let add = |tree: &mut TestTree, parent: Option<usize>, tag: &[u8], attrs: &[(&[u8], &[u8])]| {
        let index = tree.nodes.len();
        let mut node = TestNode {
            parent,
            children: Vec::new(),
            is_tag: !tag.is_empty(),
            is_comment: false,
            inline_text: tag.is_empty(),
            tag: names.id(tag),
            id: string_id::empty_id,
            classes: Vec::new(),
            attrs: Default::default(),
            pseudo: Vec::new(),
        };
        for (name, value) in attrs {
            node.attrs.insert(name.to_vec(), value.to_vec());
            if *name == b"id" {
                node.id = names.id(value);
            }
            if *name == b"class" {
                node.classes.push(names.id(value));
            }
        }
        if let Some(p) = parent {
            tree.nodes[p].children.push(index);
        }
        tree.nodes.push(node);
        index
    };
    let html = add(&mut tree, None, b"html", &[]);
    let body = add(&mut tree, Some(html), b"body", &[]);
    let ul = add(&mut tree, Some(body), b"ul", &[]);
    for k in 0..4 {
        add(&mut tree, Some(ul), b"", &[]);
        let class: &[u8] = if k % 2 == 0 { b"even" } else { b"odd" };
        add(&mut tree, Some(ul), b"li", &[(b"class", class)]);
    }
    add(&mut tree, Some(body), b"p", &[(b"id", b"a"), (b"class", b"x"), (b"lang", b"en-US"), (b"title", b"one two")]);
    tree
}

fn selector(names: &Names, text: &str, mode: DocumentMode) -> std::sync::Arc<twidget_litehtml::css_selector::CssSelector> {
    let list = parse_selector_list(names, &normalize(text.as_bytes(), f_componentize, None), strict_mode, mode);
    assert_eq!(list.len(), 1, "{text} parses to one selector");
    list[0].clone()
}

#[test]
fn specificity_counts_the_fork_way() {
    let names = Names::new();
    let spec = |text: &str| {
        let mut sel = (*selector(&names, text, DocumentMode::no_quirks_mode)).clone();
        sel.calc_specificity();
        (sel.m_specificity.b, sel.m_specificity.c, sel.m_specificity.d)
    };
    assert_eq!(spec("ul > li.odd:hover"), (0, 2, 2));
    assert_eq!(spec("#a .x [title]"), (1, 2, 0));
    // :where() adds nothing; :is() and :not() count one whatever they hold.
    assert_eq!(spec(":where(#a) p"), (0, 0, 1));
    assert_eq!(spec(":is(#a, #b)"), (0, 1, 0));
    assert_eq!(spec("li::before"), (0, 1, 1));
    // ::placeholder becomes the renderer's element, a child of the input.
    let placeholder = selector(&names, "input::placeholder", DocumentMode::no_quirks_mode);
    assert_eq!(&*names.s(placeholder.m_right.m_tag), b"lh-placeholder");
}

#[test]
fn structure_and_attributes_match_as_the_fork_matches() {
    let names = Names::new();
    let tree = tree(&names);
    let matcher = Matcher::new(&tree, &names);
    let matching = |text: &str| -> Vec<usize> {
        let sel = selector(&names, text, DocumentMode::no_quirks_mode);
        (0..tree.nodes.len()).filter(|&el| matcher.select(el, &sel, true) != select_no_match).collect()
    };
    let li = [4usize, 6, 8, 10];
    assert_eq!(matching("li:nth-child(odd)"), [li[0], li[2]]);
    assert_eq!(matching("li:nth-child(2n of .odd)"), [li[3]]);
    assert_eq!(matching("li:nth-last-child(-n+2)"), [li[2], li[3]]);
    assert_eq!(matching("li.odd + li"), [li[2]]);
    assert_eq!(matching("li.even ~ li.odd"), [li[1], li[3]]);
    assert_eq!(matching("li:first-of-type"), [li[0]]);
    assert_eq!(matching("p:only-of-type"), [11]);
    assert_eq!(matching("body > :not(ul)"), [11]);
    assert_eq!(matching("[title~=two]"), [11]);
    assert_eq!(matching("[lang|=en]"), [11]);
    assert_eq!(matching(":lang(EN)"), [11]);
    // the text between the list items is no child for :empty, which they are not either
    assert_eq!(matching("ul:empty"), Vec::<usize>::new());
    // pseudo-classes left unchecked are marked in the answer; ::after on an element marks it
    let hover = selector(&names, "li:hover", DocumentMode::no_quirks_mode);
    assert_eq!(matcher.select(li[0], &hover, false), select_match | select_match_pseudo_class);
    assert_eq!(matcher.select(li[0], &hover, true), select_no_match);
    let after = selector(&names, "li::after", DocumentMode::no_quirks_mode);
    assert_eq!(matcher.select(li[0], &after, true), select_match | select_match_with_after);
}

#[test]
fn quirks_mode_lowercases_ids_and_classes_in_selectors() {
    let names = Names::new();
    let quirks = selector(&names, "#Main.Item", DocumentMode::quirks_mode);
    assert_eq!(&*names.s(quirks.m_right.m_attrs[0].name), b"main");
    assert_eq!(&*names.s(quirks.m_right.m_attrs[1].name), b"item");
    let strict = selector(&names, "#Main.Item", DocumentMode::no_quirks_mode);
    assert_eq!(&*names.s(strict.m_right.m_attrs[0].name), b"Main");
}
