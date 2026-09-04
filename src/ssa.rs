//! SSA destruction: replace phi nodes with copies in predecessors.
//!
//! Two things make this non-trivial and both are handled here:
//!   1. Critical edges (a predecessor with several successors feeding a block
//!      with several predecessors) are split so a copy has a unique home.
//!   2. All copies for one edge happen in parallel. A cycle such as a swap
//!      would clobber if run sequentially, so we sequence with a temporary.

use crate::ir::*;
use std::collections::{BTreeSet, HashMap};

pub fn destruct(func: &Function) -> Function {
    let mut f = func.clone();
    split_critical_edges(&mut f);
    eliminate_phis(&mut f);
    f
}

fn split_critical_edges(f: &mut Function) {
    let mut counter = 0usize;
    loop {
        let preds = f.preds();
        let mut found = None;
        for b in &f.blocks {
            let succs = b.term.successors();
            if succs.len() <= 1 {
                continue;
            }
            for s in succs {
                if preds.get(&s).map_or(0, |v| v.len()) > 1 {
                    found = Some((b.label.clone(), s));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        let (pred, succ) = match found {
            Some(x) => x,
            None => break,
        };

        let mid = Label::new(format!(".edge{counter}"));
        counter += 1;
        f.blocks.push(BasicBlock {
            label: mid.clone(),
            insts: vec![],
            term: Term::Jmp(succ.clone()),
        });

        let pb = f.blocks.iter_mut().find(|b| b.label == pred).unwrap();
        redirect(&mut pb.term, &succ, &mid);

        let sb = f.blocks.iter_mut().find(|b| b.label == succ).unwrap();
        for inst in &mut sb.insts {
            if let Inst::Phi { args, .. } = inst {
                for (l, _) in args.iter_mut() {
                    if *l == pred {
                        *l = mid.clone();
                    }
                }
            }
        }
    }
}

/// Redirect a single edge `from` to `to`, leaving any duplicate edge for a
/// later pass so each split stays one-edge-at-a-time.
fn redirect(t: &mut Term, from: &Label, to: &Label) {
    match t {
        Term::Jmp(l) => {
            if l == from {
                *l = to.clone();
            }
        }
        Term::Br { then_l, else_l, .. } => {
            if then_l == from {
                *then_l = to.clone();
            } else if else_l == from {
                *else_l = to.clone();
            }
        }
        Term::Ret(_) => {}
    }
}

fn eliminate_phis(f: &mut Function) {
    let mut copies_per_pred: HashMap<Label, Vec<(VReg, VReg)>> = HashMap::new();
    for b in &f.blocks {
        for inst in &b.insts {
            if let Inst::Phi { dst, args } = inst {
                for (pred, val) in args {
                    copies_per_pred
                        .entry(pred.clone())
                        .or_default()
                        .push((dst.clone(), val.clone()));
                }
            }
        }
    }

    let mut fresh = 0usize;
    for b in &mut f.blocks {
        if let Some(copies) = copies_per_pred.get(&b.label) {
            let seq = sequence_parallel_copies(copies, &mut fresh);
            for (dst, src) in seq {
                b.insts.push(Inst::Copy { dst, src });
            }
        }
    }

    for b in &mut f.blocks {
        b.insts.retain(|i| !matches!(i, Inst::Phi { .. }));
    }
}

/// Turn a set of simultaneous copies (dst <- src, all dsts distinct) into an
/// ordered sequence. A copy writing `d` must run after every copy that still
/// reads `d`; when only a cycle remains we save one value in a fresh temporary
/// to break it. This is the classic swap-safe sequencing.
fn sequence_parallel_copies(copies: &[(VReg, VReg)], fresh: &mut usize) -> Vec<(VReg, VReg)> {
    let mut pending: Vec<(VReg, VReg)> =
        copies.iter().filter(|(d, s)| d != s).cloned().collect();
    let mut result = Vec::new();

    while !pending.is_empty() {
        let srcs: BTreeSet<VReg> = pending.iter().map(|(_, s)| s.clone()).collect();
        if let Some(pos) = pending.iter().position(|(d, _)| !srcs.contains(d)) {
            result.push(pending.remove(pos));
        } else {
            let (d, _) = pending[0].clone();
            let tmp = VReg::new(format!(".pc{fresh}"));
            *fresh += 1;
            result.push((tmp.clone(), d.clone()));
            for (_, s) in pending.iter_mut() {
                if *s == d {
                    *s = tmp.clone();
                }
            }
        }
    }
    result
}
