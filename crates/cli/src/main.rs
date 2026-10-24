//! `pm`: a command-line demo of the procedural music theory core.

mod midi;

use std::process::ExitCode;

use pm_theory::{voicing, Chord, Meter, Note, Pitch, Progression, Rhythm, Scale, Tuning, PPQ};

const HELP: &str = "\
pm - procedural music theory demo

USAGE
  pm scale <name>                 e.g. pm scale \"Bb dorian\"
  pm chord <symbol>               e.g. pm chord F#m7b5
  pm edo <divisions> <steps>      e.g. pm edo 19 3,3,2,3,3,3,2
  pm rhythm <grid>                e.g. pm rhythm \"X..x..x.\"
  pm euclid <hits> <steps>        e.g. pm euclid 5 8
  pm prog [options] <chords>      e.g. pm prog --key \"C major\" \"ii7 V7 Imaj7 -\"

PROG OPTIONS
  --key <scale>     read chords as Roman numerals in this key (otherwise chord symbols)
  --bpm <n>         tempo for the MIDI file (default 100)
  --loops <n>       passes through the progression (default 2)
  --out <file.mid>  write a MIDI file: pads (ch 1), bass (ch 2), drums (ch 10)

In a progression, '-' holds the previous chord for another bar and '|' is ignored.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("scale") => cmd_scale(&args[1..]),
        Some("chord") => cmd_chord(&args[1..]),
        Some("edo") => cmd_edo(&args[1..]),
        Some("rhythm") => cmd_rhythm(&args[1..]),
        Some("euclid") => cmd_euclid(&args[1..]),
        Some("prog") => cmd_prog(&args[1..]),
        Some("help" | "--help" | "-h") | None => {
            print!("{HELP}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command '{other}' (try `pm help`)")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

type CmdResult = Result<(), String>;

fn arg<'a>(args: &'a [String], i: usize, what: &str) -> Result<&'a str, String> {
    args.get(i)
        .map(String::as_str)
        .ok_or_else(|| format!("missing {what}"))
}

fn cmd_scale(args: &[String]) -> CmdResult {
    let text = args.join(" ");
    let scale = Scale::parse(&text).ok_or_else(|| format!("unknown scale '{text}'"))?;
    println!("{scale}");
    let notes: Vec<String> = (0..=scale.len() as i32)
        .map(|d| scale.pitch(d, 4).to_string())
        .collect();
    println!("  notes      {}", notes.join(" "));
    let offsets = scale.degree_offsets();
    let steps: Vec<String> = offsets
        .iter()
        .enumerate()
        .map(|(i, &o)| (offsets.get(i + 1).copied().unwrap_or(12) - o).to_string())
        .collect();
    println!("  steps      {}", steps.join(" "));
    let iv: Vec<String> = scale
        .pc_set()
        .interval_vector()
        .iter()
        .map(u8::to_string)
        .collect();
    println!("  intervals  <{}>  (interval-class vector)", iv.join(""));
    if scale.len() == 7 {
        let triads: Vec<String> = (0..7)
            .map(|d| scale.chord_on_degree(d, 3, 4).to_string())
            .collect();
        let sevenths: Vec<String> = (0..7)
            .map(|d| scale.chord_on_degree(d, 4, 4).to_string())
            .collect();
        println!("  triads     {}", triads.join(" "));
        println!("  sevenths   {}", sevenths.join(" "));
        let modes: Vec<String> = (1..7).map(|d| scale.mode(d).to_string()).collect();
        println!("  modes      {}", modes.join(", "));
    }
    Ok(())
}

fn cmd_chord(args: &[String]) -> CmdResult {
    let symbol = arg(args, 0, "chord symbol")?;
    let chord = Chord::from_symbol(symbol, 4).ok_or_else(|| format!("unknown chord '{symbol}'"))?;
    println!("{chord}");
    println!("  notes      {chord:#}");
    println!("  classes    {}", chord.pitch_classes());
    let intervals: Vec<String> = chord
        .notes()
        .iter()
        .map(|p| {
            (*p - chord.notes()[0])
                .simple()
                .short_name()
                .unwrap_or("?")
                .to_string()
        })
        .collect();
    println!("  intervals  {}", intervals.join(" "));
    for i in 1..chord.len().min(4) {
        let inv = chord.invert(i as i32);
        println!("  inversion {i}  {inv:#}  ({inv})");
    }
    println!("  drop 2     {:#}", voicing::drop2(&chord));
    Ok(())
}

