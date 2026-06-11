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
    (
        "canaanite",
        "Canaanite shift",
        "Phoenician, Hebrew, c. 1400 BC",
        "Long a rounds and raises: aa > oo (Proto-Semitic *šalām > šalōm).",
        "a a > o o",
    ),
    (
        "hgcs",
        "High German consonant shift (core)",
        "Old High German, c. 500-800 AD",
        "Voiceless stops affricate initially and spirantize medially: p t k > pf ts kh / pen > Pfanne, water > Wasser.",
        "p > p f / # _\nt > t s / # _\np > f / V _ V\nt > s / V _ V\nk > h / V _ V",
    ),
    (
        "ktassim",
        "Stop-cluster assimilation kt > tt",
        "Italian (octo > otto), Western Romance",
        "The first stop of a cluster assimilates to the second.",
        "k t > t t\np t > t t",
    ),
    (
        "plpalatal",
        "Initial cluster palatalization pl/kl/fl > ll",
        "Spanish (plorare > llorar, clavis > llave, flamma > llama)",
        "Word-initial stop+l clusters merge into a palatal lateral (written ll).",
        "p l > l l / # _\nk l > l l / # _\nf l > l l / # _",
    ),
    (
        "aumono",
        "au > o monophthongization",
        "Vulgar Latin (aurum > oro), Old French",
        "The au diphthong smooths to o.",
        "a u > o",
    ),
    (
        "finalmloss",
        "Final m loss",
        "Vulgar Latin (the accusative -um endings)",
        "Word-final m after a vowel is lost.",
        "m > / V _ #",
    ),
    (
        "dloss",
        "Intervocalic d loss",
        "Colloquial Spanish (-ado > -ao), Danish",
        "d between vowels weakens away entirely.",
        "d > / V _ V",
    ),
    (
        "fh",
        "f > h",
        "Old Spanish (ferir > herir), Japanese /h/ history (reversed)",
        "Initial f debuccalizes to h (often later lost — chain with h-dropping).",
        "f > h / # _",
    ),
    (
        "sdebucc",
        "Coda s debuccalization",
        "Andalusian and Caribbean Spanish, Ancient Greek (initial)",
        "s weakens to h before a consonant or word-finally (estas > ehtah).",
        "s > h / _ C\ns > h / _ #",
    ),
    (
        "lvocal",
        "l-vocalization",
        "Polish (ł), Brazilian Portuguese, Cockney",
        "l becomes w before a consonant or finally (Brasil > Brasiw).",
        "l > w / _ C\nl > w / _ #",
    ),
    (
        "thfront",
        "th-fronting",
        "Cockney and Estuary English",
        "The dental fricative merges with f (think > fink).",
        "th > f",
    ),
    (
        "knloss",
        "Initial kn/gn reduction",
        "Early Modern English (knee, gnat)",
        "The stop of word-initial kn/gn clusters is lost.",
        "k > / # _ n\ng > / # _ n",
    ),
    (
        "wrloss",
        "Initial wr reduction",
        "Early Modern English (write, wrong)",
        "w is lost before word-initial r.",
        "w > / # _ r",
    ),
    (
        "hwmerge",
        "wine-whine merger",
        "Most modern English varieties",
        "hw simplifies to w (which/witch merge).",
        "h > / # _ w",
    ),
    (
        "ghloss",
        "gh loss",
        "Late Middle English (night, thought)",
        "The medial/final velar fricative (spelled gh) is lost.",
        "g h >",
    ),
    (
        "nonrhotic",
        "Non-rhoticity",
        "Southern British English, Boston, Australian English",
        "r is lost before a consonant and word-finally (car, card).",
        "r > / _ C\nr > / _ #",
    ),
    (
        "codarl",
        "Coda lambdacism r > l",
        "Andalusian Spanish, some Caribbean varieties",
        "Syllable-final r merges with l (puerta > puelta).",
        "r > l / _ C",
    ),
    (
        "betacism",
        "Betacism v > b",
        "Spanish, Galician, medieval Greek",
        "The v/b distinction collapses to b.",
        "v > b",
    ),
    (
        "yodcoal",
        "Yod coalescence",
        "British English (tune, duke)",
        "t+j and d+j fuse into affricates (written ch, j here).",
        "t j > ch\nd j > zh",
    ),
    (
        "midraise",
        "Long mid-vowel raising",
        "The Great Vowel Shift's lower half (see, moon)",
        "Long mid vowels raise: ee oo > ii uu (feeds the gvs entry — order them!).",
        "e e > i i\no o > u u",
    ),
    (
        "liqmetath",
        "Slavic liquid metathesis",
        "South/West Slavic, c. 800 AD (*gordŭ > grad)",
        "or/ol between consonants metathesize to ro/lo.",
        "o r > r o / C _ C\no l > l o / C _ C",
    ),
    (
        "deaspiration",
        "Voiced-aspirate deaspiration",
        "Iranian, Balto-Slavic, Celtic (PIE *bʰ dʰ gʰ > b d g)",
        "The breathy series merges with plain voiced stops.",
        "b h > b\nd h > d\ng h > g",
    ),
];

