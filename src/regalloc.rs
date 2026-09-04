//! Chaitin-Briggs graph-coloring register allocation.
//!
//! Pipeline per round: build the interference graph, `simplify` low-degree
//! nodes onto a stack (optimistically pushing a spill candidate when stuck),
//! then `select` colors in reverse. A node that cannot be colored is an actual
//! spill: it gets a stack slot, the code is rewritten with loads and stores
//! around every use and definition, and the whole round runs again. Because
//! spill reloads are tiny live ranges, the process converges for any K >= 2.
//!
//! Two subtleties keep it terminating and correct at small K:
//!   - A spilled parameter has no defining instruction, so its incoming value
//!     is stored to its slot at the very top of the entry block.
//!   - Temporaries introduced for a spill are never spilled again; if one still
//!     cannot be colored, K is genuinely too small and we report that.

use crate::error::{Error, Result};
use crate::interference::{self, InterferenceGraph};
use crate::ir::*;
use crate::liveness;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Location {
    Reg(usize),
    Slot(usize),
}

pub struct Allocation {
    /// The function after spill rewriting; every remaining vreg is colored.
    pub func: Function,
    pub coloring: BTreeMap<VReg, usize>,
    /// Original vregs that were spilled, and the slot each received.
    pub slots: BTreeMap<VReg, usize>,
    pub num_regs: usize,
    pub num_slots: usize,
    pub spilled: Vec<VReg>,
    pub rounds: usize,
}

impl Allocation {
    /// Location of an original-program vreg for reporting.
    pub fn location(&self, v: &VReg) -> Option<Location> {
        if let Some(slot) = self.slots.get(v) {
            Some(Location::Slot(*slot))
        } else {
            self.coloring.get(v).map(|c| Location::Reg(*c))
        }
    }
}

pub fn allocate(func: &Function, k: usize) -> Result<Allocation> {
    if k < 2 {
        return Err(Error::Alloc("need at least 2 registers".into()));
    }
    let params: BTreeSet<VReg> = func.params.iter().cloned().collect();
    if params.len() > k {
        return Err(Error::Alloc(format!(
            "cannot allocate: {} parameters exceed {k} registers",
            params.len()
        )));
    }
    let mut f = func.clone();
    let mut slot_of: BTreeMap<VReg, usize> = BTreeMap::new();
    let mut next_slot = 0usize;
    let mut spilled_all: Vec<VReg> = Vec::new();
    // Spill reload temporaries; never re-spilled.
    let mut protected: BTreeSet<VReg> = BTreeSet::new();
    let mut fresh = 0usize;
    let mut rounds = 0usize;

    loop {
        rounds += 1;
        if rounds > 10_000 {
            return Err(Error::Alloc("spilling did not converge".into()));
        }
        let live = liveness::analyze(&f);
        let ig = interference::build(&f, &live);
        match color(&ig, k, &params, &protected) {
            Ok(coloring) => {
                return Ok(Allocation {
                    func: f,
                    coloring,
                    slots: slot_of,
                    num_regs: k,
                    num_slots: next_slot,
                    spilled: spilled_all,
                    rounds,
                });
            }
            Err(spills) => {
                if spills.iter().any(|s| protected.contains(s)) {
                    return Err(Error::Alloc(format!(
                        "cannot allocate with {k} registers: instruction needs more live values than registers"
                    )));
                }
                for s in &spills {
                    slot_of.entry(s.clone()).or_insert_with(|| {
                        let n = next_slot;
                        next_slot += 1;
                        n
                    });
                    spilled_all.push(s.clone());
                }
                let (nf, new_temps) = rewrite_spills(&f, &spills, &slot_of, &params, &mut fresh);
                protected.extend(new_temps);
                f = nf;
            }
        }
    }
}

/// Optimistic coloring. Returns a full coloring, or the set of nodes that could
/// not be colored (the actual spills).
fn color(
    ig: &InterferenceGraph,
    k: usize,
    params: &BTreeSet<VReg>,
    protected: &BTreeSet<VReg>,
) -> std::result::Result<BTreeMap<VReg, usize>, Vec<VReg>> {
    let mut remaining: BTreeSet<VReg> = ig.adj.keys().cloned().collect();
    let mut degree: BTreeMap<VReg, usize> =
        ig.adj.iter().map(|(v, n)| (v.clone(), n.len())).collect();
    let mut stack: Vec<VReg> = Vec::new();

    while !remaining.is_empty() {
        let low = remaining.iter().find(|v| degree[*v] < k).cloned();
        let node = match low {
            Some(n) => n,
            None => pick_spill(&remaining, &degree, params, protected),
        };
        remaining.remove(&node);
        for m in ig.neighbors(&node) {
            if remaining.contains(m) {
                if let Some(d) = degree.get_mut(m) {
                    *d = d.saturating_sub(1);
                }
            }
        }
        stack.push(node);
    }

    let mut coloring: BTreeMap<VReg, usize> = BTreeMap::new();
    let mut actual: Vec<VReg> = Vec::new();
    while let Some(node) = stack.pop() {
        let used: BTreeSet<usize> = ig
            .neighbors(&node)
            .iter()
            .filter_map(|m| coloring.get(m).copied())
            .collect();
        match (0..k).find(|c| !used.contains(c)) {
            Some(c) => {
                coloring.insert(node, c);
            }
            None => actual.push(node),
        }
    }

    if actual.is_empty() {
        Ok(coloring)
    } else {
        Err(actual)
    }
}

