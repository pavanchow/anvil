//! Structural validation of the SSA IR.
//!
//! The rest of the backend trusts its input to be well-formed SSA. This pass is
//! the gate that makes that trust safe: it runs at the top of the compile
//! pipeline and before the reference interpreter, so every entry point rejects
//! ill-formed input with a clear error instead of miscompiling it silently.
//!
//! It checks four properties:
//!   1. every value is defined at most once (params, instruction dsts, phi dsts);
//!   2. every used value is defined and its definition dominates the use (a phi
//!      use is checked at the exit of its incoming predecessor, not the phi);
//!   3. every phi has exactly one entry per CFG predecessor and none for a
//!      non-predecessor;
//!   4. every branch/jump/phi target names a real block.

use crate::error::{Error, Result};
use crate::ir::*;
use std::collections::{BTreeSet, HashMap, HashSet};

pub fn validate(func: &Function) -> Result<()> {
    let idx: HashMap<&Label, usize> = func
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (&b.label, i))
        .collect();

    check_targets(func, &idx)?;
    let defs = collect_defs(func)?;
    let reachable = reachable_blocks(func, &idx);
    let doms = dominators(func, &idx);
    check_phi_preds(func, &idx, &reachable)?;
    check_uses(func, &idx, &defs, &doms, &reachable)?;
    Ok(())
}

/// Where a value is defined: which block, and its position within that block's
/// instruction list. Parameters are marked separately since they dominate the
/// whole function.
struct DefSite {
    block: usize,
    pos: usize,
}

struct Defs {
    param: HashSet<VReg>,
    site: HashMap<VReg, DefSite>,
}

fn collect_defs(func: &Function) -> Result<Defs> {
    let mut param: HashSet<VReg> = HashSet::new();
    let mut site: HashMap<VReg, DefSite> = HashMap::new();
    let mut seen: HashSet<VReg> = HashSet::new();

    for p in &func.params {
        if !seen.insert(p.clone()) {
            return Err(Error::Validate(format!("{p} defined more than once")));
        }
        param.insert(p.clone());
    }
    for (bi, b) in func.blocks.iter().enumerate() {
        for (ii, inst) in b.insts.iter().enumerate() {
            if let Some(d) = inst.def() {
                if !seen.insert(d.clone()) {
                    return Err(Error::Validate(format!("{d} defined more than once")));
                }
                site.insert(d.clone(), DefSite { block: bi, pos: ii });
            }
        }
    }
    Ok(Defs { param, site })
}