fn cmd_edo(args: &[String]) -> CmdResult {
    let n: u8 = arg(args, 0, "number of divisions")?
        .parse()
        .map_err(|_| "divisions must be a number")?;
    let tuning = Tuning::edo(n).ok_or("divisions must be 1 to 64")?;
    let steps: Vec<u8> = arg(args, 1, "step pattern")?
        .split(',')
        .map(|s| s.trim().parse::<u8>())
        .collect::<Result<_, _>>()
        .map_err(|_| "steps must be comma-separated numbers")?;
    let scale = Scale::from_steps(tuning, 0, &steps)
        .ok_or(format!("steps must be positive and sum to {n}"))?;
    println!(
        "{n}-EDO scale, {} notes, step = {:.2} cents",
        scale.len(),
        tuning.cents(1)
    );
    println!(
        "  {:>6} {:>9} {:>10} {:>14}",
        "degree", "cents", "Hz", "nearest MIDI"
    );
    for d in 0..=scale.len() as i32 {
        let p = scale.pitch(d, 4);
        let (midi, off) = tuning.to_midi(p.step());
        println!(
            "  {:>6} {:>9.1} {:>10.2} {:>8} {:+5.0}c",
            d,
            tuning.cents(p.step() - tuning.c4_step()),
            p.frequency(),
            Pitch::midi(midi).to_string(),
            off
        );
    }
    Ok(())
}

fn cmd_rhythm(args: &[String]) -> CmdResult {
    let grid = args.join("");
    let r = Rhythm::from_grid(&grid, PPQ / 4).ok_or("rhythm grids use x X . - _ | and spaces")?;
    print_rhythm(&r);
    Ok(())
}

fn cmd_euclid(args: &[String]) -> CmdResult {
    let hits: u32 = arg(args, 0, "hits")?
        .parse()
        .map_err(|_| "hits must be a number")?;
    let steps: u32 = arg(args, 1, "steps")?
        .parse()
        .map_err(|_| "steps must be a number")?;
    let r = Rhythm::euclidean(hits, steps, PPQ / 4).ok_or("need 0 <= hits <= steps, steps > 0")?;
    println!("E({hits},{steps})");
    print_rhythm(&r);
    for k in 1..steps.min(8) {
        println!("  rotate {k}   {}", r.rotate(-((k * PPQ / 4) as i64)));
    }
    Ok(())
}

fn print_rhythm(r: &Rhythm) {
    println!("  grid       {r}");
    println!(
        "  hits       {} over {} sixteenths",
        r.len(),
        r.length() / (PPQ / 4)
    );
}

struct ProgOptions {
    key: Option<Scale>,
    bpm: f64,
    loops: u32,
    out: Option<String>,
    chords: String,
}

fn parse_prog_options(args: &[String]) -> Result<ProgOptions, String> {
    let mut opts = ProgOptions {
        key: None,
        bpm: 100.0,
        loops: 2,
        out: None,
        chords: String::new(),
    };
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--key" => {
                let text = arg(args, i + 1, "key")?;
                opts.key = Some(Scale::parse(text).ok_or_else(|| format!("unknown key '{text}'"))?);
                i += 2;
            }
            "--bpm" => {
                opts.bpm = arg(args, i + 1, "bpm")?
                    .parse()
                    .map_err(|_| "bpm must be a number")?;
                i += 2;
            }
            "--loops" => {
                opts.loops = arg(args, i + 1, "loops")?
                    .parse()
                    .map_err(|_| "loops must be a number")?;
                i += 2;
            }
            "--out" => {
                opts.out = Some(arg(args, i + 1, "output file")?.to_string());
                i += 2;
            }
            other => {
                rest.push(other.to_string());
                i += 1;
            }
        }
    }
    opts.chords = rest.join(" ");
    if opts.chords.trim().is_empty() {
        return Err("missing chords".into());
    }
    Ok(opts)
}

