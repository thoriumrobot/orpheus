//! `lattice plan` — runs the planning calculations from Cockshott & Cottrell's
//! *Towards a New Socialism*. The arithmetic lives in `lib/plan.lat` (labour-value and
//! gross-output iteration); this host supplies a worked two-sector economy, runs the
//! Latte computation on Loom, and prints the fixed-point results as decimals.

use crate::knot::{Knot, N};
use crate::latte;

// A small productive economy of two goods: steel and grain. Quantities are fixed-point
// (×1000). `recipes` lists one recipe per good = the column of the input matrix A:
//   make 1 steel: 0.2 steel + 0.1 grain   -> [200 100]
//   make 1 grain: 0.5 steel + 0.1 grain   -> [500 100]
const GOODS: [&str; 2] = ["steel", "grain"];
const RECIPES: &str = "[ [200 [100 0]] [ [500 [100 0]] 0 ] ]";
const DIRECT_LABOUR: &str = "[400 [300 0]]"; // direct labour per unit: steel 0.4, grain 0.3
const FINAL_DEMAND: &str = "[0 [1000 0]]"; // want a net 1.0 unit of grain

fn run(expr: &str) -> Result<N, String> {
    latte::run_with_libs(expr, &["std", "plan"])
}

fn to_vec(n: &N) -> Vec<u128> {
    let mut out = Vec::new();
    let mut cur = n.clone();
    while let Knot::Cell(h, t) = &*cur {
        if let Some(v) = h.as_atom().and_then(|a| a.to_u128()) {
            out.push(v);
        }
        cur = t.clone();
    }
    out
}

fn fixed(v: u128) -> String {
    format!("{}.{:03}", v / 1000, v % 1000)
}

/// Compute a planning report for an arbitrary final demand (values in thousandths, e.g.
/// 1000 = 1.0 unit). Used by the `/api/plan` endpoint and the planner GUI page.
pub fn plan_report(demand_steel: u128, demand_grain: u128, iters: u64) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Two-sector economy ({} · {}), {} iterations of the TANS recurrence.\n\n",
        GOODS[0], GOODS[1], iters
    ));
    out.push_str("technology (inputs per unit output):\n");
    out.push_str("  1 steel needs 0.200 steel + 0.100 grain;  1 grain needs 0.500 steel + 0.100 grain\n");
    out.push_str("  direct labour per unit: steel 0.400, grain 0.300\n\n");

    match run(&format!("(values {} {} {})", DIRECT_LABOUR, RECIPES, iters)) {
        Ok(v) => {
            let vals = to_vec(&v);
            out.push_str("labour values  (v = l + v·A, iterated) — hours per unit:\n");
            for (g, val) in GOODS.iter().zip(vals.iter()) {
                out.push_str(&format!("  {:<6} = {}\n", g, fixed(*val)));
            }
        }
        Err(e) => out.push_str(&format!("labour values: error {}\n", e)),
    }

    let demand = format!("[{} [{} 0]]", demand_steel, demand_grain);
    match run(&format!("(gross {} {} {})", demand, RECIPES, iters)) {
        Ok(x) => {
            let xs = to_vec(&x);
            out.push_str(&format!(
                "\ngross outputs  (x = y + A·x, iterated) for final demand [steel {}, grain {}]:\n",
                fixed(demand_steel),
                fixed(demand_grain)
            ));
            for (g, val) in GOODS.iter().zip(xs.iter()) {
                out.push_str(&format!("  {:<6} = {} units\n", g, fixed(*val)));
            }
        }
        Err(e) => out.push_str(&format!("gross outputs: error {}\n", e)),
    }
    out
}

pub fn cmd_plan(args: &[String]) {
    let mut iters: u64 = 60;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--iters" => {
                i += 1;
                if i < args.len() {
                    iters = args[i].parse().unwrap_or(60);
                }
            }
            other => {
                eprintln!("plan: unknown arg {}", other);
                return;
            }
        }
        i += 1;
    }

    println!("Planning calculations — Towards a New Socialism (Cockshott & Cottrell)");
    println!("two-sector economy: {} and {}; {} iterations\n", GOODS[0], GOODS[1], iters);

    println!("technology (inputs per unit output, fixed-point ×1000):");
    println!("  make 1 {}: 0.200 {} + 0.100 {}", GOODS[0], GOODS[0], GOODS[1]);
    println!("  make 1 {}: 0.500 {} + 0.100 {}", GOODS[1], GOODS[0], GOODS[1]);
    println!("  direct labour per unit: {} 0.400, {} 0.300\n", GOODS[0], GOODS[1]);

    match run(&format!("(values {} {} {})", DIRECT_LABOUR, RECIPES, iters)) {
        Ok(v) => {
            let vals = to_vec(&v);
            println!("labour values  (v = l + v·A, iterated):");
            for (g, val) in GOODS.iter().zip(vals.iter()) {
                println!("  {:<6} = {} labour-hours per unit", g, fixed(*val));
            }
        }
        Err(e) => println!("labour values: error {}", e),
    }

    match run(&format!("(gross {} {} {})", FINAL_DEMAND, RECIPES, iters)) {
        Ok(x) => {
            let xs = to_vec(&x);
            println!("\ngross outputs to deliver final demand [{} 0.000, {} 1.000]:", GOODS[0], GOODS[1]);
            for (g, val) in GOODS.iter().zip(xs.iter()) {
                println!("  {:<6} = {} units of gross output", g, fixed(*val));
            }
        }
        Err(e) => println!("gross outputs: error {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labour_values_converge() {
        // closed-form solution of v = l + v·A is v ≈ [0.582, 0.657]
        let v = run(&format!("(values {} {} 80)", DIRECT_LABOUR, RECIPES)).unwrap();
        let vals = to_vec(&v);
        assert_eq!(vals.len(), 2);
        let near = |a: u128, b: u128| (a as i64 - b as i64).abs() <= 2;
        assert!(near(vals[0], 582), "steel value {} not ~582", vals[0]);
        assert!(near(vals[1], 657), "grain value {} not ~657", vals[1]);
    }

    #[test]
    fn values_grow_from_direct_labour() {
        // total labour value is at least the direct labour (indirect labour adds on top)
        let v = run(&format!("(values {} {} 40)", DIRECT_LABOUR, RECIPES)).unwrap();
        let vals = to_vec(&v);
        assert!(vals[0] >= 400 && vals[1] >= 300);
    }

    #[test]
    fn gross_output_meets_demand() {
        // gross output must exceed the final demand (it also covers intermediate use)
        let x = run(&format!("(gross {} {} 80)", FINAL_DEMAND, RECIPES)).unwrap();
        let xs = to_vec(&x);
        assert!(xs[1] >= 1000, "grain gross {} < demand", xs[1]); // at least the 1.0 demanded
        assert!(xs[0] > 0, "steel gross should be positive (grain needs steel)");
    }
}
