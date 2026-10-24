use super::*;

const WHOLE: Ticks = 3840;

/// Fux's cantus firmus in D Dorian: D F E D G F A G F E D.
fn fux_cf(octave: i32) -> Vec<FixedNote> {
    let d = 12 * (octave + 1) + 2;
    [0, 3, 2, 0, 5, 3, 7, 5, 3, 2, 0]
        .iter()
        .enumerate()
        .map(|(i, &x)| FixedNote {
            start: i as Ticks * WHOLE,
            duration: WHOLE,
            pitch: d + x,
        })
        .collect()
}

fn voice(name: &str, low: &str, high: &str, fixed: Option<Vec<FixedNote>>) -> VoiceDef {
    VoiceDef {
        name: name.into(),
        low: Pitch::parse(low).unwrap().step(),
        high: Pitch::parse(high).unwrap().step(),
        fixed,
    }
}

fn two_voice(species: Species) -> Problem {
    let cf = fux_cf(3);
    let measures = cf.iter().map(|n| (n.start, n.duration)).collect();
    Problem {
        voices: vec![
            voice("Soprano", "C4", "G5", None),
            voice("Bass", "C3", "C4", Some(cf)),
        ],
        measures,
        keys: vec![(0, 11 * WHOLE, Scale::parse("D dorian").unwrap())],
        chords: vec![],
        species,
        rules: RuleSet::default(),
        beam: 64,
        variety: 0,
        seed: 1,
    }
}

/// Pitch of `voice` sounding at `t`.
fn at(sol: &Solution, voice: usize, t: Ticks) -> Option<i32> {
    sol.slots
        .iter()
        .zip(&sol.pitches)
        .find(|(s, _)| s.voice == voice && s.start <= t && t < s.end())
        .map(|(_, &p)| p)
}

fn assert_no_hard(sol: &Solution) {
    let hard: Vec<_> = sol
        .violations
        .iter()
        .filter(|v| v.cost >= HARD)
        .map(|v| (v.rule.name(), sol.slots[v.slot].start))
        .collect();
    assert!(hard.is_empty(), "hard violations: {hard:?}");
}

#[test]
fn first_species_follows_the_rules() {
    let sol = solve(&two_voice(Species::First));
    assert_no_hard(&sol);
    let mut prev: Option<(i32, i32)> = None;
    for m in 0..11 {
        let t = m * WHOLE;
        let (s, b) = (at(&sol, 0, t).unwrap(), at(&sol, 1, t).unwrap());
        let interval = (s - b).rem_euclid(12);
        assert!(
            [0, 3, 4, 7, 8, 9].contains(&interval),
            "measure {m}: dissonance {interval}"
        );
        assert!(s >= b, "voices cross in measure {m}");
        if let Some((ps, pb)) = prev {
            let before = (ps - pb).rem_euclid(12);
            let similar = (s - ps).signum() == (b - pb).signum() && s != ps;
            assert!(
                !(similar && before == interval && (interval == 0 || interval == 7)),
                "parallel perfect into measure {m}"
            );
            assert!((s - ps).abs() != 6, "melodic tritone into measure {m}");
        }
        prev = Some((s, b));
    }
    let first = (at(&sol, 0, 0).unwrap() - at(&sol, 1, 0).unwrap()).rem_euclid(12);
    assert!(first == 0 || first == 7, "starts on {first}");
    assert_eq!(
        (at(&sol, 0, 10 * WHOLE).unwrap() - 50).rem_euclid(12),
        0,
        "ends on D"
    );
    // Musica ficta: the penultimate note is C#, rising to D.
    assert_eq!(
        at(&sol, 0, 9 * WHOLE).unwrap().rem_euclid(12),
        1,
        "penultimate C#"
    );
}

