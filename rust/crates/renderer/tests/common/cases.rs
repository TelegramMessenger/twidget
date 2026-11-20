//! Scripts through the oracle and the port, compared: the C++'s tape and transcript first, then the port's run on that
//! tape, transcript for transcript.

use twidget_testkit::{Oracle, hex};

use super::{Run, Script, Tally, run, split};

/// Scripts compared in runs of the oracle of at most this many, so that a run's answers stay small.
const CHUNK: usize = 48;

/// Every script through both; the scripts that differ, with where, fail the test.
pub fn compare(oracle: &Oracle, scripts: &[(Script, String)], tally: &mut Tally) -> Vec<Run> {
    let mut runs = Vec::with_capacity(scripts.len());
    let mut differences = Vec::new();
    for chunk in scripts.chunks(CHUNK) {
        let input: Vec<u8> = chunk.iter().flat_map(|(s, _)| s.record()).collect();
        let answers = oracle.run(input, chunk.len());
        for ((script, what), (status, answer)) in chunk.iter().zip(&answers) {
            let (tape, cpp) = split(answer);
            let r = run(script, tape, tally);
            if *status != 1 || !r.errors.is_empty() || cpp != r.transcript.as_slice() {
                let rust = &r.transcript;
                let at = cpp.iter().zip(rust).position(|(a, b)| a != b).unwrap_or(cpp.len().min(rust.len()));
                let near = |b: &[u8]| {
                    let piece = &b[at.saturating_sub(24).min(b.len())..(at + 64).min(b.len())];
                    if piece.iter().all(|c| (0x20..0x7f).contains(c)) {
                        format!("{:?}", String::from_utf8_lossy(piece))
                    } else {
                        hex(piece)
                    }
                };
                differences.push(format!(
                    "{what}: {}; transcripts {} / {} bytes, first difference at {at}: C++ …{} / Rust …{}",
                    if r.errors.is_empty() { "the tape replays".to_string() } else { r.errors.join("; ") },
                    cpp.len(),
                    rust.len(),
                    near(cpp),
                    near(rust)
                ));
            }
            runs.push(r);
        }
    }
    assert!(
        differences.is_empty(),
        "{} of {} scripts differ, the first:\n{}",
        differences.len(),
        scripts.len(),
        differences[..differences.len().min(6)].join("\n")
    );
    runs
}

pub fn report(name: &str, scripts: usize, tally: &Tally) {
    eprintln!("{name}: {scripts} scripts alike;{}", tally.show());
}
