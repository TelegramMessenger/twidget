//! Values the helpers must give, checked without the oracle: what the C++ gives for them, including the
//! ways it is not the CSS specification's (the 63-character number buffer, strtod's whole grammar).

use twidget_css::*;
use twidget_cstd::{atoi, format_fixed, strtod};
use twidget_json::Json;
use twidget_wire::Color;

#[test]
fn strtod_reads_the_c_grammar() {
    assert_eq!(strtod(b"12.5px"), (12.5, 4));
    assert_eq!(strtod(b"  -.5e1x"), (-5.0, 7));
    assert_eq!(strtod(b"1e"), (1.0, 1), "an exponent without digits is not taken");
    assert_eq!(strtod(b"0x1p3"), (8.0, 5));
    assert_eq!(strtod(b"0x.8"), (0.5, 4));
    assert_eq!(strtod(b"0xg"), (0.0, 1), "0x without a hex digit is the 0 alone");
    assert_eq!(strtod(b"0x1p-1074").0.to_bits(), 1, "the smallest subnormal");
    assert_eq!(strtod(b"0x1p-1075").0, 0.0, "half of it is a tie to the even zero");
    assert_eq!(strtod(b"0x1.8p-1075").0.to_bits(), 1, "more than half rounds up");
    assert_eq!(strtod(b"0x1.fffffffffffff8p1023").0, f64::INFINITY, "rounding past the largest double");
    assert_eq!(strtod(b"0x1.fffffffffffff7p1023").0, f64::MAX);
    assert_eq!(strtod(b"-inFinity"), (f64::NEG_INFINITY, 9));
    assert_eq!(strtod(b"infinit").1, 3);
    assert!(strtod(b"nan(abc_1)x").0.is_nan() && strtod(b"nan(abc_1)x").1 == 10);
    assert_eq!(strtod(b"nan(a b)").1, if cfg!(target_vendor = "apple") { 8 } else { 3 });
    assert_eq!(strtod(b"."), (0.0, 0));
    assert_eq!(strtod(b"\x0b5"), (5.0, 2), "\\v is C's whitespace");
    assert_eq!(atoi(b"700"), 700);
    assert_eq!(atoi(b"3000000000"), if core::ffi::c_long::BITS == 32 { i32::MAX } else { -1_294_967_296 });
    assert_eq!(format_fixed(-0.0, 2), "-0.00");
    assert_eq!(format_fixed(0.125, 2), "0.12", "ties to even");
}

#[cfg(target_vendor = "apple")]
#[test]
fn apple_nan_payloads_keep_their_bits_and_end() {
    for (input, payload, end) in [
        (b"nan(123)px".as_slice(), 123u64, 8),
        (b"nan(051)", 41, 8),
        (b"nan(0x80123456)", 0x80123456, 15),
        (b"nan(0x10000000000000001)", 1, 24),
        (b"nan(0Xff)", 0, 9),
        (b"nan(9 9)", 0, 8),
        (b"nan( v(a)b)", 0, 9),
        (b"nan(12\0)px", 0, 3),
        (b"nan(12", 0, 3),
    ] {
        for (prefix, sign) in [(b"".as_slice(), 0), (b" -", 1u64)] {
            let text = [prefix, input].concat();
            let (d, nd) = strtod(&text);
            let (f, nf) = twidget_cstd::strtof(&text);
            let double = sign << 63 | 0x7ff8_0000_0000_0000 | payload;
            let single = (sign as u32) << 31 | 0x7fc0_0000 | (payload as u32 & 0x003f_ffff);
            assert_eq!((d.to_bits(), nd), (double, end + prefix.len()));
            assert_eq!((f.to_bits(), nf), (single, end + prefix.len()));
        }
    }
}

#[test]
fn numbers() {
    assert_eq!(number(b" 1.5 "), Some(1.5));
    assert_eq!(number(b"0x10"), Some(16.0), "strtod reads hexadecimal");
    assert_eq!(number(b"inf"), None, "not finite");
    assert_eq!(number(b"1.5px"), None, "not the whole text");
    assert_eq!(format_number(1.5, 4), "1.5");
    assert_eq!(format_number(1.0 / 3.0, 4), "0.3333");
    assert_eq!(format_number(-0.00001, 4), "0", "-0 is 0");
    assert_eq!(format_number(1e9, 4), "1000000000");
    assert_eq!(format_number(f64::NAN, 4), "0");
    let huge = format_number(1e100, 4);
    assert_eq!(huge.len(), 63, "the C++ buffer holds 63 characters: {huge}");
}

