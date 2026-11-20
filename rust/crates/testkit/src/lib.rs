//! What the stages' tests share (rust/PLAN.md, "Tests"): the C++ suites' seeded generator and byte
//! mutator, drawing exactly as the C++ does so that a seed changes bytes the same way on both sides, and
//! the client of the `oracle/` programs, which take cases on stdin and answer each with a status and bytes.

#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// The precedent's 64-bit LCG (tests/decoder_checks.h, widget-sandbox's json fuzz): the high 31 bits of
/// each step.
#[derive(Clone, Debug)]
pub struct Lcg {
    pub state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.state >> 33) as u32
    }

    /// A draw below `n`; none is drawn for 0.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { self.next() as usize % n }
    }

    pub fn chance(&mut self, one_in: usize) -> bool {
        self.below(one_in) == 0
    }
}

/// u32 values at the edges of what a count, a length, a reference, a mask or float bits can be.
pub const EDGES: [u32; 19] = [
    0, 1, 2, 3, 4, 5, 0x7f, 0x80, 0xff, 0x100, 0xffff, 0x10000, 0x10000000, 0x20000000, 0x40000000, 0x7fffffff, 0x80000000, 0xfffffffe,
    0xffffffff,
];

/// One to three random changes to `bytes`, and what they were: tests/decoder_checks.h's `mutate`, the
/// same draws in the same order.
pub fn mutate(random: &mut Lcg, bytes: &mut Vec<u8>) -> String {
    let mut done = String::new();
    let edits = 1 + random.below(3);
    for _ in 0..edits {
        let at = random.below(bytes.len());
        match random.below(7) {
            0 => {
                let _ = write!(done, " flip@{at}");
                if !bytes.is_empty() {
                    bytes[at] ^= 1 << random.below(8);
                }
            }
            1 => {
                let _ = write!(done, " byte@{at}");
                if !bytes.is_empty() {
                    bytes[at] = random.next() as u8;
                }
            }
            2 => {
                if bytes.len() >= 4 {
                    let p = random.below(bytes.len() - 3);
                    let v = EDGES[random.below(EDGES.len())];
                    let _ = write!(done, " u32@{p}={v:#x}");
                    bytes[p..p + 4].copy_from_slice(&v.to_le_bytes());
                }
            }
            3 => {
                let n = 1 + random.below(4);
                let _ = write!(done, " delete@{at}+{n}");
                if !bytes.is_empty() {
                    let end = (at + n).min(bytes.len());
                    bytes.drain(at..end);
                }
            }
            4 => {
                let p = random.below(bytes.len() + 1);
                let n = 1 + random.below(4);
                let _ = write!(done, " insert@{p}+{n}");
                let added: Vec<u8> = (0..n).map(|_| random.next() as u8).collect();
                bytes.splice(p..p, added);
            }
            5 => {
                let n = (1 + random.below(8)).min(bytes.len() - at);
                let _ = write!(done, " repeat@{at}+{n}");
                if !bytes.is_empty() {
                    let piece = bytes[at..at + n].to_vec();
                    bytes.splice(at..at, piece);
                }
            }
            _ => {
                let _ = write!(done, " cut@{at}");
                bytes.truncate(at);
            }
        }
    }
    done
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(digits: &str) -> Vec<u8> {
    let clean: Vec<u8> = digits.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    clean.chunks(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("hex digits"), 16).expect("hex digits")).collect()
}

/// One answer of an oracle program: a status and bytes.
pub type Answer = (u8, Vec<u8>);

/// An `oracle/` program, found in `TWIDGET_ORACLE_DIR`.
#[derive(Debug)]
pub struct Oracle {
    program: PathBuf,
}

impl Oracle {
    /// The program `name` (without `.exe`); None when `TWIDGET_ORACLE_DIR` is not set, a failure when
    /// `TWIDGET_ORACLE_REQUIRED` says the run must compare or the program is not there.
    pub fn find(name: &str) -> Option<Oracle> {
        let required = std::env::var_os("TWIDGET_ORACLE_REQUIRED").is_some();
        let Some(dir) = std::env::var_os("TWIDGET_ORACLE_DIR") else {
            assert!(!required, "TWIDGET_ORACLE_REQUIRED is set but TWIDGET_ORACLE_DIR is not");
            eprintln!("TWIDGET_ORACLE_DIR is not set: the comparison with the C++ is skipped");
            return None;
        };
        let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
        let program = PathBuf::from(dir).join(file);
        assert!(program.is_file(), "no oracle at {}", program.display());
        Some(Oracle { program })
    }

