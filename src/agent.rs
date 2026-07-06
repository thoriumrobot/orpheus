//! Agents — pure transition functions `(action, state) -> (effects, state')`,
//! now written as Latte MODULES (cores) whose high-level behavior is implemented
//! in the new language itself: the counter's `add` is a Latte library function,
//! and the key-value store is an association list with `get`/`put`/`del` written in
//! Latte. Rust supplies only the 12-rule core and (optionally) audited jets.
//!
//! An agent's compiled form is a core value `[battery sample]`. To run the `poke`
//! arm we set the sample to `[action state]` (Loom EDIT) and invoke it (Loom CALL).

use crate::atom::Atom;
use crate::knot::{cell, cord, num, Knot, N};
use crate::latte;
use crate::loom::{self, slot, tar, Crash, Eval};

// ---- counter agent: high-level `poke` calls the Latte library `add` -------
pub const COUNTER_V1: &str = r#"
core counter
  add  = fn [a b] -> fast %add
                     loop with [acc = a, i = 0] :
                       if (i == b) then acc else again(+(acc), +(i)) end
  poke = fn [action state] ->
           let tag = head action in
           let amt = tail action in
           case tag of
             %incr  -> [ nil +(state) ] ;
             %reset -> [ nil 0 ] ;
             %add   -> [ nil (add state amt) ] ;
             %get   -> [ nil state ] ;
             _      -> [ nil state ]
           end
end
"#;

/// v2 (for the upgrade demo): incr adds 2.
pub const COUNTER_V2: &str = r#"
core counter
  add  = fn [a b] -> loop with [acc = a, i = 0] :
                       if (i == b) then acc else again(+(acc), +(i)) end
  poke = fn [action state] ->
           let tag = head action in
           let amt = tail action in
           case tag of
             %incr  -> [ nil +(+(state)) ] ;
             %reset -> [ nil 0 ] ;
             %add   -> [ nil (add state amt) ] ;
             %get   -> [ nil state ] ;
             _      -> [ nil state ]
           end
end
"#;

// ---- key-value store agent: an assoc list, entirely in Latte -------------
//   state = nil(0) | [[key val] rest]
pub const KV: &str = r#"
core kv
  :: look up key k in assoc list lst (0 if absent)
  get = fn [lst k] ->
          loop with [cur = lst] :
            if (cur == 0) then 0
            else if ((head (head cur)) == k) then (tail (head cur))
            else again((tail cur))
          end
  :: remove every pair with key k
  del = fn [lst k] ->
          loop with [cur = lst, acc = 0] :
            if (cur == 0) then acc
            else if ((head (head cur)) == k) then again((tail cur), acc)
            else again((tail cur), [ (head cur) acc ])
          end
  :: set k = v (drop any old binding first, then prepend)
  put = fn [lst k v] -> [ [k v] (del lst k) ]
  poke = fn [action state] ->
           let tag = head action in
           let val = tail action in
           case tag of
             %put   -> [ nil (put state (head val) (tail val)) ] ;
             %del   -> [ nil (del state val) ] ;
             %clear -> [ nil 0 ] ;
             _      -> [ nil state ]
           end
end
"#;

// ---- collaborative-notes agent: block-sequence documents, entirely in Latte
/// The notes agent (lib/notes.lat): shared documents as anchored block
/// sequences with tombstones — intention-preserving merges on top of the
/// event log's total order. See the module header for the full model.
pub const NOTES: &str = include_str!("../lib/notes.lat");

pub struct Agent {
    core: N,
    poke_axis: u128,
    label: String,
    // When the module source is known, the `poke` transition can also be run by the
    // Anvil-compiled native program (compiled once, then each [action state] piped in on
    // stdin) so the *persistent state is folded by natively compiled code* — with no
    // interpreter fuel ceiling. The interpreter remains the verified fallback.
    native: Option<NativePoke>,
}

#[derive(Clone)]
struct NativePoke {
    expr: String,
    libs: Vec<String>,
}

impl Agent {
    pub fn new() -> Result<Agent, String> {
        Agent::new_version(1)
    }

    pub fn new_version(version: u8) -> Result<Agent, String> {
        match version {
            2 => Agent::from_module(COUNTER_V2, "counter-v2"),
            _ => {
                loom::register_jet(b"add", jet_add);
                Agent::from_module(COUNTER_V1, "counter-v1")
            }
        }
    }

    pub fn new_kv() -> Result<Agent, String> {
        Agent::from_module(KV, "kv")
    }

    pub fn new_notes() -> Result<Agent, String> {
        Agent::from_module(NOTES, "notes")
    }

    pub fn by_name(name: &str) -> Result<Agent, String> {
        match name {
            "v2" => Agent::new_version(2),
            "kv" => Agent::new_kv(),
            "notes" => Agent::new_notes(),
            _ => Agent::new_version(1),
        }
    }