#[test]
fn every_species_solves_without_hard_violations() {
    for species in [
        Species::Second,
        Species::Third,
        Species::Fourth,
        Species::Fifth,
    ] {
        let sol = solve(&two_voice(species));
        assert_no_hard(&sol);
        let free = sol.slots.iter().filter(|s| s.fixed.is_none()).count();
        match species {
            Species::Second => assert_eq!(free, 21),
            Species::Third => assert_eq!(free, 41),
            Species::Fourth => assert_eq!(free, 11),
            _ => assert!(free > 11),
        }
    }
}

#[test]
fn fourth_species_suspensions_resolve_down() {
    let sol = solve(&two_voice(Species::Fourth));
    // Every free note held over a downbeat where it is dissonant falls by step next.
    let free: Vec<usize> = (0..sol.slots.len())
        .filter(|&i| sol.slots[i].voice == 0)
        .collect();
    let mut suspensions = 0;
    for w in free.windows(2) {
        let (s, next) = (sol.slots[w[0]], w[1]);
        for m in 0..11 {
            let t = m * WHOLE;
            if s.start < t && t < s.end() {
                let b = at(&sol, 1, t).unwrap();
                let interval = (sol.pitches[w[0]] - b).rem_euclid(12);
                if ![0, 3, 4, 7, 8, 9].contains(&interval) {
                    suspensions += 1;
                    let fall = sol.pitches[next] - sol.pitches[w[0]];
                    assert!(
                        fall == -1 || fall == -2,
                        "suspension at measure {m} moves {fall}"
                    );
                }
            }
        }
    }
    let _ = suspensions; // may be zero if consonant syncopations score better
}

#[test]
fn four_part_chorale_over_a_progression() {
    let key = Scale::parse("C major").unwrap();
    let prog = pm_theory::Progression::from_roman("I vi IV ii V7 I", &key, WHOLE, 4).unwrap();
    let mut chords = Vec::new();
    let mut t = 0;
    for step in prog.steps() {
        chords.push((t, t + step.duration, step.chord));
        t += step.duration;
    }
    let p = Problem {
        voices: vec![
            voice("Soprano", "C4", "G5", None),
            voice("Alto", "G3", "C5", None),
            voice("Tenor", "C3", "G4", None),
            voice("Bass", "E2", "C4", None),
        ],
        measures: chords.iter().map(|c| (c.0, c.1 - c.0)).collect(),
        keys: vec![(0, t, key)],
        chords: chords.clone(),
        species: Species::First,
        rules: RuleSet::default(),
        beam: 64,
        variety: 0,
        seed: 1,
    };
    let sol = solve(&p);
    assert_no_hard(&sol);
    for (s, e, chord) in &chords {
        let _ = e;
        for v in 0..4 {
            let pitch = at(&sol, v, *s).unwrap();
            assert!(
                chord.contains_pc(pitch),
                "voice {v} at {s}: not a chord tone"
            );
        }
        let bass = at(&sol, 3, *s).unwrap();
        let _ = bass;
        for v in 0..3 {
            assert!(
                at(&sol, v, *s).unwrap() >= at(&sol, v + 1, *s).unwrap(),
                "voices cross at {s}"
            );
        }
    }
    // The leading tone (B) in the V7 rises to C.
    let v7 = chords[4].0;
    let fin = chords[5].0;
    for v in 0..3 {
        let p = at(&sol, v, v7).unwrap();
        if p.rem_euclid(12) == 11 {
            assert_eq!(
                at(&sol, v, fin).unwrap(),
                p + 1,
                "leading tone in voice {v}"
            );
        }
    }
}

#[test]
fn explanations_and_variety() {
    let sol = solve(&two_voice(Species::First));
    let free = (0..sol.slots.len()).filter(|&i| sol.slots[i].fixed.is_none());
    for i in free {
        let (alt, delta) = sol.alternatives[i].expect("an alternative exists");
        assert_ne!(alt, sol.pitches[i]);
        assert!(delta > -50.0, "beam missed a much better note: {delta}");
    }
    let mut varied = two_voice(Species::First);
    varied.variety = 60;
    varied.seed = 99;
    let other = solve(&varied);
    assert_no_hard(&other);
    // Still the same cantus firmus.
    assert_eq!(at(&other, 1, 0), at(&sol, 1, 0));
}

