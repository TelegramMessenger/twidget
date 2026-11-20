//! widget-sandbox's JSON tests (packages/desktop/tests/test_main.cpp: json_fuzz_never_crashes,
//! json_as_int_saturates, json_parse_and_dump, the surrogate cases of the SDK test), ported. Its
//! json_numbers_ignore_lc_numeric has no counterpart: nothing in the Rust port reads a locale.

mod common;

use common::fuzz_texts;
use twidget_json::*;

#[test]
fn json_fuzz_never_crashes() {
    let mut parsed = 0;
    for text in fuzz_texts() {
        if let Some(v) = Json::parse(&text) {
            parsed += 1;
            let dumped = v.dump();
            let again = Json::parse(&dumped).expect("a dump parses again");
            assert_eq!(again.dump(), dumped, "a dump dumps the same again");
        }
    }
    assert!(parsed > 0);
    assert!(Json::parse(vec![b'['; 200000]).is_none());
    let _ = Json::parse(b"\"\\ud800");
}

/// Numbers the sandbox sends are any doubles: as_int answers the nearest end of `i32` past it.
#[test]
fn json_as_int_saturates() {
    let v = Json::parse("[1e300,-1e300,2147483648,-2147483649,2147483647.5,-2147483648.5,1.9,-1.9,1e999,\"7\"]").expect("parses");
    assert_eq!(v[0].as_int(0), i32::MAX);
    assert_eq!(v[1].as_int(0), i32::MIN);
    assert_eq!(v[2].as_int(0), i32::MAX);
    assert_eq!(v[3].as_int(0), i32::MIN);
    assert_eq!(v[4].as_int(0), i32::MAX);
    assert_eq!(v[5].as_int(0), i32::MIN);
    assert_eq!(v[6].as_int(0), 1);
    assert_eq!(v[7].as_int(0), -1);
    assert_eq!(v[8].as_int(5), 5);
    assert_eq!(v[9].as_int(5), 5);
}

fn shortest_round_trips(number: fn(f64) -> String, which: &str) {
    assert_eq!(number(1234.5), "1234.5", "{which}");
    assert_eq!(number(0.1), "0.1", "{which}");
    assert_eq!(number(16.7), "16.7", "{which}");
    assert_eq!(number(-0.25), "-0.25", "{which}");
    for d in [1.0 / 3.0, 2.0 / 3.0, 0.1 + 0.2, 1e-7, 123456.789e10, 5e-324, 1.7976931348623157e308] {
        let text = number(d);
        assert_eq!(Json::parse(&text).expect("parses").as_number(0.0), d, "{which}: {text}");
        assert!(text.len() <= 24, "{which}: {text}");
    }
}

#[test]
fn json_parse_and_dump() {
    let v = Json::parse("{\"a\":[1,2.5,\"x\\u00e9\\ud83d\\ude00\",true,null],\"b\":{}, \"c\":-1e2}").expect("parses");
    assert_eq!(v.dump_lossy(), "{\"a\":[1,2.5,\"xé\u{1F600}\",true,null],\"b\":{},\"c\":-100}");
    assert_eq!(v["a"][2].as_str(), "xé\u{1F600}".as_bytes());
    assert_eq!(v["c"].as_number(0.0), -100.0);
    assert!(Json::parse("[1,]").is_none());
    assert!(Json::parse("{\"a\":1} x").is_none());
    assert!(Json::parse("{a:1}").is_none());
    assert!(Json::parse("[".repeat(300)).is_none());
    assert!(Json::parse("[".repeat(100) + &"]".repeat(100)).is_some());
    assert_eq!(number(3.0), "3");
    assert_eq!(number(1e21), "1e+21");
    shortest_round_trips(number, "number");
    shortest_round_trips(to_chars_shortest, "to_chars");
    shortest_round_trips(shortest_of_15_to_17, "the Apple fallback");
    assert_eq!(quote(b"a\"b\\c\n\x01"), b"\"a\\\"b\\\\c\\n\\u0001\"");
    let mut o = Json::object();
    o.set("k", "v").set("n", 2);
    assert_eq!(o.dump_lossy(), "{\"k\":\"v\",\"n\":2}");
}

/// The JSON route of a lone surrogate: kept as itself when asked to, U+FFFD otherwise, and a lone high
/// surrogate does not swallow the escape after it.
#[test]
fn json_surrogates() {
    let keep = |t: &str| Json::parse_with(t, Surrogates::Keep, DEFAULT_MAX_DEPTH).expect("parses").as_str().to_vec();
    let replace = |t: &str| Json::parse(t).expect("parses").as_str().to_vec();
    assert_eq!(keep("\"\\ud800\""), b"\xED\xA0\x80");
    assert_eq!(keep("\"\\udc00x\""), b"\xED\xB0\x80x");
    assert_eq!(keep("\"\\ud83d\\ude00\""), b"\xF0\x9F\x98\x80");
    assert_eq!(replace("\"\\ud800\""), b"\xEF\xBF\xBD");
    assert_eq!(replace("\"\\ud800\\u0041\""), b"\xEF\xBF\xBDA");
    assert_eq!(keep("\"\\ud800\\u0041\""), b"\xED\xA0\x80A");
    assert!(Json::parse("\"\\ud800\\u00zz\"").is_none(), "the escape after a high surrogate is read whole");
}

#[test]
fn json_accessors() {
    let mut v = Json::parse("{\"a\":1,\"a\":2,\"s\":\"x\"}").expect("parses");
    assert_eq!(v.size(), 3, "duplicate members are kept");
    assert_eq!(v["a"].as_number(0.0), 1.0, "the first is found");
    v.set("a", 3);
    assert_eq!(v.dump_lossy(), "{\"a\":3,\"a\":2,\"s\":\"x\"}", "set replaces the first");
    assert!(v["missing"].is_null() && v[0].is_null() && v["s"][3].is_null());
    let mut a = Json::Null;
    a.push(1).push("two");
    assert_eq!(a.dump_lossy(), "[1,\"two\"]");
    let mut b = Json::from(5);
    b.set("k", true);
    assert_eq!(b.dump_lossy(), "{\"k\":true}", "set makes an object of anything else");
    assert_eq!(number(f64::NAN), "null");
    assert_eq!(number(-0.0), "0");
    assert_eq!(number(1e15), "1e+15");
    assert_eq!(number(999999999999999.0), "999999999999999");
    assert_eq!(Json::parse(b"\"\x80\xff\"").expect("bytes pass through").as_str(), b"\x80\xff");
    assert!(Json::parse_with("[]", Surrogates::Replace, -1).is_none(), "a negative bound takes nothing");
    assert!(Json::parse_with("1", Surrogates::Replace, 0).is_some());
    assert!(Json::parse_with("[1]", Surrogates::Replace, 0).is_none());
}