    fn from_module(src: &str, label: &str) -> Result<Agent, String> {
        let (core, axes) = latte::compile_module(src)?;
        let poke_axis = axes
            .iter()
            .find(|(n, _)| n == "poke")
            .map(|(_, a)| *a)
            .ok_or_else(|| "module has no `poke` arm".to_string())?;
        Ok(Agent {
            core,
            poke_axis,
            label: label.to_string(),
            native: Self::build_native(src),
        })
    }

    /// Set up the native `poke` fold for this module, if it has a `core NAME`: register the
    /// module as a runtime library (so Anvil can resolve `poke`) and record the expression
    /// and library list to compile. Compilation itself is lazy (first `step`); a `None` here,
    /// or any later native decline, transparently falls back to the interpreter.
    fn build_native(src: &str) -> Option<NativePoke> {
        let core_name = latte::module_core_name(src)?;
        // Register PRIVATELY: resolvable for native compilation, but kept out of all_libs() so an
        // app's arms (e.g. a counter/todo `add`) never shadow std arithmetic in shared evaluators.
        latte::register_private_lib(&core_name, src);
        let mut libs = latte::module_imports(src);
        if !libs.iter().any(|l| l == &core_name) {
            libs.push(core_name);
        }
        // `poke` returns [effects state]; the input pair [action state] arrives on stdin,
        // so action = (head __in), state = (tail __in). (The initial state is the atom 0,
        // which the app's own `cur` normalizes — so we must NOT index into it.)
        Some(NativePoke {
            expr: "(poke (head __in) (tail __in))".to_string(),
            libs,
        })
    }

    /// Build an agent from an arbitrary Latte application module (used by Mocha).
    /// The module must expose a `poke = fn [action state] -> [effects state]` arm.
    pub fn from_source(src: &str, label: &str) -> Result<Agent, String> {
        Agent::from_module(src, label)
    }

    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn initial_state(&self) -> N {
        num(0)
    }

    /// Apply one action: set sample = [action state], invoke `poke`, return new state.
    pub fn step(&self, action: &N, state: &N) -> Result<N, Crash> {
        // Native fold: persistent state is updated by the Anvil-compiled `poke` (compiled
        // once, content-addressed, then this position piped in), with no fuel ceiling. The
        // emitted result is byte-identical to the interpreter's (the compiler is audited
        // against it system-wide), so this changes only speed/limits, not semantics.
        if let Some(np) = &self.native {
            let input = cell(action.clone(), state.clone());
            let libs: Vec<&str> = np.libs.iter().map(|s| s.as_str()).collect();
            if let Some(prod) = crate::rustgen::run_native_with_input(&np.expr, &input, &libs, false) {
                if let Some((_effects, new_state)) = prod.as_cell() {
                    return Ok(new_state.clone());
                }
            }
            // native unavailable/declined → verified interpreter fallback below
        }
        let sample = cell(action.clone(), state.clone());
        let core2 = loom::edit(&Atom::from_u128(3), &sample, &self.core)?;
        let armf = slot(&Atom::from_u128(self.poke_axis), &core2)?;
        let product = tar(&core2, &armf)?; // [effects new-state]
        match product.as_cell() {
            Some((_effects, new_state)) => Ok(new_state.clone()),
            None => Err(Crash::Bottom("agent did not return [effects state]".into())),
        }
    }

    pub fn formula_cid(&self) -> [u8; 32] {
        self.core.cid()
    }
    pub fn cid_atom(&self) -> Atom {
        Atom::from_bytes_le(self.formula_cid().to_vec())
    }
    pub fn cid_hex(&self) -> String {
        self.core.cid_hex()
    }
    #[allow(dead_code)]
    pub fn version(&self) -> u8 {
        if self.label.ends_with("v2") {
            2
        } else {
            1
        }
    }
}

/// Native jet for the Latte `add` arm. The arm runs with subject = the core, so
/// the sample [a b] is at axis 3. MUST equal what the Latte loop computes (checked
/// in audit mode).
fn jet_add(subject: &N) -> Eval {
    let sample = slot(&Atom::from_u128(3), subject)?;
    let (a, b) = sample
        .as_cell()
        .ok_or_else(|| Crash::Bottom("add jet: sample not [a b]".into()))?;
    let aa = a.as_atom().ok_or_else(|| Crash::Bottom("add jet: a not atom".into()))?;
    let bb = b.as_atom().ok_or_else(|| Crash::Bottom("add jet: b not atom".into()))?;
    Ok(std::sync::Arc::new(Knot::Atom(aa.add(bb))))
}