#[test]
fn colors() {
    assert_eq!(parse_color(b"#f00"), Some(Color::rgba(255, 0, 0, 255)));
    assert_eq!(parse_color(b"#ff000080"), Some(Color::rgba(255, 0, 0, 128)));
    assert_eq!(parse_color(b"rgb(100%, 50%, 0)"), Some(Color::rgba(255, 128, 0, 255)));
    assert_eq!(parse_color(b"rgba(1 2 3 / 50%)"), Some(Color::rgba(1, 2, 3, 128)));
    assert_eq!(parse_color(b"hsl(120deg, 100%, 50%)"), Some(Color::rgba(0, 255, 0, 255)));
    assert_eq!(parse_color(b"RebeccaPurple"), Some(Color::rgba(0x66, 0x33, 0x99, 255)));
    assert_eq!(parse_color(b"transparent"), Some(Color::rgba(0, 0, 0, 0)));
    assert_eq!(parse_color(b"currentColor"), Some(Color::rgba(0, 0, 0, 255)));
    assert_eq!(parse_color(b"#12345"), None);
    assert_eq!(normalize_colors(b"red #00f8 x-red"), b"rgba(255, 0, 0, 1) rgba(0, 0, 255, 0.5333) x-red".to_vec());
}

#[test]
fn text_rewrites() {
    assert_eq!(resolve_light_dark(b"a light-dark(#fff, #000) b", true, 0), b"a #000 b".to_vec());
    assert_eq!(resolve_light_dark(b"'light-dark(a,b)'", true, 0), b"'light-dark(a,b)'".to_vec(), "inside a string it stays");
    assert_eq!(resolve_light_dark(b"light-dark(light-dark(1,2),3)", false, 0), b"1".to_vec());
    let deep = format!("{}x{}", "light-dark(".repeat(40), ")".repeat(40));
    assert!(resolve_light_dark(deep.as_bytes(), false, 0).is_empty(), "past 32 levels the side is dropped");
    let vars = vec![(b"--a".to_vec(), b"red".to_vec())];
    assert_eq!(substitute_variables(b"var(--a) var(--b, rgb(1, 2, 3)) var(--c)", &vars), b"red rgb(1, 2, 3) var(--c)".to_vec());
    assert_eq!(unwrap_where(b":where(a) {} :where(a,b).x {} :where(a, b){}"), b"a {} :where(a,b).x {} a, b{}".to_vec());
    assert_eq!(unwrap_where(b":where(a, b) {}"), b":where(a, b) {}".to_vec(), "a list unwraps only right before `{{` or `,`");
    let d = parse_declarations(b"Color: red; --X: 1 !IMPORTANT; color: blue; b@d: 1; : x");
    assert_eq!(d.len(), 2);
    assert_eq!((d[0].name.as_slice(), d[0].value.as_slice(), d[0].important), (&b"--X"[..], &b"1"[..], true));
    assert_eq!((d[1].name.as_slice(), d[1].value.as_slice()), (&b"color"[..], &b"blue"[..]), "a later duplicate wins");
    assert_eq!(split_top_level(b"a, rgb(1,2), 'x,y',", b','), vec![b"a".to_vec(), b" rgb(1,2)".to_vec(), b" 'x,y'".to_vec()]);
}

