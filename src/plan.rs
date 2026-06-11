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

// ============================================================================
// CUSTOM ECONOMIES. An economy spec is line-oriented text:
//
//   sector steel  l=0.4  steel=0.2 grain=0.1     # inputs per unit output
//   sector grain  l=0.3  steel=0.5 grain=0.1
//   demand steel=0 grain=1.0                     # net final demand
//   market steel=0.9 grain=1.4                   # observed clearing prices
//                                                #   (labour tokens), optional
//   labour 1.2                                   # labour budget, optional
//
// With `market` lines the report runs ONE STEERING STEP (TNS ch. 8): each
// demand entry scales by its price/labour-value ratio. With a `labour` budget
// below requirements it runs the HARMONY allocation (TNS pp. 94-99). The
// arithmetic — labour values, Leontief gross outputs, steering, harmony —
// all runs in lib/plan.lat on Loom.
// ============================================================================

#[derive(Default)]
pub struct Economy {
    pub goods: Vec<String>,
    pub labour: Vec<u128>,        // direct labour per unit (×1000)
    pub recipes: Vec<Vec<u128>>,  // recipe per good: inputs of each good (×1000)
    pub demand: Vec<u128>,        // final demand (×1000)
    pub market: Option<Vec<u128>>, // observed clearing prices (×1000)
    pub budget: Option<u128>,     // labour budget (×1000)
}

fn fx(s: &str) -> Option<u128> {
    let s = s.trim();
    let neg = s.starts_with('-');
    if neg {
        return None; // quantities are non-negative
    }
    let mut it = s.splitn(2, '.');
    let whole: u128 = it.next()?.parse().ok()?;
    let frac = it.next().unwrap_or("0");
    let frac3: String = format!("{:0<3}", frac.chars().take(3).collect::<String>());
    Some(whole * 1000 + frac3.parse::<u128>().ok()?)
}

pub fn parse_economy(spec: &str) -> Result<Economy, String> {
    let mut eco = Economy::default();
    let mut demand_map: Vec<(String, u128)> = Vec::new();
    let mut market_map: Vec<(String, u128)> = Vec::new();
    let mut inputs: Vec<Vec<(String, u128)>> = Vec::new();
    for raw in spec.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut toks = line.split_whitespace();
        match toks.next() {
            Some("sector") | Some("good") => {
                let name = toks.next().ok_or("sector: missing name")?.to_string();
                let mut l = 0u128;
                let mut ins: Vec<(String, u128)> = Vec::new();
                for t in toks {
                    let mut kv = t.splitn(2, '=');
                    let k = kv.next().unwrap_or("");
                    let v = fx(kv.next().ok_or_else(|| format!("sector {}: bad token {}", name, t))?)
                        .ok_or_else(|| format!("sector {}: bad number in {}", name, t))?;
                    if k == "l" || k == "labour" || k == "labor" {
                        l = v;
                    } else {
                        ins.push((k.to_string(), v));
                    }
                }
                eco.goods.push(name);
                eco.labour.push(l);
                inputs.push(ins);
            }
            Some("labour") | Some("labor") => {
                // labour <budget>
                let v = toks.next().and_then(fx).ok_or("labour: missing budget")?;
                eco.budget = Some(v);
            }
            Some(kw @ ("demand" | "market")) => {
                for t in toks {
                    let mut kv = t.splitn(2, '=');
                    let k = kv.next().unwrap_or("").to_string();
                    let v = fx(kv.next().ok_or_else(|| format!("{}: bad token {}", kw, t))?)
                        .ok_or_else(|| format!("{}: bad number in {}", kw, t))?;
                    if kw == "demand" {
                        demand_map.push((k, v));
                    } else {
                        market_map.push((k, v));
                    }
                }
            }
            Some(other) => return Err(format!("unknown directive '{}'", other)),
            None => {}
        }
    }
    if eco.goods.is_empty() {
        return Err("no sectors defined".into());
    }
    let idx = |n: &str| eco.goods.iter().position(|g| g == n);
    // recipes: column per good over ALL goods
    for ins in &inputs {
        let mut col = vec![0u128; eco.goods.len()];
        for (n, v) in ins {
            let i = idx(n).ok_or_else(|| format!("unknown input good '{}'", n))?;
            col[i] = *v;
        }
        eco.recipes.push(col);
    }
    eco.demand = vec![0; eco.goods.len()];
    for (n, v) in demand_map {
        let i = idx(&n).ok_or_else(|| format!("demand names unknown good '{}'", n))?;
        eco.demand[i] = v;
    }
    if !market_map.is_empty() {
        let mut m = vec![0; eco.goods.len()];
        for (n, v) in market_map {
            let i = idx(&n).ok_or_else(|| format!("market names unknown good '{}'", n))?;
            m[i] = v;
        }
        eco.market = Some(m);
    }
    Ok(eco)
}