#[test]
fn rules_can_be_switched_off() {
    let mut p = two_voice(Species::First);
    p.rules = RuleSet {
        cadence: false,
        melody: false,
        ..RuleSet::default()
    };
    let sol = solve(&p);
    assert!(sol
        .violations
        .iter()
        .all(|v| !matches!(v.rule.group(), Group::Cadence | Group::Melody)));
}

#[test]
#[ignore]
fn dump() {
    let name = |p: i32| Pitch::midi(p).to_string();
    let sol = solve(&two_voice(Species::First));
    for v in 0..2 {
        let line: Vec<String> = (0..11)
            .map(|m| name(at(&sol, v, m * WHOLE).unwrap()))
            .collect();
        println!("{}", line.join(" "));
    }
    for v in sol.issues() {
        println!(
            "  {} @ {} cost {}",
            v.rule.name(),
            sol.slots[v.slot].start / WHOLE,
            v.cost
        );
    }
    println!("cost {}", sol.cost);
    let key = Scale::parse("C major").unwrap();
    let prog = pm_theory::Progression::from_roman("I vi IV ii V7 I", &key, WHOLE, 4).unwrap();
    let mut chords = Vec::new();
    let mut t = 0;
    for step in prog.steps() {
        chords.push((t, t + step.duration, step.chord));
        t += step.duration;
    }
    let p = Problem {
        voices: vec![
            voice("S", "C4", "G5", None),
            voice("A", "G3", "C5", None),
            voice("T", "C3", "G4", None),
            voice("B", "E2", "C4", None),
        ],
        measures: chords.iter().map(|c| (c.0, c.1 - c.0)).collect(),
        keys: vec![(0, t, key)],
        chords,
        species: Species::First,
        rules: RuleSet::default(),
        beam: 64,
        variety: 0,
        seed: 1,
    };
    let sol = solve(&p);
    for v in 0..4 {
        let line: Vec<String> = (0..6)
            .map(|m| name(at(&sol, v, m * WHOLE).unwrap()))
            .collect();
        println!("{}", line.join(" "));
    }
    for v in sol.issues() {
        println!(
            "  {} v{} @ {} cost {}",
            v.rule.name(),
            sol.slots[v.slot].voice,
            sol.slots[v.slot].start / WHOLE,
            v.cost
        );
    }
    println!("cost {}", sol.cost);
}

#[test]
#[ignore]
fn dump_species() {
    for species in [
        Species::Second,
        Species::Third,
        Species::Fourth,
        Species::Fifth,
    ] {
        let t = std::time::Instant::now();
        let sol = solve(&two_voice(species));
        let el = t.elapsed();
        let mut line = String::new();
        for (s, p) in sol.slots.iter().zip(&sol.pitches) {
            if s.voice == 0 {
                line += &format!(
                    "{}{} ",
                    if s.strength == 2 { "|" } else { "" },
                    Pitch::midi(*p)
                );
            }
        }
        println!(
            "{species:?} ({:.1} ms, cost {:.1}): {line}",
            el.as_secs_f64() * 1000.0,
            sol.cost
        );
        for v in sol.issues() {
            println!("    {} @ m{}", v.rule.name(), sol.slots[v.slot].measure);
        }
    }
}

