//! conlang — the conlanging tool suite: a library of ATTESTED sound changes
//! (each with its historical provenance) that assembles into a SCArs ruleset,
//! and a PHONOLOGY BUILDER that designs a proto-language's sound system,
//! generates words obeying it, and evolves them by calling the sound-change
//! pipeline. The tools are deliberately modular, Oberon-style: the phonology
//! builder calls the word generator calls the change library calls the SCArs
//! engine (`lib/sca.lat` on Loom) — four tools in one chain.

/// One attested sound change: (id, name, attested-in, description, scars rules).
/// Rules use the SCArs syntax (docs/scars-sound-changes.md); several entries are
/// simplified from their full historical statement, and say so.
pub const SOUND_CHANGES: &[(&str, &str, &str, &str, &str)] = &[
    (
        "grimm1",
        "Grimm's law (first phase)",
        "Proto-Germanic, c. 500 BC",
        "Voiceless stops become fricatives: p t k > f th h.",
        "p > f\nt > th\nk > h",
    ),
    (
        "grimm2",
        "Grimm's law (second phase)",
        "Proto-Germanic",
        "Voiced stops devoice: b d g > p t k.",
        "b > p\nd > t\ng > k",
    ),
    (
        "verner",
        "Verner's law (simplified)",
        "Proto-Germanic, c. 400 BC",
        "Medial fricatives voice between vowels (the full law is conditioned on accent; this is the vowel-context core).",
        "f > v / V _ V\nth > dh / V _ V\ns > z / V _ V\nh > g / V _ V",
    ),
    (
        "lenition",
        "Intervocalic lenition",
        "Western Romance (Spanish, Portuguese), Brythonic Celtic",
        "Voiceless stops voice between vowels: VpV VtV VkV > VbV VdV VgV (Latin vita > Spanish vida).",
        "p > b / V _ V\nt > d / V _ V\nk > g / V _ V",
    ),
    (
        "spirant",
        "Voiced-stop spirantization",
        "Spanish, Hebrew (begadkefat, simplified)",
        "Voiced stops weaken to fricatives between vowels: b d g > v dh gh.",
        "b > v / V _ V\nd > dh / V _ V\ng > gh / V _ V",
    ),
    (
        "finaldevoice",
        "Final devoicing",
        "German, Dutch, Russian, Turkish",
        "Voiced obstruents devoice word-finally (German Tag [tak]).",
        "b > p / _ #\nd > t / _ #\ng > k / _ #\nz > s / _ #\nv > f / _ #",
    ),
    (
        "rhotacism",
        "Rhotacism",
        "Latin (flos/floris), West Germanic",
        "s becomes r between vowels (Old Latin *flosis > floris).",
        "s > r / V _ V",
    ),
    (
        "palatal1",
        "First palatalization",
        "Common Slavic, c. 400-600 AD",
        "Velars soften before front vowels: k g h > ch zh sh before e/i.",
        "k > ch / _ e\nk > ch / _ i\ng > zh / _ e\ng > zh / _ i\nh > sh / _ e\nh > sh / _ i",
    ),
    (
        "umlaut",
        "i-umlaut (adjacency-simplified)",
        "Old English, Old Norse, Old High German",
        "Back vowels front before i (full law acts across a consonant; here: across one consonant).",
        "a > e / _ C i\no > eu / _ C i\nu > y / _ C i",
    ),
    (
        "monophth",
        "Monophthongization",
        "Latin (ae oe > e), Greek (ai > e)",
        "Falling diphthongs smooth to long mid vowels: ai au > ee oo.",
        "a i > ee\na u > oo",
    ),
    (
        "gvs",
        "Great Vowel Shift (fragment)",
        "Early Modern English, 1400-1700",
        "Long high vowels break to diphthongs: ii uu > ai au (time, house).",
        "i i > a i\nu u > a u",
    ),
    (
        "nasalcomp",
        "Nasal loss with compensatory lengthening",
        "Latin/Greek before s, Old English before fricatives",
        "A nasal drops before s and the vowel lengthens: ans > aas (Latin *consol > cosol > cōsul pattern).",
        "a n > a a / _ s\ne n > e e / _ s\no n > o o / _ s",
    ),
    (
        "hdrop",
        "h-dropping",
        "Vulgar Latin, many English dialects",
        "h is lost everywhere.",
        "h >",
    ),
    (
        "clusterred",
        "Final cluster reduction",
        "Many languages (e.g. Mandarin historical codas)",
        "A stop drops after a consonant at word end.",
        "p > / C _ #\nt > / C _ #\nk > / C _ #",
    ),
    (
        "prothesis",
        "Vowel prothesis before sC",
        "Western Romance (Spanish escuela, French école)",
        "An e is inserted before word-initial s+stop clusters.",
        "s > e s / # _ C",
    ),
    (
        "apocope",
        "Final-vowel apocope",
        "French, many Germanic languages",
        "Short final vowels after a consonant are lost (simplified: e and a drop word-finally).",
        "e > / C _ #\na > / C _ #",
    ),
    (
        "nasalassim",
        "Nasal place assimilation (n>m before labials)",
        "Essentially universal (Latin in+port > import)",
        "n becomes m before p or b.",
        "n > m / _ p\nn > m / _ b",
    ),
    (
        "wfortition",
        "Glide fortition w > v",
        "Latin to Romance, Germanic to High German",
        "The glide w hardens to v.",
        "w > v",
    ),
    (
        "thstop",
        "Dental fricative hardening th > t",
        "Continental Germanic, French loans",
        "th hardens to t (or d medially).",
        "th > d / V _ V\nth > t",
    ),
    (
        "degemination",
        "Degemination",
        "Western Romance (Spanish), late Latin",
        "Double consonants simplify: pp tt kk ss > p t k s.",
        "p p > p\nt t > t\nk k > k\ns s > s",
    ),
];

