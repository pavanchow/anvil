//! The `anvil` CLI.
//!
//!   anvil run <file.ir> [args...]
//!   anvil regalloc <file.ir> --regs K [args...]
//!   anvil emit <file.ir> --regs K
//!   anvil check <file.ir> --regs K [args...]

use anvil::error::{Error, Result};
use anvil::ir::Function;
use anvil::regalloc::{self, Location};
use anvil::target;
use anvil::{interference, liveness, lower, ssa};
use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(&args) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("{e}");
            exit(1);
        }
    }
}

fn dispatch(args: &[String]) -> Result<()> {
    let cmd = args.first().map(String::as_str).unwrap_or("");
    match cmd {
        "run" => cmd_run(&args[1..]),
        "regalloc" => cmd_regalloc(&args[1..]),
        "emit" => cmd_emit(&args[1..]),
        "check" => cmd_check(&args[1..]),
        "" | "help" | "-h" | "--help" => {
            print_usage();
            Ok(())
        }
        other => Err(Error::Parse(format!("unknown command '{other}'"))),
    }
}

fn print_usage() {
    eprintln!("anvil - a compiler backend with a readable register allocator");
    eprintln!();
    eprintln!("usage:");
    eprintln!("  anvil run <file.ir> [args...]");
    eprintln!("  anvil regalloc <file.ir> --regs K [args...]");
    eprintln!("  anvil emit <file.ir> --regs K");
    eprintln!("  anvil check <file.ir> --regs K [args...]");
}

/// Split positional args from the `--regs K` flag.
struct Parsed {
    file: String,
    regs: Option<usize>,
    args: Vec<i64>,
}

fn parse_args(args: &[String]) -> Result<Parsed> {
    let mut file: Option<String> = None;
    let mut regs: Option<usize> = None;
    let mut nums: Vec<i64> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--regs" {
            let v = args
                .get(i + 1)
                .ok_or_else(|| Error::Parse("--regs needs a value".into()))?;
            regs = Some(
                v.parse()
                    .map_err(|_| Error::Parse(format!("bad --regs value '{v}'")))?,
            );
            i += 2;
        } else if file.is_none() {
            file = Some(a.clone());
            i += 1;
        } else {
            nums.push(
                a.parse()
                    .map_err(|_| Error::Parse(format!("bad integer argument '{a}'")))?,
            );
            i += 1;
        }
    }
    Ok(Parsed {
        file: file.ok_or_else(|| Error::Parse("missing input file".into()))?,
        regs,
        args: nums,
    })
}

fn load(file: &str) -> Result<Function> {
    let src = std::fs::read_to_string(file)
        .map_err(|e| Error::Parse(format!("cannot read {file}: {e}")))?;
    let module = anvil::parse::parse_module(&src)?;
    module
        .entry_function()
        .cloned()
        .ok_or_else(|| Error::Parse("no function found".into()))
}

fn need_regs(p: &Parsed) -> Result<usize> {
    p.regs
        .ok_or_else(|| Error::Parse("this command needs --regs K".into()))
}

fn cmd_run(args: &[String]) -> Result<()> {
    let p = parse_args(args)?;
    let f = load(&p.file)?;
    let v = anvil::run_ir(&f, &p.args)?;
    println!("{v}");
    Ok(())
}

fn cmd_regalloc(args: &[String]) -> Result<()> {
    let p = parse_args(args)?;
    let k = need_regs(&p)?;
    let f = load(&p.file)?;
    anvil::validate::validate(&f)?;
    let phi_free = ssa::destruct(&f);
    let live = liveness::analyze(&phi_free);
    let ig = interference::build(&phi_free, &live);
    let alloc = regalloc::allocate(&phi_free, k)?;

    println!("liveness (per block):");
    for b in &phi_free.blocks {
        let inn = fmt_set(live.live_in[&b.label].iter());
        let out = fmt_set(live.live_out[&b.label].iter());
        println!("  {}: in={{{inn}}} out={{{out}}}", b.label);
    }

    println!("\ninterference graph:");
    for (v, ns) in &ig.adj {
        println!("  {v}: {}", fmt_set(ns.iter()));
    }

    println!("\nallocation (K={k}, rounds={}):", alloc.rounds);
    for v in phi_free.vregs() {
        match alloc.location(&v) {
            Some(Location::Reg(r)) => println!("  {v} -> r{r}"),
            Some(Location::Slot(s)) => println!("  {v} -> slot{s} (spilled)"),
            None => println!("  {v} -> (unused)"),
        }
    }
    if alloc.spilled.is_empty() {
        println!("\nspills: none");
    } else {
        let s = fmt_set(alloc.spilled.iter());
        println!("\nspills: {s} ({} slots)", alloc.num_slots);
    }
    Ok(())
}

fn cmd_emit(args: &[String]) -> Result<()> {
    let p = parse_args(args)?;
    let k = need_regs(&p)?;
    let f = load(&p.file)?;
    anvil::validate::validate(&f)?;
    let phi_free = ssa::destruct(&f);
    let alloc = regalloc::allocate(&phi_free, k)?;
    let tf = lower::lower(&alloc);
    println!("{tf}");
    Ok(())
}

fn cmd_check(args: &[String]) -> Result<()> {
    let p = parse_args(args)?;
    let k = need_regs(&p)?;
    let f = load(&p.file)?;
    anvil::validate::validate(&f)?;
    let phi_free = ssa::destruct(&f);
    let alloc = regalloc::allocate(&phi_free, k)?;
    let tf = lower::lower(&alloc);

    let ir_res = anvil::run_ir(&f, &p.args);
    let tgt_res = target::run(&tf, &p.args);

    let ir_str = fmt_res(&ir_res);
    let tgt_str = fmt_res(&tgt_res);
    println!("IR interpreter:     {ir_str}");
    println!("target interpreter: {tgt_str}");
    println!(
        "registers: {k}, spills: {}, slots: {}",
        alloc.spilled.len(),
        alloc.num_slots
    );
    if ir_res == tgt_res {
        println!("OK: results match");
        Ok(())
    } else {
        Err(Error::Runtime("MISMATCH: backend is wrong".into()))
    }
}

fn fmt_set<'a>(it: impl Iterator<Item = &'a anvil::ir::VReg>) -> String {
    it.map(|v| v.to_string()).collect::<Vec<_>>().join(", ")
}

fn fmt_res(r: &Result<i64>) -> String {
    match r {
        Ok(v) => v.to_string(),
        Err(e) => format!("{e}"),
    }
}
