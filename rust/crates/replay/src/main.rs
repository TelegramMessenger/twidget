//! twidget_replay on the Rust renderer end, as the C++ program takes its arguments; a directory in place of the
//! recording replays every `.twrec` under it, each alone, and prints the counts by its first folder.

#![forbid(unsafe_code)]

use std::path::Path;

use twidget_replay::{Options, Replay, replay_tree};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [dir] = args.as_slice()
        && Path::new(dir).is_dir()
    {
        let tree = replay_tree(Path::new(dir), |source, code, replay| {
            if code == 0 {
                println!("{source}: {}", replay.counts().text());
            } else {
                for line in replay.lines.iter().skip(1) {
                    println!("{line}");
                }
            }
        });
        for (folder, (recordings, counts)) in &tree.folders {
            println!("{folder}: {recordings} recordings; {}", counts.text());
        }
        println!("{} recordings, {} failed; all: {}", tree.recordings, tree.failed.len(), tree.total().text());
        std::process::exit(i32::from(!tree.failed.is_empty()));
    }
    let Some(options) = Options::parse(&args) else {
        eprintln!(
            "usage: twidget_replay <recording> --source NAME [--needs a,b,...] [--sessions N] [--message-bytes N] [--perturb list|call|key]\n\
             \x20      twidget_replay <directory of recordings>\n\
             needs: sessions steps lists batches reads calls pointers claims holds selections ticks engine resizes themes images keys"
        );
        std::process::exit(2);
    };
    let mut replay = Replay::new(options);
    replay.echo = true;
    std::process::exit(replay.run());
}
