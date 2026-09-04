//! Interference graph. Two virtual registers interfere when one is live at the
//! point the other is defined, so they cannot share a physical register.

use crate::ir::*;
use crate::liveness::Liveness;
use std::collections::{BTreeMap, BTreeSet};

pub struct InterferenceGraph {
    pub adj: BTreeMap<VReg, BTreeSet<VReg>>,
}

impl InterferenceGraph {
    pub fn degree(&self, v: &VReg) -> usize {
        self.adj.get(v).map_or(0, |s| s.len())
    }

    pub fn neighbors(&self, v: &VReg) -> &BTreeSet<VReg> {
        static EMPTY: BTreeSet<VReg> = BTreeSet::new();
        self.adj.get(v).unwrap_or(&EMPTY)
    }

    fn add_edge(&mut self, a: &VReg, b: &VReg) {
        if a == b {
            return;
        }
        self.adj.entry(a.clone()).or_default().insert(b.clone());
        self.adj.entry(b.clone()).or_default().insert(a.clone());
    }
}

pub fn build(f: &Function, live: &Liveness) -> InterferenceGraph {
    let mut g = InterferenceGraph {
        adj: BTreeMap::new(),
    };
    for v in f.vregs() {
        g.adj.entry(v).or_default();
    }

    // Parameters arrive together in distinct registers, so they form a clique.
    for (i, a) in f.params.iter().enumerate() {
        for b in &f.params[i + 1..] {
            g.add_edge(a, b);
        }
    }

    for b in &f.blocks {
        let mut live_now: BTreeSet<VReg> = live.live_out[&b.label].clone();
        for u in b.term.uses() {
            live_now.insert(u);
        }
        for inst in b.insts.iter().rev() {
            if let Some(d) = inst.def() {
                for v in live_now.iter() {
                    // A copy's source and destination hold the same value at
                    // that point, so they need not interfere.
                    if let Inst::Copy { src, .. } = inst {
                        if v == src {
                            continue;
                        }
                    }
                    if v != d {
                        g.adj.entry(d.clone()).or_default().insert(v.clone());
                        g.adj.entry(v.clone()).or_default().insert(d.clone());
                    }
                }
                live_now.remove(d);
            }
            for u in inst.uses() {
                live_now.insert(u);
            }
        }
    }
    g
}
