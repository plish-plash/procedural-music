//! The rules of the solver, grouped so the user can switch groups off.

/// Cost of breaking a rule that should never be broken.
pub const HARD: f32 = 1000.0;

/// Rule groups, each switchable as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    /// Parallel and hidden perfect intervals.
    Parallels,
    /// Where dissonances may fall and how they must be approached and left.
    Dissonance,
    /// Leaps, recovery, repeated notes, awkward intervals, the melodic high point.
    Melody,
    /// Crossing, overlap and spacing between voices.
    Spacing,
    /// Leading tones rise, chordal sevenths fall.
    Tendency,
    /// Chord tones on strong beats when a progression is given.
    Harmony,
    /// How the piece begins and ends.
    Cadence,
}

/// Which rule groups are active.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RuleSet {
    pub parallels: bool,
    pub dissonance: bool,
    pub melody: bool,
    pub spacing: bool,
    pub tendency: bool,
    pub harmony: bool,
    pub cadence: bool,
}

impl Default for RuleSet {
    fn default() -> Self {
        RuleSet {
            parallels: true,
            dissonance: true,
            melody: true,
            spacing: true,
            tendency: true,
            harmony: true,
            cadence: true,
        }
    }
}

impl RuleSet {
    pub fn allows(&self, group: Group) -> bool {
        match group {
            Group::Parallels => self.parallels,
            Group::Dissonance => self.dissonance,
            Group::Melody => self.melody,
            Group::Spacing => self.spacing,
            Group::Tendency => self.tendency,
            Group::Harmony => self.harmony,
            Group::Cadence => self.cadence,
        }
    }
}

/// Every rule the solver checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rule {
    ParallelFifths,
    ParallelOctaves,
    AccentedParallels,
    HiddenPerfect,
    ParallelImperfectRun,
    StrongDissonance,
    DissonanceByLeap,
    UnresolvedDissonance,
    NeighborInSecondSpecies,
    UnresolvedSuspension,
    MelodicTritone,
    AugmentedSecond,
    ChromaticStep,
    RepeatedFigure,
    LeapTooLarge,
    LargeLeap,
    UnrecoveredLeap,
    LeapsSameDirection,
    RepeatedNote,
    Oscillation,
    RepeatedHighPoint,
    WideRange,
    VoiceCrossing,
    VoiceOverlap,
    WideSpacing,
    Unison,
    Tessitura,
    LeadingTone,
    SeventhResolution,
    NonChordTone,
    BassNotRoot,
    MissingThird,
    DoubledLeadingTone,
    ImperfectStart,
    ImperfectEnd,
    FinalNotTonic,
    FinalByLeap,
    NoLeadingTone,
    SimilarMotion,
    Variety,
}

impl Rule {
    pub fn group(self) -> Group {
        use Rule::*;
        match self {
            ParallelFifths | ParallelOctaves | AccentedParallels | HiddenPerfect
            | ParallelImperfectRun | SimilarMotion => Group::Parallels,
            StrongDissonance
            | DissonanceByLeap
            | UnresolvedDissonance
            | NeighborInSecondSpecies
            | UnresolvedSuspension => Group::Dissonance,
            MelodicTritone | AugmentedSecond | ChromaticStep | RepeatedFigure | LeapTooLarge
            | LargeLeap | UnrecoveredLeap | LeapsSameDirection | RepeatedNote | Oscillation
            | RepeatedHighPoint | WideRange | Variety => Group::Melody,
            VoiceCrossing | VoiceOverlap | WideSpacing | Unison | Tessitura => Group::Spacing,
            LeadingTone | SeventhResolution | DoubledLeadingTone => Group::Tendency,
            NonChordTone | BassNotRoot | MissingThird => Group::Harmony,
            ImperfectStart | ImperfectEnd | FinalNotTonic | FinalByLeap | NoLeadingTone => {
                Group::Cadence
            }
        }
    }

    /// Short name for the UI.
    pub fn name(self) -> &'static str {
        use Rule::*;
        match self {
            ParallelFifths => "Parallel fifths",
            ParallelOctaves => "Parallel octaves",
            AccentedParallels => "Parallels on strong beats",
            HiddenPerfect => "Hidden fifth or octave",
            ParallelImperfectRun => "Too many parallel 3rds/6ths",
            StrongDissonance => "Dissonance on a strong beat",
            DissonanceByLeap => "Dissonance entered by leap",
            UnresolvedDissonance => "Dissonance not left by step",
            NeighborInSecondSpecies => "Neighbor tone in 2nd species",
            UnresolvedSuspension => "Suspension not resolved down",
            MelodicTritone => "Melodic tritone",
            AugmentedSecond => "Augmented second",
            ChromaticStep => "Chromatic step",
            RepeatedFigure => "Figure repeats the last measure",
            LeapTooLarge => "Leap too large",
            LargeLeap => "Large leap",
            UnrecoveredLeap => "Leap not recovered",
            LeapsSameDirection => "Leaps in the same direction",
            RepeatedNote => "Repeated note",
            Oscillation => "Line circles back and forth",
            RepeatedHighPoint => "High point repeated",
            WideRange => "Line spans more than a tenth",
            VoiceCrossing => "Voices cross",
            VoiceOverlap => "Voices overlap",
            WideSpacing => "Voices too far apart",
            Unison => "Unison",
            Tessitura => "Far from the middle of the range",
            LeadingTone => "Leading tone doesn't rise",
            SeventhResolution => "Chordal 7th doesn't fall",
            NonChordTone => "Not a chord tone",
            BassNotRoot => "Bass not on the root",
            MissingThird => "Chord without its third",
            DoubledLeadingTone => "Leading tone doubled",
            ImperfectStart => "Doesn't begin on a perfect consonance",
            ImperfectEnd => "Doesn't end on a unison or octave",
            FinalNotTonic => "Final bass isn't the tonic",
            FinalByLeap => "Top voice leaps to the final",
            NoLeadingTone => "Cadence without a leading tone",
            SimilarMotion => "Similar motion",
            Variety => "Variety",
        }
    }

    /// Whether the rule counts as an issue worth showing (small preferences don't).
    pub fn is_shown(self) -> bool {
        !matches!(self, Rule::SimilarMotion | Rule::Variety | Rule::Tessitura)
    }
}
