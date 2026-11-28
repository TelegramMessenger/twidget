//! The Rust codec against the C++ one (`oracle/wire_oracle`, built from core/src/wire.cpp): the same
//! bytes go through both decoders, what either takes is encoded again by both, and the answers must be
//! the same byte for byte. Skipped without `TWIDGET_ORACLE_DIR`; `tools/gate.py` sets it.

mod common;

use common::*;
use twidget_wire::*;

struct Run {
    cases: Vec<Case>,
    expected: Vec<((u8, Vec<u8>), String)>,
    caps: Caps,
}

impl Run {
    fn new() -> Run {
        Run { cases: vec![Case::Caps(Caps::default())], expected: Vec::new(), caps: Caps::default() }
    }

    fn caps(&mut self, caps: Caps) {
        self.cases.push(Case::Caps(caps));
        self.caps = caps;
    }

    fn same(&mut self, form: FormKind, bytes: Vec<u8>, what: String) {
        self.expected.push((form.answer(&bytes, &self.caps, &self.caps), what));
        self.cases.push(Case::Same(form, bytes));
    }

    fn starts(&mut self, most: u64, bytes: Vec<u8>, what: String) {
        self.expected.push((starts_answer(&bytes, &self.caps, most), what));
        self.cases.push(Case::Starts(most, bytes));
    }

    fn encode(&mut self, form: FormKind, bytes: Vec<u8>, what: String) {
        self.expected.push((form.answer(&bytes, &Caps::default(), &self.caps), what));
        self.cases.push(Case::Encode(form, bytes));
    }

