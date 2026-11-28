//! widget-sandbox's packages/core/tests/fixtures/paint-policy.json against the port, case by case as
//! tests/paint_policy_tests.cpp runs it: the rule every renderer that counts heavy boxes implements.

use std::path::{Path, PathBuf};

use twidget_json::Json;
use twidget_paint::*;

fn widget_sandbox() -> PathBuf {
    std::env::var_os("TWIDGET_WIDGET_SANDBOX")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../widget-sandbox"))
}

fn text(v: &Json) -> Vec<u8> {
    match v {
        Json::String(s) => s.clone(),
        Json::Number(n) => twidget_json::number(*n).into_bytes(),
        _ => Vec::new(),
    }
}

struct Owned {
    fields: [Vec<u8>; 6],
    button: Button,
}

impl Owned {
    fn new(s: &Json) -> Owned {
        let button = match s["button"].as_str() {
            b"disabled" => Button::Disabled,
            b"pressed" => Button::Pressed,
            _ => Button::None,
        };
        Owned {
            fields: [
                text(&s["display"]),
                text(&s["content"]),
                text(&s["boxShadow"]),
                text(&s["filter"]),
                text(&s["backdropFilter"]),
                text(&s["opacity"]),
            ],
            button,
        }
    }

    fn view(&self) -> Style<'_> {
        let [display, content, box_shadow, filter, backdrop_filter, opacity] = &self.fields;
        Style { display, content, box_shadow, filter, backdrop_filter, opacity, button: self.button }
    }
}

fn expected(v: &Json) -> Option<BoxPaint> {
    if v.is_null() {
        return None;
    }
    Some(BoxPaint { heavy: v["heavy"].as_bool(false), backdrop: v["backdrop"].as_bool(false) })
}

fn animation(keyframes: &Json) -> Option<BoxPaint> {
    let mut paint = AnimationPaint::default();
    for k in keyframes.arr() {
        let computed = &k["computedOffset"];
        let offset = &k["offset"];
        paint.keyframe(if computed.is_number() {
            Some(computed.as_number(0.0))
        } else if offset.is_number() {
            Some(offset.as_number(0.0))
        } else {
            None
        });
        for (name, value) in k.obj() {
            paint.property(name, if value.is_string() { Some(value.as_str()) } else { None });
        }
    }
    paint.result()
}

fn keys(v: &Json) -> Vec<Vec<u8>> {
    if v.is_string() {
        return vec![v.as_str().to_vec()];
    }
    v.arr().iter().map(|k| k.as_str().to_vec()).collect()
}

#[test]
fn the_shared_fixture() {
    let path = widget_sandbox().join("packages/core/tests/fixtures/paint-policy.json");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let f = Json::parse(&bytes).expect("the fixture parses");
    let mut checks = 0;
    let mut check = |ok: bool, what: String| {
        checks += 1;
        assert!(ok, "{what}");
    };

    check(f["constants"]["PAINT_HEAVY_BOXES"].as_int(0) == MAX_HEAVY_BOXES, "constants: heavy boxes".into());
    check(f["constants"]["PAINT_BACKDROP_BOXES"].as_int(0) == MAX_BACKDROP_BOXES, "constants: backdrop boxes".into());

    for s in f["shadows"].arr() {
        let value = s["value"].as_str();
        let shown = String::from_utf8_lossy(value);
        check(box_shadow_blurred(value) == s["blurred"].as_bool(false), format!("shadows: blurred '{shown}'"));
        let without = box_shadow_without_blur(value);
        check(
            without == s["withoutBlur"].as_str(),
            format!("shadows: without blur '{shown}' (got '{}')", String::from_utf8_lossy(&without)),
        );
    }

    for b in f["boxes"].arr() {
        let style = Owned::new(&b["style"]);
        let got = box_paint(&style.view(), b["pseudo"].as_bool(false));
        check(got == expected(&b["expect"]), format!("boxes: {} (got {got:?})", String::from_utf8_lossy(b["name"].as_str())));
    }

    for h in f["hidesSubtree"].arr() {
        check(hides_subtree(h["display"].as_str()) == h["expect"].as_bool(false), format!("hidesSubtree: {:?}", h["display"]));
    }

    for a in f["animations"].arr() {
        let got = animation(&a["keyframes"]);
        check(got == expected(&a["expect"]), format!("animations: {} (got {got:?})", String::from_utf8_lossy(a["name"].as_str())));
    }

    for m in f["messages"].arr() {
        let code = PaintLimit::from_code(m["code"].as_str());
        check(
            code.is_some_and(|c| c.message(m["count"].as_int(0)).as_bytes() == m["expect"].as_str()),
            format!("messages: {:?}", m["code"]),
        );
    }

    for c in f["cases"].arr() {
        let mut counter = PaintCounter::<Vec<u8>>::new();
        let name = String::from_utf8_lossy(c["name"].as_str()).into_owned();
        for (step, s) in c["steps"].arr().iter().enumerate() {
            let step = step + 1;
            if s.find("set").is_some() {
                let style = Owned::new(&s["style"]);
                let paint = box_paint(&style.view(), s["pseudo"].as_bool(false));
                for key in keys(&s["set"]) {
                    counter.set_box(&key, paint);
                }
            } else if s.find("remove").is_some() {
                for key in keys(&s["remove"]) {
                    counter.remove_box(&key);
                }
            } else if s.find("start").is_some() {
                if let Some(paint) = animation(&s["keyframes"]) {
                    for key in keys(&s["start"]) {
                        counter.start_animation(&key, s["animation"].as_str(), paint);
                    }
                }
            } else if s.find("end").is_some() {
                for key in keys(&s["end"]) {
                    counter.end_animation(&key, s["animation"].as_str());
                }
            } else if s.find("check").is_some() {
                let want = &s["check"];
                let verdict = counter.check();
                let stop = if want["stop"].is_null() {
                    None
                } else {
                    Some(Verdict {
                        code: PaintLimit::from_code(want["stop"]["code"].as_str()).expect("a code"),
                        count: want["stop"]["count"].as_int(0),
                    })
                };
                let at = format!("cases: {name}, step {step}");
                check(counter.heavy() == want["heavy"].as_int(0), format!("{at}: heavy {}", counter.heavy()));
                check(counter.backdrop() == want["backdrop"].as_int(0), format!("{at}: backdrop {}", counter.backdrop()));
                check(verdict == stop, format!("{at}: verdict {verdict:?}"));
            }
        }
    }
    eprintln!("{checks} checks");
    assert!(checks > 150);
}
