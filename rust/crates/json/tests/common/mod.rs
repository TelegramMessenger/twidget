//! What the JSON tests share: widget-sandbox's json_fuzz_never_crashes text generator.

/// json_fuzz_never_crashes' text generator, the same draws in the same order.
pub fn fuzz_texts() -> Vec<Vec<u8>> {
    let mut state: u64 = 0x9E3779B97F4A7C15;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) as u32
    };
    let seeds: [&[u8]; 3] = [
        b"{\"type\":\"result\",\"qid\":1,\"result\":{\"w\":[1,2.5e300,-0.0,\"\\ud83d\\ude00\"]}}",
        b"[\"\\u0000\",{\"a\":{\"b\":[null,true,false]}},-1e-400,1E+999]",
        b"{\"ops\":[{\"op\":\"el\",\"id\":2,\"tag\":\"div\",\"attrs\":{\"class\":\"x\"}}]}",
    ];
    let alphabet: &[u8] = b"{}[]\":,.-+eE0123456789truefalsn \\u/abcdef\x00\x80\xff";
    let pick = |n: u32| alphabet[n as usize % alphabet.len()];
    let mut out = Vec::with_capacity(60000);
    for t in 0..60000 {
        let mut text: Vec<u8>;
        if t % 2 == 0 {
            text = seeds[next() as usize % 3].to_vec();
            let edits = 1 + next() % 6;
            let mut e = 0;
            while e < edits && !text.is_empty() {
                let at = next() as usize % text.len();
                match next() % 4 {
                    0 => text[at] = pick(next()),
                    1 => {
                        let n = 1 + next() as usize % 4;
                        let end = (at + n).min(text.len());
                        text.drain(at..end);
                    }
                    2 => text.insert(at, pick(next())),
                    _ => text.truncate(at),
                }
                e += 1;
            }
        } else {
            let n = next() % 48;
            text = (0..n).map(|_| pick(next())).collect();
        }
        out.push(text);
    }
    out
}