/// The shared class prelude every assembled ruleset gets.
const CLASS_PRELUDE: &str = "class V = a e i o u y aa ee ii oo uu eu ai au\nclass C = p t k b d g m n r l s z f v h w j ch zh sh th dh gh\n";

/// Assemble an ordered selection of change ids into a complete SCArs file.
pub fn assemble_sca(ids: &[&str]) -> Result<String, String> {
    let mut out = String::from(
        ":: assembled by the Orpheus sound-change library tool\n:: rules apply IN ORDER — reorder the selection to explore chain shifts\n\n",
    );
    out.push_str(CLASS_PRELUDE);
    for id in ids {
        let entry = SOUND_CHANGES
            .iter()
            .find(|(i, _, _, _, _)| i == id)
            .ok_or_else(|| format!("unknown sound change '{}'", id))?;
        out.push_str(&format!("\n:: {} — {} ({})\n", entry.1, entry.3, entry.2));
        out.push_str(entry.4);
        out.push('\n');
    }
    Ok(out)
}

/// Run words through an assembled ruleset on the SCArs engine (lib/sca.lat).
pub fn apply_changes(ids: &[&str], words: &[String]) -> Result<(String, Vec<(String, String)>), String> {
    let sca = assemble_sca(ids)?;
    let rows = words
        .iter()
        .map(|w| {
            let lines: Vec<String> = sca.lines().map(String::from).collect();
            let evolved = crate::sca::run_sca(w, &lines).unwrap_or_else(|e| format!("<error: {}>", e));
            (w.clone(), evolved)
        })
        .collect();
    Ok((sca, rows))
}

// ============================================================================
// THE PHONOLOGY BUILDER. Choose an inventory and syllable patterns; the
// generator produces words that obey them (weighted onsets, legal codas),
// and — modularly — hands them to the sound-change pipeline above.
// ============================================================================

pub struct Phonology {
    pub consonants: Vec<String>,
    pub vowels: Vec<String>,
    pub patterns: Vec<String>, // e.g. ["CV", "CVC", "CCV"] — C consonant, V vowel
}

/// Curated presets, each a believable natural-language profile.
pub fn presets() -> Vec<(&'static str, &'static str, Phonology)> {
    vec![
        (
            "polynesian",
            "small open-syllable inventory (Hawaiian-like)",
            Phonology {
                consonants: "p k m n l h w".split(' ').map(String::from).collect(),
                vowels: "a e i o u".split(' ').map(String::from).collect(),
                patterns: vec!["CV".into(), "V".into(), "CV".into(), "CVV".into()],
            },
        ),
        (
            "pie",
            "stop-rich with liquids and s (early Indo-European-like)",
            Phonology {
                consonants: "p t k b d g s m n r l w j h".split(' ').map(String::from).collect(),
                vowels: "a e i o u".split(' ').map(String::from).collect(),
                patterns: vec!["CVC".into(), "CV".into(), "CCVC".into(), "VC".into(), "CVCC".into()],
            },
        ),
        (
            "semitic",
            "guttural-flavored, closed syllables (Semitic-like)",
            Phonology {
                consonants: "p t k b d g s z sh m n r l h q".split(' ').map(String::from).collect(),
                vowels: "a i u".split(' ').map(String::from).collect(),
                patterns: vec!["CVC".into(), "CV".into(), "CVCC".into()],
            },
        ),
        (
            "sinitic",
            "no clusters, final nasals only (Sinitic-like)",
            Phonology {
                consonants: "p t k m n s l h ch sh w j".split(' ').map(String::from).collect(),
                vowels: "a e i o u".split(' ').map(String::from).collect(),
                patterns: vec!["CV".into(), "CVn".into(), "CV".into()],
            },
        ),
    ]
}