/// The shared class prelude every assembled ruleset gets.
const CLASS_PRELUDE: &str = "class V = a e i o u y aa ee ii oo uu eu ai au\nclass C = p t k b d g m n r l s z f v h w j ch zh sh th dh gh\n";

/// Assemble an ordered selection of change ids into a complete SCArs file.
pub fn assemble_sca(ids: &[&str]) -> Result<String, String> {
    let mut out = String::from(":: assembled by the Orpheus sound-change library tool\n");
    out.push_str(&format!(":: changes: {}\n", ids.join(" ")));
    out.push_str(":: rules apply IN ORDER — reorder the selection to explore chain shifts\n\n");
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

// ============================================================================
// FILES. Sound changes persist as ordinary SCArs rule files in `sca/` — the
// engine's own format, so `latte sca --file sca/<name>.sca <words>` (and any
// future tool that reads .sca) loads them directly; the ordered selection is
// recorded in a `:: changes: id id …` header so the GUI can restore it.
// Phonologies persist as `.phon` files in `phonology/`: three plain lines
// (`consonants = …`, `vowels = …`, `patterns = …`) any tool can parse.
// ============================================================================

pub fn sca_dir() -> std::path::PathBuf {
    std::env::var("ORPHEUS_SCA").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::PathBuf::from("sca"))
}
pub fn phon_dir() -> std::path::PathBuf {
    std::env::var("ORPHEUS_PHON").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::PathBuf::from("phonology"))
}

pub fn format_phon(ph: &Phonology, comment: &str) -> String {
    format!(
        ":: {}\nconsonants = {}\nvowels = {}\npatterns = {}\n",
        comment,
        ph.consonants.join(" "),
        ph.vowels.join(" "),
        ph.patterns.join(" ")
    )
}
pub fn parse_phon(text: &str) -> Result<Phonology, String> {
    let (mut c, mut v, mut p) = (Vec::new(), Vec::new(), Vec::new());
    for raw in text.lines() {
        let line = raw.split("::").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (k, val) = line.split_once('=').ok_or_else(|| format!("bad .phon line: {}", line))?;
        let items: Vec<String> = val.split_whitespace().map(String::from).collect();
        match k.trim() {
            "consonants" => c = items,
            "vowels" => v = items,
            "patterns" => p = items,
            other => return Err(format!("unknown .phon key '{}'", other)),
        }
    }
    if c.is_empty() || v.is_empty() {
        return Err("a .phon file needs consonants and vowels lines".into());
    }
    if p.is_empty() {
        p = vec!["CV".into(), "CVC".into()];
    }
    Ok(Phonology { consonants: c, vowels: v, patterns: p })
}

/// The ordered selection recorded in an assembled .sca header, if present.
pub fn sca_selection(text: &str) -> Vec<String> {
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix(":: changes:") {
            return rest.split_whitespace().map(String::from).collect();
        }
    }
    Vec::new()
}

// ============================================================================
// BELIEVABILITY. Typological checks on a proto-phonology, grounded in the
// cross-linguistic record (Maddieson's UPSID survey, WALS): implicational
// universals (voiced implies voiceless; front rounded implies both front
// unrounded and back rounded), near-universals (nasals, liquids, /t k/), and
// distributional norms (inventory sizes, the a-i-u triangle). Each check
// reports ok / note / warn with its evidence — a believable phonology is one
// that violates none of the strong implications and few of the norms.
// ============================================================================