    /// Runs the program on `input` and returns every answer it wrote, checking there are `expected`.
    pub fn run(&self, input: Vec<u8>, expected: usize) -> Vec<Answer> {
        let mut child = Command::new(&self.program)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("{} starts: {e}", self.program.display()));
        let mut stdin = child.stdin.take().expect("a piped stdin");
        let writer = std::thread::spawn(move || stdin.write_all(&input));
        let mut output = Vec::new();
        child.stdout.take().expect("a piped stdout").read_to_end(&mut output).expect("the oracle's output");
        writer.join().expect("the writer").expect("the oracle reads its input");
        let status = child.wait().expect("the oracle ends");
        assert!(status.success(), "{} ended with {status}", self.program.display());
        let mut answers = Vec::with_capacity(expected);
        let mut rest = &output[..];
        while !rest.is_empty() {
            assert!(rest.len() >= 5, "a cut answer");
            let n = u32::from_le_bytes(rest[1..5].try_into().expect("four bytes")) as usize;
            answers.push((rest[0], rest[5..5 + n].to_vec()));
            rest = &rest[5 + n..];
        }
        assert_eq!(answers.len(), expected, "the oracle answers every case");
        answers
    }
}

/// Cases for an oracle program and the answers the Rust side gives them, compared in one run.
#[derive(Debug, Default)]
pub struct Comparison {
    input: Vec<u8>,
    expected: Vec<(Answer, String)>,
}

impl Comparison {
    pub fn new() -> Comparison {
        Comparison::default()
    }

    /// Bytes that set something for the cases after them and get no answer.
    pub fn setting(&mut self, bytes: &[u8]) {
        self.input.extend_from_slice(bytes);
    }

    /// A case: its bytes for the program, the answer the Rust side gives, and what it is for a failure.
    pub fn case(&mut self, bytes: &[u8], rust: Answer, what: impl Into<String>) {
        self.input.extend_from_slice(bytes);
        self.expected.push((rust, what.into()));
    }

    pub fn len(&self) -> usize {
        self.expected.len()
    }

    pub fn is_empty(&self) -> bool {
        self.expected.is_empty()
    }

    /// Runs the cases and fails listing the first differences; returns how many answers had each status
    /// (statuses past 2 counted as 2).
    pub fn check(self, oracle: &Oracle) -> [usize; 3] {
        let answers = oracle.run(self.input, self.expected.len());
        let mut tally = [0; 3];
        let mut differences = Vec::new();
        for (cpp, (rust, what)) in answers.iter().zip(&self.expected) {
            tally[usize::from(cpp.0.min(2))] += 1;
            if cpp != rust {
                differences.push(describe(what, cpp, rust));
            }
        }
        assert!(
            differences.is_empty(),
            "{} of {} answers differ, the first:\n{}",
            differences.len(),
            answers.len(),
            differences.iter().take(10).cloned().collect::<Vec<_>>().join("\n")
        );
        tally
    }
}

fn describe(what: &str, cpp: &Answer, rust: &Answer) -> String {
    let at = cpp.1.iter().zip(&rust.1).position(|(a, b)| a != b).unwrap_or(cpp.1.len().min(rust.1.len()));
    let from = at.saturating_sub(8);
    let near = |b: &[u8]| {
        let piece = &b[from.min(b.len())..(at + 24).min(b.len())];
        if piece.iter().all(|c| (0x20..0x7f).contains(c)) { format!("{:?}", String::from_utf8_lossy(piece)) } else { hex(piece) }
    };
    format!(
        "{what}: status C++ {} / Rust {}, {} / {} bytes, first difference at byte {at}: C++ …{} / Rust …{}",
        cpp.0,
        rust.0,
        cpp.1.len(),
        rust.1.len(),
        near(&cpp.1),
        near(&rust.1)
    )
}

/// A record of the oracles' framing: a tag, a u32 length and the bytes.
pub fn framed(tag: u8, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 5);
    out.push(tag);
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
    out
}
