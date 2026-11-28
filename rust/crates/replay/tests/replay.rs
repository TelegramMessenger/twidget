//! T5, the renderer's gate (stage 5.2): recordings of the C++ renderer replayed through the Rust renderer end, step
//! for step, as `twidget_parity_*` replays them through the C++ one. The committed stub recording
//! (`tests/recordings/corpus-stub.twrec`) is replayed whole and then perturbed three ways, each change caught where it
//! was made; `corpus.twrec` up to where its recording's release order parts from the C++ the port follows; with
//! `TWIDGET_RECORDINGS` set to a directory, every recording under it (`tools/record/record_corpus.py`'s corpus, and
//! the `core.twrec` and `bench.twrec` `twidget_core_record` and `twidget_bench_record` make).

use std::path::{Path, PathBuf};

use twidget_json::Json;
use twidget_replay::recording::{Kind, Reader, Step};
use twidget_replay::{Options, Perturb, Replay, replay_tree};

fn twidget() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn options(path: &Path, source: &str) -> Options {
    Options { recording: path.to_path_buf(), source: source.to_string(), ..Options::default() }
}

fn run(o: Options) -> (i32, Replay) {
    let mut replay = Replay::new(o);
    let code = replay.run();
    (code, replay)
}

fn shown(replay: &Replay) -> String {
    replay.lines.iter().skip(1).cloned().collect::<Vec<_>>().join("\n")
}

/// What `twidget_parity_corpus_stub` needs of it, as CMakeLists.txt asks the C++ replay.
const STUB_NEEDS: &str = "sessions,steps,lists,batches,calls,pointers,claims,holds,ticks,engine,resizes,themes,images,keys";
const STUB_SESSIONS: u64 = 7;

#[test]
fn committed_stub_corpus() {
    let path = twidget().join("tests/recordings/corpus-stub.twrec");
    let mut o = options(&path, "corpus-stub");
    o.needs = STUB_NEEDS.split(',').map(str::to_string).collect();
    o.sessions = Some(STUB_SESSIONS);
    let (code, replay) = run(o);
    assert_eq!(code, 0, "{}", shown(&replay));
    println!("{}", shown(&replay));
    for perturb in [Perturb::List, Perturb::Call, Perturb::Key] {
        let mut o = options(&path, "corpus-stub");
        o.perturb = perturb;
        let (code, replay) = run(o);
        assert_eq!(code, 0, "a perturbed {perturb:?} is caught where it was changed: {}", shown(&replay));
        println!("{}", replay.lines.last().cloned().unwrap_or_default());
    }
}

#[test]
fn benchmark_reports_only_verified_steps_and_first_contentful_paint() {
    let path = twidget().join("tests/recordings/ordinary/article-card.twrec");
    let mut options = options(&path, "article-card");
    options.benchmark = true;
    let (code, replay) = run(options.clone());
    assert_eq!(code, 0, "{}", shown(&replay));
    let report = Json::parse(replay.lines.last().unwrap()).unwrap();
    let sessions = report["sessions"].arr();
    assert_eq!(sessions.len(), 1);
    let session = &sessions[0];
    let mut reader = Reader::open(&path);
    let mut step = Step::default();
    let mut bytes = 0;
    let mut steps = Vec::new();
    let mut frames = Vec::new();
    let mut first = None;
    while reader.next(&mut step, &mut bytes) {
        steps.push(step.kind.name());
        if step.kind == Kind::Frame {
            frames.push(Json::Array(vec![steps.len().into(), step.painted.into(), step.list.len().into()]));
            if step.painted && !step.list.is_empty() {
                first.get_or_insert(steps.len());
            }
        }
    }
    assert!(reader.ended());
    let samples = session["steps"].arr();
    assert_eq!(samples.len(), steps.len());
    for (sample, kind) in samples.iter().zip(steps) {
        assert_eq!(sample.at(0).as_str(), kind.as_bytes());
        assert!(sample.at(1).as_number(-1.0) >= 0.0);
    }
    let first = first.expect("the article's first contentful paint");
    let first_ns: f64 = samples[..first].iter().map(|s| s.at(1).as_number(-1.0)).sum();
    assert_eq!(session["first_paint_step"], first.into());
    assert_eq!(session["first_paint_ns"], first_ns.into());
    assert_eq!(session["frames"].arr(), frames);
    options.sessions = Some(2);
    let (code, rejected) = run(options);
    assert_ne!(code, 0);
    assert!(!rejected.lines.iter().any(|s| Json::parse(s).is_some()));
}

/// corpus.twrec was recorded on macOS at twidget 5bc9639, before stage 0.3 made `~Renderer` release a canvas's fonts in
/// the order they were made: libc++'s hash order released canvas font 6 before 5. The C++ since then and the port
/// release 5 first, so the replay matches every step up to the first widget's destruction (step 132) and parts there,
/// as the C++ replay does on Windows. Past it a second difference of the time waits: at step 343 a 160deg gradient's
/// end point is one ulp off, where clang on arm64 fused `y - m * x` before 0.3 turned contraction off. Once the owner
/// records it again on macOS, it must match whole.
#[test]
fn committed_corpus_twrec() {
    let path = twidget().join("tests/recordings/corpus.twrec");
    let mut o = options(&path, "corpus");
    let (code, replay) = run(o.clone());
    let origin = replay.lines.first().cloned().unwrap_or_default();
    if !origin.contains(r#""twidget":"5bc9639-dirty""#) {
        o.needs = "sessions,steps,lists,batches,calls,pointers,claims,holds,ticks,engine,resizes,themes,images"
            .split(',')
            .map(str::to_string)
            .collect();
        o.sessions = Some(4);
        let (code, replay) = run(o);
        assert_eq!(code, 0, "a corpus.twrec recorded again matches whole: {}", shown(&replay));
        return;
    }
    assert_eq!(code, 1, "{}", shown(&replay));
    let (step, failures) = replay.failure.clone().expect("a step that differs");
    assert_eq!(step, 132, "the first difference is the first widget's destruction: {}", shown(&replay));
    assert_eq!(
        failures,
        vec!["the Backend calls are the direct renderer's, in order call[1].font: 0x6 against 0x5".to_string()],
        "and only its canvas fonts' release order"
    );
    assert_eq!(replay.counts().steps, 131, "every step before it matches");
}

#[test]
fn recorded_corpus() {
    let Some(dir) = std::env::var_os("TWIDGET_RECORDINGS") else {
        println!("TWIDGET_RECORDINGS is not set: the recorded corpus is not replayed");
        return;
    };
    let dir = PathBuf::from(dir);
    let tree = replay_tree(&dir, |source, code, replay| {
        if code != 0 {
            println!("{source}: exit {code}\n{}", shown(replay));
        }
    });
    for (folder, (recordings, counts)) in &tree.folders {
        println!("{folder}: {recordings} recordings; {}", counts.text());
    }
    let total = tree.total();
    println!("{} recordings, {} failed; {}; {} renderer messages", tree.recordings, tree.failed.len(), total.text(), total.returned);
    assert!(tree.recordings > 0, "{} holds no recording", dir.display());
    assert!(tree.failed.is_empty(), "{} recordings differ: {:?}", tree.failed.len(), tree.failed);
}
