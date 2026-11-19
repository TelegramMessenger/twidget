use std::time::{Duration, Instant};

use lh_html5::{parse, Limits, NONE};

fn text_of(flat: &lh_html5::Flat, kind_name: &str) -> Vec<String> {
    flat.nodes
        .iter()
        .filter(|n| n.name.start != NONE && flat.str(n.name) == kind_name)
        .map(|n| flat.str(n.name).to_string())
        .collect()
}

#[test]
fn reconstructed_formatting_elements_do_not_amplify_strings() {
    let mut html = String::from("<p><b x=\"");
    html.push_str(&"A".repeat(400_000));
    html.push_str("\"></p>");
    for _ in 0..20_000 {
        html.push_str("<p>x</p>");
    }
    let flat = parse(html.as_bytes(), false, Limits::default());
    assert!(flat.truncated);
    assert!(flat.strings.len() <= (1 << 20) + html.len() * 16);
    assert!(flat.strings.len() <= 64 << 20);
}

#[test]
fn many_attributes_stay_fast_and_capped() {
    let mut html = String::from("<div ");
    for i in 0..20_000 {
        html.push_str(&format!("a{i} "));
    }
    html.push('>');
    let started = Instant::now();
    let flat = parse(html.as_bytes(), false, Limits::default());
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(flat.truncated);
    assert!(flat.nodes.iter().all(|n| n.attr_count <= 256));
    assert!(!text_of(&flat, "div").is_empty());
}

#[test]
fn depth_and_node_caps_hold() {
    let deep = "<div>".repeat(5_000);
    let flat = parse(deep.as_bytes(), false, Limits::default());
    assert!(flat.truncated);
    assert!(flat.nodes.len() < 1_000);

    let wide = "<i></i>".repeat(300_000);
    let flat = parse(wide.as_bytes(), false, Limits::default());
    assert!(flat.truncated);
    assert!(flat.nodes.len() <= 200_002);
}

#[test]
fn invalid_utf8_and_empty_input_parse() {
    let flat = parse(b"<p>\xff\xfe</p>", false, Limits::default());
    assert!(!flat.positions);
    assert!(!flat.nodes.is_empty());
    let flat = parse(b"", true, Limits::default());
    assert!(!flat.nodes.is_empty());
}

#[test]
fn positions_point_into_input() {
    let html = "<div id=\"a\" class='b'>t&amp;x<span>y</span></div>";
    let flat = parse(html.as_bytes(), false, Limits::default());
    assert!(flat.positions);
    for n in &flat.nodes {
        if n.source.start != NONE {
            assert!((n.source.start + n.source.len) as usize <= html.len());
        }
    }
    let quoted: Vec<&str> = flat
        .attrs
        .iter()
        .filter(|a| a.source.start != NONE)
        .map(|a| &html[a.source.start as usize..(a.source.start + a.source.len) as usize])
        .collect();
    assert_eq!(quoted, vec!["\"a\"", "'b'"]);
}