fn check_targets(func: &Function, idx: &HashMap<&Label, usize>) -> Result<()> {
    for b in &func.blocks {
        for s in b.term.successors() {
            if !idx.contains_key(&s) {
                return Err(Error::Validate(format!(
                    "block {} jumps to unknown label {s}",
                    b.label
                )));
            }
        }
        for inst in &b.insts {
            if let Inst::Phi { args, .. } = inst {
                for (l, _) in args {
                    if !idx.contains_key(l) {
                        return Err(Error::Validate(format!(
                            "phi in block {} names unknown label {l}",
                            b.label
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

fn reachable_blocks(func: &Function, idx: &HashMap<&Label, usize>) -> BTreeSet<usize> {
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    if func.blocks.is_empty() {
        return seen;
    }
    let mut stack = vec![0usize];
    seen.insert(0);
    while let Some(i) = stack.pop() {
        for s in func.blocks[i].term.successors() {
            if let Some(&j) = idx.get(&s) {
                if seen.insert(j) {
                    stack.push(j);
                }
            }
        }
    }
    seen
}

/// Predecessor block indices for each block, restricted to existing targets.
fn preds_by_index(func: &Function, idx: &HashMap<&Label, usize>) -> Vec<Vec<usize>> {
    let mut preds = vec![Vec::new(); func.blocks.len()];
    for (i, b) in func.blocks.iter().enumerate() {
        for s in b.term.successors() {
            if let Some(&j) = idx.get(&s) {
                preds[j].push(i);
            }
        }
    }
    preds
}

/// Classic iterative dominators over block indices. `dom[i]` includes `i`.
fn dominators(func: &Function, idx: &HashMap<&Label, usize>) -> Vec<BTreeSet<usize>> {
    let n = func.blocks.len();
    let preds = preds_by_index(func, idx);
    let all: BTreeSet<usize> = (0..n).collect();
    let mut dom = vec![all; n];
    if n > 0 {
        dom[0] = BTreeSet::from([0]);
    }

    let mut changed = true;
    while changed {
        changed = false;
        for i in 1..n {
            let mut newset: Option<BTreeSet<usize>> = None;
            for &p in &preds[i] {
                newset = Some(match newset {
                    None => dom[p].clone(),
                    Some(cur) => cur.intersection(&dom[p]).copied().collect(),
                });
            }
            let mut newset = newset.unwrap_or_default();
            newset.insert(i);
            if newset != dom[i] {
                dom[i] = newset;
                changed = true;
            }
        }
    }
    dom
}

fn check_phi_preds(
    func: &Function,
    idx: &HashMap<&Label, usize>,
    reachable: &BTreeSet<usize>,
) -> Result<()> {
    let preds = preds_by_index(func, idx);
    for (bi, b) in func.blocks.iter().enumerate() {
        if !reachable.contains(&bi) {
            continue;
        }
        let pred_set: BTreeSet<usize> = preds[bi].iter().copied().collect();
        for inst in &b.insts {
            if let Inst::Phi { dst, args } = inst {
                let mut entries: BTreeSet<usize> = BTreeSet::new();
                for (l, _) in args {
                    let pi = idx[l];
                    if !entries.insert(pi) {
                        return Err(Error::Validate(format!(
                            "phi {dst} in block {} has a duplicate entry for {l}",
                            b.label
                        )));
                    }
                    if !pred_set.contains(&pi) {
                        return Err(Error::Validate(format!(
                            "phi {dst} in block {} has an entry for {l}, which is not a predecessor",
                            b.label
                        )));
                    }
                }
                for &pi in &pred_set {
                    if !entries.contains(&pi) {
                        return Err(Error::Validate(format!(
                            "phi {dst} in block {} is missing an entry for predecessor {}",
                            b.label, func.blocks[pi].label
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

fn check_uses(
    func: &Function,
    idx: &HashMap<&Label, usize>,
    defs: &Defs,
    doms: &[BTreeSet<usize>],
    reachable: &BTreeSet<usize>,
) -> Result<()> {
    for (bi, b) in func.blocks.iter().enumerate() {
        if !reachable.contains(&bi) {
            continue;
        }
        for (ii, inst) in b.insts.iter().enumerate() {
            match inst {
                Inst::Phi { args, .. } => {
                    for (l, v) in args {
                        let pred = idx[l];
                        check_dominates_pred(func, defs, doms, v, pred)?;
                    }
                }
                _ => {
                    for u in inst.uses() {
                        check_dominates_use(func, defs, doms, &u, bi, ii)?;
                    }
                }
            }
        }
        for u in b.term.uses() {
            check_dominates_use(func, defs, doms, &u, bi, b.insts.len())?;
        }
    }
    Ok(())
}

/// A normal use at `(block, pos)`: the definition must dominate the block, and
/// if it is in the same block it must come strictly earlier.
fn check_dominates_use(
    func: &Function,
    defs: &Defs,
    doms: &[BTreeSet<usize>],
    v: &VReg,
    block: usize,
    pos: usize,
) -> Result<()> {
    if defs.param.contains(v) {
        return Ok(());
    }
    let site = defs
        .site
        .get(v)
        .ok_or_else(|| Error::Validate(format!("use of undefined {v} in block {}", func.blocks[block].label)))?;
    let ok = if site.block == block {
        site.pos < pos
    } else {
        doms[block].contains(&site.block)
    };
    if ok {
        Ok(())
    } else {
        Err(Error::Validate(format!(
            "{v} is used in block {} before its definition dominates the use",
            func.blocks[block].label
        )))
    }
}

/// A phi use: the definition must be available at the exit of the incoming
/// predecessor, i.e. it must dominate that predecessor (defining it there
/// counts).
fn check_dominates_pred(
    func: &Function,
    defs: &Defs,
    doms: &[BTreeSet<usize>],
    v: &VReg,
    pred: usize,
) -> Result<()> {
    if defs.param.contains(v) {
        return Ok(());
    }
    let site = defs
        .site
        .get(v)
        .ok_or_else(|| Error::Validate(format!("phi uses undefined {v}")))?;
    if doms[pred].contains(&site.block) {
        Ok(())
    } else {
        Err(Error::Validate(format!(
            "phi value {v} does not dominate predecessor {}",
            func.blocks[pred].label
        )))
    }
}