fn cmd_prog(args: &[String]) -> CmdResult {
    let opts = parse_prog_options(args)?;
    let meter = Meter::COMMON;
    let bar = meter.ticks_per_bar();
    let prog = match &opts.key {
        Some(key) => Progression::from_roman(&opts.chords, key, bar, 4),
        None => Progression::from_symbols(&opts.chords, bar, 4),
    }
    .ok_or("could not read the progression")?;
    if prog.is_empty() {
        return Err("the progression has no chords".into());
    }

    // Pads: lead each chord from the previous one, within E3..G5.
    let low = Pitch::parse("E3").unwrap();
    let high = Pitch::parse("G5").unwrap();
    let pads = prog.map_with_previous(|prev, chord| match prev {
        Some(p) => voicing::lead(p, chord, low, high),
        None => voicing::close(chord, Pitch::parse("C4").unwrap()),
    });

    if let Some(key) = &opts.key {
        println!("Key: {key}");
    }
    println!("{:<6} {:<10} {:<24} motion", "bar", "chord", "pad voicing");
    let mut start_bar = 1;
    let mut prev: Option<&Chord> = None;
    for step in pads.steps() {
        let motion = match prev {
            Some(p) => p
                .notes()
                .iter()
                .zip(step.chord.notes())
                .map(|(a, b)| (b.step() - a.step()).to_string())
                .collect::<Vec<_>>()
                .join(" "),
            None => String::from("-"),
        };
        println!(
            "{:<6} {:<10} {:<24} {}",
            start_bar,
            step.chord.to_string(),
            format!("{:#}", step.chord),
            motion
        );
        start_bar += step.duration / bar;
        prev = Some(&step.chord);
    }

    if let Some(path) = &opts.out {
        let bytes = render_midi(&pads, opts.loops.max(1), opts.bpm);
        std::fs::write(path, bytes).map_err(|e| format!("could not write {path}: {e}"))?;
        println!(
            "\nWrote {path} ({} loops at {} bpm)",
            opts.loops.max(1),
            opts.bpm
        );
    }
    Ok(())
}

/// Pads, a tresillo bassline on the chord roots, and a simple beat.
fn render_midi(pads: &Progression, loops: u32, bpm: f64) -> Vec<u8> {
    let total = pads.length() * loops;
    let mut tempo = midi::Track::new("Tempo");
    tempo.tempo(0, bpm);
    let mut pad_track = midi::Track::new("Pads");
    let mut bass_track = midi::Track::new("Bass");
    let mut drum_track = midi::Track::new("Drums");

    for pass in 0..loops {
        let mut start = pass * pads.length();
        for step in pads.steps() {
            for (v, &pitch) in step.chord.notes().iter().enumerate() {
                let note = Note {
                    start,
                    duration: step.duration,
                    pitch,
                    velocity: 70,
                    voice: v as u8,
                };
                pad_track.note(0, &note);
            }
            start += step.duration;
        }
    }

    let eighth = PPQ / 2;
    let bass_rhythm = Rhythm::from_grid("x..x..x.", eighth).expect("valid grid");
    bass_rhythm.for_each_in(0, total, |t, onset| {
        if let Some((_, _, step)) = pads.at(t) {
            let root = step
                .chord
                .root()
                .unwrap_or_else(|| step.chord.bass().unwrap().pitch_class());
            let pitch = Pitch::from_octave(step.chord.tuning(), 2, root);
            let note = Note {
                start: t,
                duration: onset.duration * 3 / 4,
                pitch,
                velocity: onset.velocity,
                voice: 0,
            };
            bass_track.note(1, &note);
        }
    });

    let sixteenth = PPQ / 4;
    let parts: [(&str, i32); 3] = [
        ("X...x...X...x...", 36), // kick
        ("....X.......X...", 38), // snare
        ("x.x.x.x.x.x.x.X.", 42), // closed hat
    ];
    for (grid, key) in parts {
        let r = Rhythm::from_grid(grid, sixteenth)
            .expect("valid grid")
            .with_gate(50);
        r.for_each_in(0, total, |t, onset| {
            let note = Note {
                start: t,
                duration: onset.duration,
                pitch: Pitch::midi(key),
                velocity: onset.velocity,
                voice: 0,
            };
            drum_track.note(9, &note);
        });
    }
    midi::file(&[tempo, pad_track, bass_track, drum_track])
}