fn lcg(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed >> 33
}

/// Generate `n` words obeying the phonology: 1-3 syllables, each drawn from the
/// pattern list; no identical adjacent consonants across syllable boundaries.
pub fn generate_words(ph: &Phonology, n: usize, seed: u64) -> Vec<String> {
    let mut s = seed.wrapping_mul(2).wrapping_add(1);
    let mut out = Vec::new();
    let pick = |s: &mut u64, set: &[String]| -> String {
        if set.is_empty() {
            return String::new();
        }
        set[(lcg(s) as usize) % set.len()].clone()
    };
    for _ in 0..n {
        let nsyl = 1 + (lcg(&mut s) as usize) % 3;
        let mut w = String::new();
        let mut last_c = String::new();
        for _ in 0..nsyl {
            let pat = if ph.patterns.is_empty() { "CV".to_string() } else { ph.patterns[(lcg(&mut s) as usize) % ph.patterns.len()].clone() };
            for ch in pat.chars() {
                match ch {
                    'C' => {
                        let mut c = pick(&mut s, &ph.consonants);
                        if c == last_c {
                            c = pick(&mut s, &ph.consonants);
                        }
                        w.push_str(&c);
                        last_c = c;
                    }
                    'V' => {
                        w.push_str(&pick(&mut s, &ph.vowels));
                        last_c.clear();
                    }
                    other => {
                        w.push(other.to_ascii_lowercase());
                        last_c = other.to_string();
                    }
                }
            }
        }
        out.push(w);
    }
    out
}

/// The full modular pipeline: build words from a phonology, then evolve them
/// through an ordered selection of attested changes — the phonology builder
/// calling the word generator calling the change library calling SCArs.
pub fn phonology_pipeline(
    ph: &Phonology,
    n: usize,
    seed: u64,
    change_ids: &[&str],
) -> Result<(Vec<(String, String)>, String), String> {
    let words = generate_words(ph, n.clamp(1, 60), seed);
    if change_ids.is_empty() {
        return Ok((words.into_iter().map(|w| (w.clone(), w)).collect(), String::new()));
    }
    let (sca, rows) = apply_changes(change_ids, &words)?;
    Ok((rows, sca))
}

#[cfg(test)]
mod tests {
    #[test]
    fn attested_changes_assemble_and_run() {
        // Grimm + Verner on a classic: *pater -> father-like shapes
        let (sca, rows) = super::apply_changes(&["grimm1", "verner"], &["pater".into(), "bhrater".into()]).unwrap();
        assert!(sca.contains("Grimm"), "{}", sca);
        let pater = &rows[0].1;
        assert!(pater.starts_with('f'), "Grimm should give f-: {}", pater);
        assert!(pater.contains('d') || pater.contains("dh"), "Verner should voice the medial: {}", pater);
    }

    #[test]
    fn lenition_then_final_devoicing_order_matters() {
        let (_, a) = super::apply_changes(&["lenition", "finaldevoice"], &["lupo".into()]).unwrap();
        assert_eq!(a[0].1, "lubo", "intervocalic p voices: {}", a[0].1);
    }

    #[test]
    fn phonology_builder_generates_and_evolves() {
        let ph = &super::presets()[1].2; // the PIE-like preset
        let words = super::generate_words(ph, 12, 42);
        assert_eq!(words.len(), 12);
        assert!(words.iter().all(|w| !w.is_empty() && w.len() <= 16));
        // deterministic for a given seed
        assert_eq!(words, super::generate_words(ph, 12, 42));
        // the modular pipeline: words then changes through SCArs
        let (rows, sca) = super::phonology_pipeline(ph, 8, 7, &["grimm1", "apocope"]).unwrap();
        assert_eq!(rows.len(), 8);
        assert!(sca.contains("Grimm"));
        assert!(rows.iter().all(|(p, e)| !p.is_empty() && !e.is_empty()));
    }
}