/// Pick a spill candidate. Prefer ordinary values, then parameters, and only as
/// a last resort a spill temporary. Within a tier, spill the highest degree.
fn pick_spill(
    remaining: &BTreeSet<VReg>,
    degree: &BTreeMap<VReg, usize>,
    params: &BTreeSet<VReg>,
    protected: &BTreeSet<VReg>,
) -> VReg {
    remaining
        .iter()
        .max_by_key(|v| {
            let tier = if protected.contains(*v) {
                0
            } else if params.contains(*v) {
                1
            } else {
                2
            };
            (tier, degree[*v])
        })
        .cloned()
        .unwrap()
}

/// Rewrite the function so every spilled value lives in its slot: reload before
/// each use, store after each definition. Returns the new function and the
/// temporaries created (which must not be spilled again).
fn rewrite_spills(
    f: &Function,
    spills: &[VReg],
    slot_of: &BTreeMap<VReg, usize>,
    params: &BTreeSet<VReg>,
    fresh: &mut usize,
) -> (Function, Vec<VReg>) {
    let spill_set: BTreeSet<VReg> = spills.iter().cloned().collect();
    let mut created: Vec<VReg> = Vec::new();
    let mut nf = f.clone();

    for b in &mut nf.blocks {
        let mut out: Vec<Inst> = Vec::new();
        for inst in &b.insts {
            let mut inst = inst.clone();
            let mut subst: BTreeMap<VReg, VReg> = BTreeMap::new();
            for u in inst.uses() {
                if spill_set.contains(&u) && !subst.contains_key(&u) {
                    let t = mk_fresh(fresh, &mut created);
                    out.push(Inst::Load {
                        dst: t.clone(),
                        slot: slot_of[&u],
                    });
                    subst.insert(u, t);
                }
            }
            rename_uses(&mut inst, &subst);
            match inst.def().cloned() {
                Some(d) if spill_set.contains(&d) => {
                    let t = mk_fresh(fresh, &mut created);
                    rename_def(&mut inst, &t);
                    out.push(inst);
                    out.push(Inst::Store {
                        src: t,
                        slot: slot_of[&d],
                    });
                }
                _ => out.push(inst),
            }
        }
        let mut subst: BTreeMap<VReg, VReg> = BTreeMap::new();
        for u in b.term.uses() {
            if spill_set.contains(&u) && !subst.contains_key(&u) {
                let t = mk_fresh(fresh, &mut created);
                out.push(Inst::Load {
                    dst: t.clone(),
                    slot: slot_of[&u],
                });
                subst.insert(u, t);
            }
        }
        rename_term_uses(&mut b.term, &subst);
        b.insts = out;
    }

    // A spilled parameter has no defining instruction, so store the incoming
    // value to its slot at the very top of the entry block. The parameter keeps
    // a tiny live range there and is colored to the register it arrives in.
    let spilled_params: Vec<VReg> = f
        .params
        .iter()
        .filter(|p| spill_set.contains(*p))
        .cloned()
        .collect();
    if !spilled_params.is_empty() {
        if let Some(entry) = nf.blocks.first_mut() {
            for p in spilled_params.iter().rev() {
                if params.contains(p) {
                    entry.insts.insert(
                        0,
                        Inst::Store {
                            src: p.clone(),
                            slot: slot_of[p],
                        },
                    );
                }
            }
        }
    }

    (nf, created)
}

fn mk_fresh(fresh: &mut usize, created: &mut Vec<VReg>) -> VReg {
    let v = VReg::new(format!(".sp{fresh}"));
    *fresh += 1;
    created.push(v.clone());
    v
}

fn rename_uses(inst: &mut Inst, subst: &BTreeMap<VReg, VReg>) {
    let sub = |v: &mut VReg| {
        if let Some(n) = subst.get(v) {
            *v = n.clone();
        }
    };
    match inst {
        Inst::Bin { a, b, .. } => {
            sub(a);
            sub(b);
        }
        Inst::Copy { src, .. } => sub(src),
        Inst::Store { src, .. } => sub(src),
        Inst::Phi { args, .. } => {
            for (_, v) in args {
                sub(v);
            }
        }
        Inst::Const { .. } | Inst::Load { .. } => {}
    }
}

fn rename_def(inst: &mut Inst, t: &VReg) {
    match inst {
        Inst::Const { dst, .. }
        | Inst::Bin { dst, .. }
        | Inst::Copy { dst, .. }
        | Inst::Phi { dst, .. }
        | Inst::Load { dst, .. } => *dst = t.clone(),
        Inst::Store { .. } => {}
    }
}

fn rename_term_uses(term: &mut Term, subst: &BTreeMap<VReg, VReg>) {
    match term {
        Term::Br { cond, .. } => {
            if let Some(n) = subst.get(cond) {
                *cond = n.clone();
            }
        }
        Term::Ret(v) => {
            if let Some(n) = subst.get(v) {
                *v = n.clone();
            }
        }
        Term::Jmp(_) => {}
    }
}
