//! The renderer rules as tgw_renderer_rules.h decides them, on written cases.

use twidget_helper::rules::{self, CanvasSpellings};
use twidget_json::Json;
use twidget_renderer::BuildStats;

fn json(text: &str) -> Json {
    Json::parse(text).expect("JSON")
}

#[test]
fn ratios_slop_and_targets() {
    assert_eq!(rules::device_ratio(f64::NAN), 0.25);
    assert_eq!(rules::device_ratio(0.1), 0.25);
    assert_eq!(rules::device_ratio(2.0), 2.0);
    assert_eq!(rules::device_ratio(f64::INFINITY), 64.0);
    assert!(!rules::outside_slop(6.0, 8.0) && rules::outside_slop(6.0, 8.1), "a 10 px slop");
    assert_eq!(rules::pointer_targets(5, false, true, 7, true), rules::HoverTargets { hover: 5, active: 0 });
    assert_eq!(rules::pointer_targets(5, true, true, 7, true), rules::HoverTargets { hover: 7, active: 7 });
    assert_eq!(rules::pointer_targets(5, true, false, 7, true), rules::HoverTargets { hover: 0, active: 0 });
    assert_eq!(rules::parsed_documents(&BuildStats { builds: 9, patched: 4, ..BuildStats::default() }), 5);
}

#[test]
fn the_load_and_its_halves() {
    let (mut content, mut acked, mut loaded) = (false, false, false);
    let ack = json(r#"{"type":"query","qid":-1}"#);
    let empty = json(r#"{"type":"ops","ops":[]}"#);
    let ops = json(r#"{"type":"ops","ops":[{"op":"el"}]}"#);
    assert_eq!(
        rules::advance_load(&empty, &mut content, &mut acked, &mut loaded),
        rules::LoadChange::default(),
        "an empty batch draws nothing"
    );
    assert_eq!(rules::advance_load(&ack, &mut content, &mut acked, &mut loaded), rules::LoadChange::default());
    assert_eq!(rules::advance_load(&ops, &mut content, &mut acked, &mut loaded), rules::LoadChange { content: true, loaded: true });
    assert_eq!(rules::advance_load(&ops, &mut content, &mut acked, &mut loaded), rules::LoadChange::default(), "once");
    assert!(rules::is_load_ack(&json(r#"{"type":"query","qid":-1.5}"#)), "qid read as an int, truncated");
    assert!(!rules::is_load_ack(&json(r#"{"type":"query","qid":"-1"}"#)));
}

#[test]
fn menus_held_and_settled() {
    let mut out = vec![
        json(r#"{"type":"event","ev":{"type":"contextmenu","pointerType":"mouse"}}"#),
        json(r#"{"type":"size","h":5}"#),
        json(r#"{"type":"event","ev":{"type":"contextmenu","pointerType":"touch"}}"#),
        json(r#"{"type":"event","ev":{"type":"contextmenu","pointerType":"mouse","n":2}}"#),
    ];
    let menus = rules::take_context_menus(&mut out, b"mouse");
    assert_eq!(menus.len(), 2);
    assert_eq!(menus[1].get("ev").get("n").as_int(0), 2, "in their order");
    assert_eq!(out.len(), 2);
    assert!(rules::of_type(&out[0], b"size"), "the rest in theirs");
    let mut asked = Vec::new();
    let settled = rules::take_settled(|query| {
        asked = query.dump();
        vec![json(r#"{"type":"result","qid":0}"#), json(r#"{"type":"size","h":1}"#)]
    });
    assert_eq!(asked, br#"{"type":"query","qid":0,"query":{"q":"viewport"}}"#);
    assert_eq!(settled.len(), 1);
    assert_eq!(rules::frame_message(1.5).dump(), br#"{"type":"frame","time":1500}"#);
}

#[test]
fn canvas_spellings() {
    let mut spellings = CanvasSpellings::<u32>::default();
    let mut parses = 0;
    for i in 0..rules::MAX_CANVAS_FONT_PARSES {
        assert!(
            spellings
                .find(format!("{i}px a").as_bytes(), |_| {
                    parses += 1;
                    parses
                })
                .is_some()
        );
    }
    assert!(spellings.find(b"1px a", |_| 0).is_some(), "a kept spelling needs no parse");
    assert!(spellings.find(b"new", |_| 0).is_none(), "no parse left");
    spellings.refill();
    assert_eq!(spellings.find(b"new", |_| 99).copied(), Some(99));
    assert_eq!(spellings.len(), 1, "the 65th spelling cleared the 64 before it");
    assert!(spellings.find(&[b'x'; rules::MAX_CANVAS_FONT_SPELLING_SIZE + 1], |_| 0).is_none(), "too long");
    assert!(rules::canvas_font_limit_text().starts_with("canvas text exceeded the allowance of 16 new fonts per frame, 32 per widget, 64"));
}