// ---- action constructors ---------------------------------------------------
pub fn act_incr() -> N {
    cell(cord("incr"), num(0))
}
pub fn act_reset() -> N {
    cell(cord("reset"), num(0))
}
pub fn act_add(n: u128) -> N {
    cell(cord("add"), num(n))
}
pub fn act_get() -> N {
    cell(cord("get"), num(0))
}
pub fn act_put(k: &str, v: u128) -> N {
    cell(cord("put"), cell(cord(k), num(v)))
}
pub fn act_del(k: &str) -> N {
    cell(cord("del"), cord(k))
}
pub fn act_clear() -> N {
    cell(cord("clear"), num(0))
}

/// Parse a textual command into an action knot (covers both agents).
pub fn parse_action(line: &str) -> Option<N> {
    let mut it = line.split_whitespace();
    match it.next()? {
        "incr" => Some(act_incr()),
        "reset" => Some(act_reset()),
        "get" => Some(act_get()),
        "clear" => Some(act_clear()),
        "add" => Some(act_add(it.next()?.parse().ok()?)),
        "put" => {
            let k = it.next()?;
            let v: u128 = it.next()?.parse().ok()?;
            Some(act_put(k, v))
        }
        "del" => Some(act_del(it.next()?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_semantics_and_calls_lib_add() {
        let a = Agent::new().unwrap();
        let mut s = a.initial_state();
        s = a.step(&act_incr(), &s).unwrap();
        s = a.step(&act_add(40), &s).unwrap(); // poke calls the Latte `add`
        assert_eq!(s, num(41));
        s = a.step(&act_reset(), &s).unwrap();
        assert_eq!(s, num(0));
    }

    #[test]
    fn agent_modules_do_not_pollute_all_libs() {
        // Building agents must NOT leak their arms into all_libs(): a v2 counter defines a bare
        // `add` (no %add jet) that, if visible, shadows std arithmetic and makes `(add big big)`
        // OutOfFuel for every all-libs consumer (the native/interp fuzzer, the SCA, the GUI).
        crate::jets::register_std_jets();
        let _v1 = Agent::new_version(1).unwrap();
        let _v2 = Agent::new_version(2).unwrap(); // defines a bare `add`
        let _kv = Agent::new_kv().unwrap();
        let names = latte::all_libs();
        for n in ["counter", "kv"] {
            assert!(!names.iter().any(|s| s == n), "agent module '{}' leaked into all_libs()", n);
        }
        // std arithmetic must still win across the full all-libs namespace (fast jetted add)
        let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let r = latte::run_with_libs("(add 4000000000 4000000000)", &refs)
            .expect("all-libs `add` must be std arithmetic, not a shadowed Peano loop");
        assert_eq!(r, num(8000000000));
    }

    #[test]
    fn jet_add_matches_pure_reduction() {
        loom::set_jet_audit(true);
        let a = Agent::new().unwrap();
        let s = a.step(&act_add(5000), &a.initial_state()).expect("jet agrees with pure");
        assert_eq!(s, num(5000));
        loom::set_jet_audit(false);
    }

    #[test]
    fn kv_store_in_lattice() {
        let a = Agent::new_kv().unwrap();
        let mut s = a.initial_state();
        s = a.step(&act_put("x", 10), &s).unwrap();
        s = a.step(&act_put("y", 20), &s).unwrap();
        s = a.step(&act_put("x", 99), &s).unwrap(); // overwrite x
        // verify via the Latte `get` arm by poking %get? get is a read; instead we
        // re-run get through a fresh evaluation using the agent core would need the
        // arm; simplest: check structural state has x=99 and y=20 by folding lookups.
        // Use the public step for %del to confirm removal, and check counts.
        let after_del = a.step(&act_del("y"), &s).unwrap();
        // y removed: state should not contain key "y"
        assert!(!contains_key(&after_del, "y"));
        assert!(contains_key(&after_del, "x"));
        assert_eq!(lookup(&after_del, "x"), Some(99));
    }

    // helpers that walk the assoc-list state produced by the Latte KV agent
    fn contains_key(state: &N, k: &str) -> bool {
        lookup(state, k).is_some()
    }
    fn lookup(state: &N, k: &str) -> Option<u128> {
        let mut cur = state.clone();
        while let Some((pair, rest)) = cur.as_cell() {
            if let Some((key, val)) = pair.as_cell() {
                if key.as_atom().and_then(|a| a.as_cord()).as_deref() == Some(k) {
                    return val.as_atom().and_then(|a| a.to_u128());
                }
            }
            cur = rest.clone();
        }
        None
    }

    #[test]
    fn v2_distinct_cid_and_semantics() {
        let v1 = Agent::new_version(1).unwrap();
        let v2 = Agent::new_version(2).unwrap();
        assert_ne!(v1.formula_cid(), v2.formula_cid());
        assert_eq!(v2.step(&act_incr(), &num(0)).unwrap(), num(2));
    }
}