pub fn phonology_report(ph: &Phonology) -> Vec<(&'static str, String)> {
    let cs: Vec<&str> = ph.consonants.iter().map(|s| s.as_str()).collect();
    let vs: Vec<&str> = ph.vowels.iter().map(|s| s.as_str()).collect();
    let has = |set: &[&str], x: &str| set.contains(&x);
    let mut out: Vec<(&'static str, String)> = Vec::new();

    // 1. the near-universal stops /t/ and /k/ (UPSID: each in >95% of languages)
    for (seg, pct) in [("t", "well over 95%"), ("k", "about 90%")] {
        if !has(&cs, seg) {
            out.push(("warn", format!("no /{}/: it appears in {} of surveyed languages (UPSID); its absence is possible (Hawaiian lacks /t/ by one analysis) but very marked", seg, pct)));
        }
    }
    // 2. voicing implication per place: a voiced stop implies its voiceless twin
    for (vcd, vls) in [("b", "p"), ("d", "t"), ("g", "k")] {
        if has(&cs, vcd) && !has(&cs, vls) {
            out.push(("warn", format!("/{}/ without /{}/: voiced stops overwhelmingly imply their voiceless counterparts (a strong UPSID implication; /p/-gaps as in Arabic are the rare exception)", vcd, vls)));
        }
    }
    if has(&cs, "z") && !has(&cs, "s") {
        out.push(("warn", "/z/ without /s/: voiced fricatives imply the voiceless counterpart almost without exception".into()));
    }
    // 3. nasals (UPSID: ~97% of languages have at least /n/ or /m/)
    if !has(&cs, "n") && !has(&cs, "m") {
        out.push(("warn", "no nasal consonants: about 97% of surveyed languages have /n/ or /m/; the exceptions (some Salishan and Lakes Plain languages) are famous for it".into()));
    }
    // 4. liquids (a liquid /l/ or /r/ appears in ~95% of languages)
    if !has(&cs, "l") && !has(&cs, "r") {
        out.push(("note", "no liquid (/l/ or /r/): attested (e.g. Nuxalk lacks plain /r/; some lack both) but unusual — roughly 95% of languages have one".into()));
    }
    // 5. sibilant
    if !has(&cs, "s") && !has(&cs, "sh") && !has(&cs, "z") {
        out.push(("note", "no sibilant: /s/-type sounds occur in over 80% of languages (UPSID); systems without any are attested but noteworthy".into()));
    }
    // 6. inventory sizes (UPSID consonant range 6-95+, median ~21; vowels 2-46, modal 5)
    match cs.len() {
        0..=5 => out.push(("warn", format!("{} consonants is below the smallest attested inventory (Rotokas, with 6)", cs.len()))),
        6..=8 => out.push(("note", format!("{} consonants: very small but attested (Rotokas 6, Hawaiian 8)", cs.len()))),
        9..=33 => out.push(("ok", format!("{} consonants sits comfortably in the cross-linguistic range (median ≈ 21, UPSID)", cs.len()))),
        _ => out.push(("note", format!("{} consonants is large — real at this size (Caucasian, Khoisan), but those inventories get there with ejectives, clicks, or secondary articulations", cs.len()))),
    }
    match vs.len() {
        0..=1 => out.push(("warn", format!("{} vowel(s): no attested language has fewer than 2 surface vowel qualities", vs.len()))),
        2 => out.push(("note", "2 vowels: at the attested floor (claimed for Ubykh and some Arrernte analyses) — defensible, controversial".into())),
        3 => {
            if has(&vs, "a") && has(&vs, "i") && has(&vs, "u") {
                out.push(("ok", "the 3-vowel a-i-u triangle: exactly the canonical minimal system (Classical Arabic, Quechua, Inuktitut)".into()));
            } else {
                out.push(("note", "3 vowels but not the a-i-u triangle: three-vowel systems overwhelmingly maximize dispersion as a-i-u".into()));
            }
        }
        4..=7 => out.push(("ok", format!("{} vowels: the modal zone (5 is the single most common count worldwide)", vs.len()))),
        8..=12 => out.push(("ok", format!("{} vowels: a large but ordinary system (Germanic, French territory)", vs.len()))),
        _ => out.push(("note", format!("{} vowel qualities is past nearly all attested systems unless length or nasality is doing some of the work", vs.len()))),
    }
    // 7. front rounded vowels imply the unrounded front AND rounded back series
    for fr in ["y", "ø", "oe"] {
        if has(&vs, fr) {
            if !has(&vs, "i") || !has(&vs, "u") {
                out.push(("warn", format!("front rounded /{}/ without both /i/ and /u/: front rounded vowels are typologically parasitic on the plain series (a robust WALS implication)", fr)));
            } else {
                out.push(("ok", format!("front rounded /{}/ alongside /i/ and /u/: the attested pattern (Germanic, Turkic, French)", fr)));
            }
        }
    }
    // 8. syllable patterns: onsetless and cluster typology
    let max_onset = ph
        .patterns
        .iter()
        .map(|p| p.chars().take_while(|c| *c == 'C').count())
        .max()
        .unwrap_or(0);
    if max_onset >= 3 {
        out.push(("note", "CCC onsets: attested (Georgian, Slavic) but restricted — real languages constrain them by sonority sequencing, which the generator does not model".into()));
    }
    if ph.patterns.iter().all(|p| p.ends_with('V')) {
        out.push(("ok", "open syllables only: a common, stable type (Polynesian, Japanese-like)".into()));
    }
    if out.iter().all(|(l, _)| *l == "ok") {
        out.push(("ok", "no typological red flags: this phonology would pass for a natural language's".into()));
    }
    out
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

    #[test]
    fn believability_checks_fire_correctly() {
        // a sound system: the PIE preset should pass cleanly
        let good = &super::presets()[1].2;
        let rep = super::phonology_report(good);
        assert!(rep.iter().all(|(l, _)| *l != "warn"), "{:?}", rep.iter().map(|(l, m)| format!("{}: {}", l, m)).collect::<Vec<_>>());
        // a broken one: voiced stops without voiceless, no nasals, one vowel
        let bad = super::Phonology {
            consonants: "b d g z".split(' ').map(String::from).collect(),
            vowels: vec!["e".into()],
            patterns: vec!["CV".into()],
        };
        let rep = super::phonology_report(&bad);
        let warns = rep.iter().filter(|(l, _)| *l == "warn").count();
        assert!(warns >= 5, "expected the implicational violations to fire: {:?}", rep);
    }
}
