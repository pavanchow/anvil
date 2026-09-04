//! Backward dataflow liveness over the CFG. Runs on phi-free IR.
//!
//! live_in[B]  = use[B] ∪ (live_out[B] − def[B])
//! live_out[B] = ∪ live_in[S] for successors S

use crate::ir::*;
use std::collections::{BTreeMap, BTreeSet};

pub struct Liveness {
    pub live_in: BTreeMap<Label, BTreeSet<VReg>>,
    pub live_out: BTreeMap<Label, BTreeSet<VReg>>,
}

pub fn analyze(f: &Function) -> Liveness {
    let mut use_set: BTreeMap<Label, BTreeSet<VReg>> = BTreeMap::new();
    let mut def_set: BTreeMap<Label, BTreeSet<VReg>> = BTreeMap::new();
    for b in &f.blocks {
        let (u, d) = block_use_def(b);
        use_set.insert(b.label.clone(), u);
        def_set.insert(b.label.clone(), d);
    }

    let mut live_in: BTreeMap<Label, BTreeSet<VReg>> = f
        .blocks
        .iter()
        .map(|b| (b.label.clone(), BTreeSet::new()))
        .collect();
    let mut live_out = live_in.clone();

    loop {
        let mut changed = false;
        // Reverse block order converges faster for the common forward CFG.
        for b in f.blocks.iter().rev() {
            let mut out = BTreeSet::new();
            for s in b.term.successors() {
                if let Some(li) = live_in.get(&s) {
                    out.extend(li.iter().cloned());
                }
            }
            let mut inn = use_set[&b.label].clone();
            for v in out.iter() {
                if !def_set[&b.label].contains(v) {
                    inn.insert(v.clone());
                }
            }
            if out != live_out[&b.label] {
                live_out.insert(b.label.clone(), out);
                changed = true;
            }
            if inn != live_in[&b.label] {
                live_in.insert(b.label.clone(), inn);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    Liveness { live_in, live_out }
}

/// Block-local upward-exposed uses and definitions.
fn block_use_def(b: &BasicBlock) -> (BTreeSet<VReg>, BTreeSet<VReg>) {
    let mut used = BTreeSet::new();
    let mut defined = BTreeSet::new();
    for inst in &b.insts {
        for u in inst.uses() {
            if !defined.contains(&u) {
                used.insert(u);
            }
        }
        if let Some(d) = inst.def() {
            defined.insert(d.clone());
        }
    }
    for u in b.term.uses() {
        if !defined.contains(&u) {
            used.insert(u);
        }
    }
    (used, defined)
}