    /// Runs the cases through the C++ and returns how many answers were of each status.
    fn check(self, oracle: &Oracle) -> [usize; 3] {
        let answers = oracle.run(&self.cases);
        assert_eq!(answers.len(), self.expected.len());
        let mut tally = [0; 3];
        let mut differences = Vec::new();
        for (cpp, (rust, what)) in answers.iter().zip(&self.expected) {
            tally[usize::from(cpp.0.min(2))] += 1;
            if cpp != rust {
                let at = cpp.1.iter().zip(&rust.1).position(|(a, b)| a != b).unwrap_or(cpp.1.len().min(rust.1.len()));
                let from = at.saturating_sub(8);
                let near = |b: &[u8]| hex(&b[from.min(b.len())..(at + 24).min(b.len())]);
                differences.push(format!(
                    "{what}: status C++ {} / Rust {}, {} / {} bytes, first difference at byte {at}: C++ …{} / Rust …{}",
                    cpp.0,
                    rust.0,
                    cpp.1.len(),
                    rust.1.len(),
                    near(&cpp.1),
                    near(&rust.1)
                ));
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

#[test]
fn samples_decode_and_encode_alike() {
    let Some(oracle) = oracle() else { return };
    let mut run = Run::new();
    for caps in [Caps::default(), tight()] {
        run.caps(caps);
        for (form, name, bytes) in encoded_samples(&caps) {
            for n in 0..bytes.len() {
                run.same(form, bytes[..n].to_vec(), format!("{form:?} {name} cut to {n}"));
            }
            run.same(form, bytes, format!("{form:?} {name}"));
        }
    }
    run.same(FormKind::List, full_list_bytes(), "the documented list".into());
    run.same(FormKind::Batch, full_batch_bytes(), "the documented batch".into());
    let tally = run.check(&oracle);
    assert!(tally[0] > 0 && tally[1] > 0);
}

#[test]
fn mutated_samples_decode_alike() {
    let Some(oracle) = oracle() else { return };
    let seeds =
        [(FormKind::List, 0x11570000u64), (FormKind::Batch, 0xba7c0000), (FormKind::Damage, 0xda3a0000), (FormKind::Metrics, 0x3e7c0000)];
    let mut run = Run::new();
    for (c, caps) in [Caps::default(), tight()].into_iter().enumerate() {
        run.caps(caps);
        for (form, seed) in seeds {
            let samples: Vec<_> = encoded_samples(&caps).into_iter().filter(|(f, _, _)| *f == form).collect();
            for (index, (_, name, bytes)) in samples.iter().enumerate() {
                let start = seed + 0x100 * c as u64 + index as u64;
                let mut random = Lcg::new(start);
                for round in 0..ROUNDS {
                    let mut changed = bytes.clone();
                    let edits = mutate(&mut random, &mut changed);
                    run.same(form, changed, format!("{form:?} {name} seed {start:#x} round {round}:{edits}"));
                }
            }
        }
    }
    let tally = run.check(&oracle);
    eprintln!("mutated samples: {} refused, {} taken by both", tally[0], tally[1]);
    assert!(tally[0] > 0 && tally[1] > 0);
}

/// Generated values: what Rust encodes, the C++ decodes and encodes again to the same bytes; mutated,
/// both decoders take or refuse it alike.
#[test]
fn generated_values_encode_alike() {
    let Some(oracle) = oracle() else { return };
    let mut random = Lcg::new(0x6e6e_0001);
    let mut run = Run::new();
    for i in 0..3000 {
        let mut bytes = Vec::new();
        let form = match i % 4 {
            0 => {
                assert!(encode_list(&any_list(&mut random), &Caps::default(), &mut bytes));
                FormKind::List
            }
            1 => {
                assert!(encode_batch(&any_batch(&mut random), &Caps::default(), &mut bytes));
                FormKind::Batch
            }
            2 => {
                assert!(encode_damage(&any_damage(&mut random), &Caps::default(), &mut bytes));
                FormKind::Damage
            }
            _ => {
                encode_metrics(&any_metrics(&mut random), &mut bytes);
                FormKind::Metrics
            }
        };
        let (status, again) = form.answer(&bytes, &Caps::default(), &Caps::default());
        assert!(status == 1 && again == bytes, "generated value {i} comes back to its bytes in Rust");
        for m in 0..4 {
            let mut changed = bytes.clone();
            let edits = mutate(&mut random, &mut changed);
            run.same(form, changed, format!("generated {form:?} {i}, change {m}:{edits}"));
        }
        run.same(form, bytes, format!("generated {form:?} {i}"));
    }
    let tally = run.check(&oracle);
    eprintln!("generated values: {} refused, {} taken by both", tally[0], tally[1]);
}

/// The command starts both encoders note: every sample list and generated list, and their mutations, under the default,
/// tight and one-command caps, with the bytes given at the list's own length, a byte less, below its head and none.
#[test]
fn command_starts_alike() {
    let Some(oracle) = oracle() else { return };
    let mut random = Lcg::new(0x6e6e_0003);
    let mut lists: Vec<(String, Vec<u8>)> = Vec::new();
    for (form, name, bytes) in encoded_samples(&Caps::default()) {
        if form == FormKind::List {
            lists.push((name.to_string(), bytes));
        }
    }
    for i in 0..1500 {
        let mut bytes = Vec::new();
        assert!(encode_list(&any_list(&mut random), &Caps::default(), &mut bytes));
        let mut changed = bytes.clone();
        mutate(&mut random, &mut changed);
        lists.push((format!("generated {i}"), bytes));
        lists.push((format!("generated {i}, changed"), changed));
    }
    let mut run = Run::new();
    let mut one = Caps::default();
    one.commands = 1;
    for caps in [Caps::default(), tight(), one] {
        run.caps(caps);
        for (name, bytes) in &lists {
            let n = bytes.len() as u64;
            for most in [u64::MAX, n, n.saturating_sub(1), n / 2, 6, 5, 0] {
                run.starts(most, bytes.clone(), format!("{name} with its starts within {most} bytes"));
            }
        }
    }
    let tally = run.check(&oracle);
    eprintln!("command starts: {} refused, {} noted by both, {} encodes refused", tally[0], tally[1], tally[2]);
    assert!(tally[0] > 0 && tally[1] > 0);
}

/// Encoders refuse alike: values decoded under the default caps, encoded under each tight cap alone.
#[test]
fn encoders_refuse_alike() {
    let Some(oracle) = oracle() else { return };
    let mut random = Lcg::new(0x6e6e_0002);
    let mut values = Vec::new();
    for i in 0..400 {
        let mut bytes = Vec::new();
        let form = match i % 3 {
            0 => {
                assert!(encode_list(&any_list(&mut random), &Caps::default(), &mut bytes));
                FormKind::List
            }
            1 => {
                assert!(encode_batch(&any_batch(&mut random), &Caps::default(), &mut bytes));
                FormKind::Batch
            }
            _ => {
                assert!(encode_damage(&any_damage(&mut random), &Caps::default(), &mut bytes));
                FormKind::Damage
            }
        };
        values.push((form, bytes));
    }
    for (form, _, bytes) in encoded_samples(&Caps::default()) {
        if form != FormKind::Metrics {
            values.push((form, bytes));
        }
    }
    let mut run = Run::new();
    for name in CAP_NAMES {
        for limit in [0usize, 1, 2, 3] {
            let mut caps = Caps::default();
            *cap_mut(&mut caps, name) = limit;
            run.caps(caps);
            for (i, (form, bytes)) in values.iter().enumerate() {
                run.encode(*form, bytes.clone(), format!("value {i} ({form:?}) encoded under {name} = {limit}"));
            }
        }
    }
    let tally = run.check(&oracle);
    eprintln!("encoders under single caps: {} encoded, {} refused", tally[1], tally[2]);
    assert!(tally[1] > 0 && tally[2] > 0);
}