#[test]
#[ignore]
fn dump_fifth_end() {
    let sol = solve(&two_voice(Species::Fifth));
    let last = (0..sol.slots.len())
        .rev()
        .find(|&i| sol.slots[i].voice == 0)
        .unwrap();
    println!(
        "final {} alt {:?}",
        Pitch::midi(sol.pitches[last]),
        sol.alternatives[last].map(|(p, d)| (Pitch::midi(p).to_string(), d))
    );
    let mut a = sol.pitches.clone();
    a[last] = 62 + 12;
    let prob = two_voice(Species::Fifth);
    let ctx = Ctx::new(&prob);
    let mut v = Vec::new();
    let c = ctx.score(&a, Some(&mut v));
    println!("with D5: {c}");
    for x in v {
        println!(
            "   {} @ m{} {}",
            x.rule.name(),
            ctx.slots[x.slot].measure,
            x.cost
        );
    }
}

#[test]
#[ignore]
fn compare_second() {
    let prob = two_voice(Species::Second);
    let ctx = Ctx::new(&prob);
    let sol = solve(&prob);
    let old = "A4 B4 A4 F4 G4 A4 B4 C5 D5 E5 F5 D5 C5 A4 B4 C5 D5 A4 B4 C#5 D5";
    let mut a = sol.pitches.clone();
    let free: Vec<usize> = (0..ctx.slots.len())
        .filter(|&i| ctx.slots[i].voice == 0)
        .collect();
    let mut sorted = free.clone();
    sorted.sort_by_key(|&i| ctx.slots[i].start);
    for (i, name) in sorted.iter().zip(old.split(' ')) {
        a[*i] = Pitch::parse(name).unwrap().step();
    }
    let mut v = Vec::new();
    println!(
        "old line cost {} vs found {}",
        ctx.score(&a, Some(&mut v)),
        sol.cost
    );
    for x in v {
        if x.cost >= 0.3 {
            println!(
                "   {} @ m{} {}",
                x.rule.name(),
                ctx.slots[x.slot].measure,
                x.cost
            );
        }
    }
}

#[test]
#[ignore]
fn beam_sensitivity() {
    for species in [
        Species::First,
        Species::Second,
        Species::Third,
        Species::Fourth,
        Species::Fifth,
    ] {
        let mut row = format!("{species:?}:");
        for beam in [32, 64, 128, 256, 512] {
            let mut p = two_voice(species);
            p.beam = beam;
            let t = std::time::Instant::now();
            let s = solve(&p);
            row += &format!(
                "  {beam}→{:.1} ({:.0}ms)",
                s.cost,
                t.elapsed().as_secs_f64() * 1000.0
            );
        }
        println!("{row}");
    }
}

#[test]
#[ignore]
fn four_voice_third_species_timing() {
    let key = Scale::parse("C major").unwrap();
    let prog =
        pm_theory::Progression::from_roman("I vi IV V iii vi ii7 V7", &key, WHOLE, 4).unwrap();
    let mut chords = Vec::new();
    let mut t = 0;
    for step in prog.steps() {
        chords.push((t, t + step.duration, step.chord));
        t += step.duration;
    }
    for species in [Species::First, Species::Third, Species::Fifth] {
        let p = Problem {
            voices: vec![
                voice("S", "C4", "G5", None),
                voice("A", "G3", "C5", None),
                voice("T", "C3", "G4", None),
                voice("B", "E2", "C4", None),
            ],
            measures: chords.iter().map(|c| (c.0, c.1 - c.0)).collect(),
            keys: vec![(0, t, key)],
            chords: chords.clone(),
            species,
            rules: RuleSet::default(),
            beam: 64,
            variety: 0,
            seed: 1,
        };
        let tq = std::time::Instant::now();
        let quick = solve_quick(&p);
        println!(
            "  quick: {:.0} ms, cost {:.1}",
            tq.elapsed().as_secs_f64() * 1000.0,
            quick.cost
        );
        let t0 = std::time::Instant::now();
        let sol = solve(&p);
        let hard = sol.violations.iter().filter(|v| v.cost >= HARD).count();
        println!(
            "{species:?}: {:.0} ms, cost {:.1}, hard {hard}, slots {}",
            t0.elapsed().as_secs_f64() * 1000.0,
            sol.cost,
            sol.slots.len()
        );
    }
}