#[test]
fn transforms_and_fonts() {
    let m = parse_transform(b"translate(10px, 50%) scale(2)", 100.0, 40.0).expect("parses");
    assert_eq!((m.a, m.d, m.e, m.f), (2.0, 2.0, 10.0, 20.0));
    assert!(parse_transform(b"translate(1em)", 1.0, 1.0).is_none());
    assert!(parse_transform(b"perspective(1px)", 1.0, 1.0).is_none());
    let r = parse_rotate(b"90deg").expect("parses");
    assert_eq!((r.a, r.b), ((std::f64::consts::FRAC_PI_2).cos() as f32, 1.0));
    assert_eq!(parse_origin(b"top left", 10.0, 20.0), Some((0.0, 0.0)));
    assert_eq!(parse_origin(b"", 10.0, 20.0), Some((5.0, 10.0)));
    assert_eq!(parse_opacity(b"150%"), Some(1.0));
    assert_eq!(parse_time(b"calc(1s + 500ms)"), Some(1.5));
    assert_eq!(parse_time(b"2"), None);
    let f = parse_font(b"italic bold 12pt/1.5 'Helvetica Neue', serif");
    assert_eq!((f.italic, f.weight, f.size), (true, 700, 12.0 * (96.0f32 / 72.0f32)));
    assert_eq!(f.family, b"'Helvetica Neue', serif".to_vec());
    assert_eq!(parse_font(b"").family, b"sans-serif".to_vec());
}