fn lat_vec(v: &[u128]) -> String {
    let mut s = String::from("0");
    for x in v.iter().rev() {
        s = format!("[{} {}]", x, s);
    }
    s
}
fn lat_mat(m: &[Vec<u128>]) -> String {
    let mut s = String::from("0");
    for row in m.iter().rev() {
        s = format!("[ {} {} ]", lat_vec(row), s);
    }
    s
}

/// The full TNS report for a custom economy. Every numeric step runs in
/// lib/plan.lat on Loom; this host only parses, formats, and narrates.
pub fn plan_report_custom(eco: &Economy, iters: u64) -> Result<String, String> {
    let mut out = String::new();
    let n = eco.goods.len();
    out.push_str(&format!(
        "{}-sector economy: {}.  {} iterations of each recurrence; all arithmetic in lib/plan.lat on Loom.\n\n",
        n,
        eco.goods.join(" · "),
        iters
    ));
    out.push_str("technology (inputs per unit output; direct labour l):\n");
    for (j, g) in eco.goods.iter().enumerate() {
        let ins: Vec<String> = eco
            .recipes[j]
            .iter()
            .enumerate()
            .filter(|(_, &v)| v > 0)
            .map(|(i, &v)| format!("{} {}", fixed(v), eco.goods[i]))
            .collect();
        out.push_str(&format!(
            "  1 {} needs {}  + {} labour\n",
            g,
            if ins.is_empty() { "nothing".into() } else { ins.join(" + ") },
            fixed(eco.labour[j])
        ));
    }

    // labour values: v = l + v·A
    let values = to_vec(&run(&format!(
        "(values {} {} {})",
        lat_vec(&eco.labour),
        lat_mat(&eco.recipes),
        iters
    ))?);
    out.push_str("\nlabour values  (v = l + v·A, iterated — total embodied labour per unit):\n");
    for (g, v) in eco.goods.iter().zip(&values) {
        out.push_str(&format!("  {:<10} = {} hours\n", g, fixed(*v)));
    }

    // gross outputs for the demand
    let gross = to_vec(&run(&format!(
        "(gross {} {} {})",
        lat_vec(&eco.demand),
        lat_mat(&eco.recipes),
        iters
    ))?);
    out.push_str(&format!(
        "\ngross outputs  (x = y + A·x — production including intermediate use)\n  for final demand [{}]:\n",
        eco.goods
            .iter()
            .zip(&eco.demand)
            .map(|(g, d)| format!("{} {}", g, fixed(*d)))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    for (g, x) in eco.goods.iter().zip(&gross) {
        out.push_str(&format!("  {:<10} = {} units\n", g, fixed(*x)));
    }
    // labour-token accounting: total labour = v·y (equivalently l·x)
    let need: u128 = values
        .iter()
        .zip(&eco.demand)
        .map(|(v, y)| v * y / 1000)
        .sum();
    out.push_str(&format!(
        "\nlabour accounting: this plan costs {} hours of social labour\n  (Σ v·y; labour tokens issued for work done buy exactly this product — TNS ch. 2, 5)\n",
        fixed(need)
    ));

    // market steering, if clearing prices were observed
    if let Some(prices) = &eco.market {
        let steered = to_vec(&run(&format!(
            "(steer {} {} {})",
            lat_vec(&eco.demand),
            lat_vec(prices),
            lat_vec(&values)
        ))?);
        let ratios = to_vec(&run(&format!(
            "(ratios {} {})",
            lat_vec(prices),
            lat_vec(&values)
        ))?);
        out.push_str("\nconsumer-goods market clearing (TNS ch. 8): price/labour-value steers the plan\n");
        for i in 0..n {
            let dir = if ratios[i] > 1050 {
                "expand"
            } else if ratios[i] < 950 {
                "contract"
            } else {
                "hold"
            };
            out.push_str(&format!(
                "  {:<10} clearing price {} vs value {}  ->  ratio {}  ->  {} target to {}\n",
                eco.goods[i],
                fixed(prices[i]),
                fixed(values[i]),
                fixed(ratios[i]),
                dir,
                fixed(steered[i])
            ));
        }
        let g2 = to_vec(&run(&format!(
            "(gross {} {} {})",
            lat_vec(&steered),
            lat_mat(&eco.recipes),
            iters
        ))?);
        out.push_str("  next-period gross outputs for the steered plan:\n");
        for (g, x) in eco.goods.iter().zip(&g2) {
            out.push_str(&format!("    {:<10} = {} units\n", g, fixed(*x)));
        }
    }

    // harmony allocation, if a labour budget binds
    if let Some(budget) = eco.budget {
        let costs: Vec<u128> = values
            .iter()
            .zip(&eco.demand)
            .map(|(v, y)| v * y / 1000)
            .collect();
        let total: u128 = costs.iter().sum();
        if budget < total {
            let alloc = to_vec(&run(&format!(
                "(harmony_alloc {} {} 400)",
                lat_vec(&costs),
                budget
            ))?);
            let h = run(&format!(
                "(harmony {} {})",
                lat_vec(&alloc),
                lat_vec(&costs)
            ))?
            .as_atom()
            .and_then(|a| a.to_u128())
            .unwrap_or(0);
            out.push_str(&format!(
                "\nharmony balancing (TNS pp. 94-99): budget {} hours < required {} hours.\n  Allocating labour to maximize Σ 1-(1-r)² (the concave harmony function —\n  marginals equalized by parcel transfers, Cockshott's marginalist method):\n",
                fixed(budget),
                fixed(total)
            ));
            for i in 0..n {
                let r = if costs[i] == 0 { 1000 } else { (alloc[i] * 1000 / costs[i]).min(1000) };
                out.push_str(&format!(
                    "  {:<10} gets {} hours of {} needed  ->  {}% of target\n",
                    eco.goods[i],
                    fixed(alloc[i]),
                    fixed(costs[i]),
                    r / 10
                ));
            }
            out.push_str(&format!(
                "  total harmony achieved: {} of a possible {}.000\n",
                fixed(h),
                n
            ));
        } else {
            out.push_str(&format!(
                "\nlabour budget {} hours covers the full requirement ({} hours); no rationing needed.\n",
                fixed(budget),
                fixed(total)
            ));
        }
    }
    Ok(out)
}

/// The built-in demonstration economy (used when no spec is given).
pub fn demo_spec() -> &'static str {
    "sector steel  l=0.4  steel=0.2 grain=0.1\n     sector grain  l=0.3  steel=0.5 grain=0.1\n     sector bread  l=0.2  grain=0.6\n     demand steel=0 grain=1.0 bread=2.0\n     market steel=0.9 grain=1.6 bread=0.8\n     labour 1.2\n"
}

pub fn cmd_plan(args: &[String]) {
    let mut iters: u64 = 60;
    let mut spec_path: Option<String> = None;
    let mut demo = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--iters" => {
                i += 1;
                if i < args.len() {
                    iters = args[i].parse().unwrap_or(60);
                }
            }
            "--spec" | "--economy" => {
                i += 1;
                if i < args.len() {
                    spec_path = Some(args[i].clone());
                }
            }
            "--demo3" | "--demo" => demo = true,
            other => {
                eprintln!("plan: unknown arg {}", other);
                eprintln!("usage: latte plan [--iters N] [--spec FILE | --demo3]");
                eprintln!("  --spec FILE   plan a CUSTOM economy (sector/demand/market/labour lines)");
                eprintln!("  --demo3       the 3-sector demo with market steering and a labour budget");
                return;
            }
        }
        i += 1;
    }
    // a custom or demo economy takes the full TNS path
    let spec = match (&spec_path, demo) {
        (Some(p), _) => match std::fs::read_to_string(p) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("plan: cannot read {}: {}", p, e);
                return;
            }
        },
        (None, true) => Some(demo_spec().to_string()),
        _ => None,
    };
    if let Some(spec) = spec {
        println!("Planning — Towards a New Socialism (Cockshott & Cottrell)\n");
        match parse_economy(&spec).and_then(|eco| plan_report_custom(&eco, iters)) {
            Ok(r) => println!("{}", r),
            Err(e) => eprintln!("plan: {}", e),
        }
        println!("Criticisms and replies are documented in docs/planning.md (the calculation debate).");
        return;
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

    #[test]
    fn custom_economy_full_tns_report() {
        let eco = super::parse_economy(super::demo_spec()).unwrap();
        assert_eq!(eco.goods.len(), 3);
        let r = super::plan_report_custom(&eco, 60).unwrap();
        assert!(r.contains("labour values"), "{}", r);
        assert!(r.contains("market clearing"), "{}", r);
        assert!(r.contains("harmony balancing"), "{}", r);
        // labour value of steel solves v_s = .4 + .2 v_s + .1 v_g -> 0.581
        assert!(r.contains("steel      = 0.58"), "{}", r);
    }
}