#[test]
fn theme_variables_follow_the_sdk() {
    let theme = Json::parse(r##"{"colorScheme":"dark","params":{"bg_color":"#ABCDEF","text_color":"red"},"button":{"radius":"8px","pressedOpacity":0.5,"font":"15px Arial"},"text":{"fontSize":15.555}}"##).expect("parses");
    let vars = theme_variables(&theme);
    let get = |name: &str| vars.iter().find(|(k, _)| k.as_slice() == name.as_bytes()).map(|(_, v)| String::from_utf8_lossy(v).into_owned());
    assert_eq!(get("--tg-theme-bg-color").as_deref(), Some("#abcdef"));
    assert_eq!(get("--tg-theme-text-color").as_deref(), Some("#ffffff"), "a bad colour takes the dark default");
    assert_eq!(get("--tg-color-scheme").as_deref(), Some("dark"));
    assert_eq!(get("--tg-button-radius").as_deref(), Some("8px"));
    assert_eq!(get("--tg-button-pressed-opacity").as_deref(), Some("0.5"));
    assert_eq!(get("--tg-button-font"), None, "only generic families");
    assert_eq!(get("--tg-text-font-size").as_deref(), Some("15.56px"));
    assert_eq!(get("--tg-text-line-height").as_deref(), Some("1.35"), "the reference value when left out");
    let inset = content_safe_area_variables(&Json::parse(r#"{"top":24.005,"cornerLeft":250}"#).expect("parses"), false);
    assert_eq!(inset[0].1, b"24.01px".to_vec());
    assert_eq!(inset[4].1, b"0px".to_vec(), "past 200 is 0");
    assert!(is_theme_variable(b"--tg-theme-bottom-bar-bg-color") && !is_theme_variable(b"--tg-theme-other"));
    for (name, _) in &vars {
        assert!(is_theme_variable(name), "{} is the host's", String::from_utf8_lossy(name));
    }
    assert!(
        !is_theme_variable(b"--tg-theme-bg_color") && !is_theme_variable(b"--tg-theme-bg-colors") && !is_theme_variable(b"--tg-theme-")
    );
}

/// Both safe-area layers and the viewport in a bubble and on a profile page (docs/sdk-v1.md §2.17, §2.19),
/// as the merged core/src/css.cpp makes them.
#[test]
fn place_variables_follow_the_sdk() {
    let json = |text: &str| Json::parse(text).expect("parses");
    let text = |vars: Variables| -> Vec<String> {
        vars.into_iter().map(|(k, v)| format!("{}:{}", String::from_utf8_lossy(&k), String::from_utf8_lossy(&v))).collect()
    };
    let content = |top: &str, right: &str, bottom: &str, left: &str, corner_left: &str, corner_right: &str| {
        vec![
            format!("--tg-content-safe-area-inset-top:{top}"),
            format!("--tg-content-safe-area-inset-right:{right}"),
            format!("--tg-content-safe-area-inset-bottom:{bottom}"),
            format!("--tg-content-safe-area-inset-left:{left}"),
            format!("--tg-content-safe-area-corner-left:{corner_left}"),
            format!("--tg-content-safe-area-corner-right:{corner_right}"),
        ]
    };
    let device = |top: &str, right: &str, bottom: &str, left: &str| {
        vec![
            format!("--tg-safe-area-inset-top:{top}"),
            format!("--tg-safe-area-inset-right:{right}"),
            format!("--tg-safe-area-inset-bottom:{bottom}"),
            format!("--tg-safe-area-inset-left:{left}"),
        ]
    };

    let edges = json(r#"{"top":64,"right":64.004,"bottom":64.006,"left":-1,"cornerLeft":200,"cornerRight":200.01}"#);
    assert_eq!(text(content_safe_area_variables(&edges, false)), content("64px", "0px", "0px", "0px", "200px", "0px"));
    assert_eq!(text(content_safe_area_variables(&edges, true)), content("64px", "64px", "64.01px", "0px", "0px", "0px"));
    let tall = json(r#"{"top":56,"bottom":120,"cornerLeft":0,"cornerRight":30}"#);
    assert_eq!(text(content_safe_area_variables(&tall, true)), content("56px", "0px", "120px", "0px", "0px", "0px"));
    assert_eq!(text(content_safe_area_variables(&tall, false)), content("56px", "0px", "0px", "0px", "0px", "30px"));

    let malformed = json(r#"{"top":"12","right":null,"bottom":true,"cornerLeft":[1]}"#);
    for page in [false, true] {
        let zero = content("0px", "0px", "0px", "0px", "0px", "0px");
        assert_eq!(text(content_safe_area_variables(&malformed, page)), zero);
        assert_eq!(text(content_safe_area_variables(&Json::Null, page)), zero);
        assert_eq!(text(safe_area_variables(&malformed, page)), device("0px", "0px", "0px", "0px"));
        assert_eq!(text(safe_area_variables(&Json::Null, page)), device("0px", "0px", "0px", "0px"));
    }

    let edges = json(r#"{"top":47,"right":-3,"bottom":34.567,"left":"5"}"#);
    assert_eq!(text(safe_area_variables(&edges, false)), device("0px", "0px", "0px", "0px"));
    assert_eq!(text(safe_area_variables(&edges, true)), device("47px", "0px", "34.57px", "0px"));
    assert_eq!(text(safe_area_variables(&json(r#"{"left":1000}"#), true)), device("0px", "0px", "0px", "1000px"));

    let huge = json(r#"{"bottom":1e308}"#);
    assert_eq!(text(safe_area_variables(&huge, true)), device("0px", "0px", "1e+308px", "0px"));
    assert_eq!(text(content_safe_area_variables(&huge, true)), content("0px", "0px", "1e+308px", "0px", "0px", "0px"));
    let (length, end) = strtod(b"1e+308px");
    assert!(length.is_finite() && end == 6);

    for (page, height, expected) in [
        (true, "844.567", 844.57),
        (true, "844", 844.0),
        (false, "844", 0.0),
        (true, "4000", 4000.0),
        (true, "3999.996", 4000.0),
        (true, "4000.004", 0.0),
        (true, "4000.01", 0.0),
        (true, "0.004", 0.0),
        (true, "0", 0.0),
        (true, "-1", 0.0),
        (true, "\"844\"", 0.0),
        (true, "null", 0.0),
    ] {
        assert_eq!(page_viewport_height(page, &json(height)), expected, "viewport {height} on a page: {page}");
    }
    assert_eq!(page_viewport_height(true, &Json::Null), 0.0);
    assert_eq!(PAGE_MAX_HEIGHT, 4000.0);

    for name in [
        "--tg-safe-area-inset-top",
        "--tg-safe-area-inset-right",
        "--tg-safe-area-inset-bottom",
        "--tg-safe-area-inset-left",
        "--tg-content-safe-area-corner-right",
        VIEWPORT_HEIGHT_VARIABLE,
    ] {
        assert!(is_theme_variable(name.as_bytes()), "{name} is the host's");
    }
    assert_eq!(VIEWPORT_HEIGHT_VARIABLE, "--tg-viewport-height");
    assert!(!is_theme_variable(b"--tg-safe-area-inset-middle") && !is_theme_variable(b"--tg-viewport-width"));
}
