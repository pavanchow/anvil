//! The small target machine: K physical registers r0..r(K-1) and a stack of
//! spill slots. This is what the backend finally emits, and its interpreter is
//! the second half of the round-trip oracle.

use crate::error::{Error, Result};
use crate::ir::{BinOp, Label};
use std::fmt;

const STEP_LIMIT: u64 = 100_000_000;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TInst {
    Movi { d: usize, imm: i64 },
    Mov { d: usize, s: usize },
    Bin { d: usize, op: BinOp, a: usize, b: usize },
    Load { d: usize, slot: usize },
    Store { slot: usize, s: usize },
}

impl fmt::Display for TInst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TInst::Movi { d, imm } => write!(f, "r{d} = movi {imm}"),
            TInst::Mov { d, s } => write!(f, "r{d} = mov r{s}"),
            TInst::Bin { d, op, a, b } => write!(f, "r{d} = {} r{a}, r{b}", op.name()),
            TInst::Load { d, slot } => write!(f, "r{d} = load slot{slot}"),
            TInst::Store { slot, s } => write!(f, "store slot{slot}, r{s}"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TTerm {
    Jmp(Label),
    Br { cond: usize, then_l: Label, else_l: Label },
    Ret(usize),
}

impl fmt::Display for TTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TTerm::Jmp(l) => write!(f, "jmp {l}"),
            TTerm::Br { cond, then_l, else_l } => write!(f, "br r{cond}, {then_l}, {else_l}"),
            TTerm::Ret(r) => write!(f, "ret r{r}"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TBlock {
    pub label: Label,
    pub insts: Vec<TInst>,
    pub term: TTerm,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TFunc {
    pub name: String,
    pub num_regs: usize,
    pub num_slots: usize,
    /// Physical register each parameter is placed in on entry.
    pub param_regs: Vec<usize>,
    pub blocks: Vec<TBlock>,
}

impl fmt::Display for TFunc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fn {} regs={} slots={} params=[", self.name, self.num_regs, self.num_slots)?;
        for (i, r) in self.param_regs.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "r{r}")?;
        }
        writeln!(f, "] {{")?;
        for b in &self.blocks {
            writeln!(f, "{}:", b.label)?;
            for inst in &b.insts {
                writeln!(f, "  {inst}")?;
            }
            writeln!(f, "  {}", b.term)?;
        }
        write!(f, "}}")
    }
}

pub fn run(tf: &TFunc, args: &[i64]) -> Result<i64> {
    if args.len() != tf.param_regs.len() {
        return Err(Error::Runtime(format!(
            "expected {} args, got {}",
            tf.param_regs.len(),
            args.len()
        )));
    }
    let mut regs = vec![0i64; tf.num_regs.max(1)];
    let mut slots = vec![0i64; tf.num_slots];
    for (r, a) in tf.param_regs.iter().zip(args) {
        regs[*r] = *a;
    }

    let mut cur = tf.blocks[0].label.clone();
    let mut steps = 0u64;
    loop {
        steps += 1;
        if steps > STEP_LIMIT {
            return Err(Error::Runtime("step limit exceeded".into()));
        }
        let block = tf
            .blocks
            .iter()
            .find(|b| b.label == cur)
            .ok_or_else(|| Error::Runtime(format!("no block {cur}")))?;
        for inst in &block.insts {
            match inst {
                TInst::Movi { d, imm } => regs[*d] = *imm,
                TInst::Mov { d, s } => regs[*d] = regs[*s],
                TInst::Bin { d, op, a, b } => regs[*d] = op.eval(regs[*a], regs[*b])?,
                TInst::Load { d, slot } => regs[*d] = slots[*slot],
                TInst::Store { slot, s } => slots[*slot] = regs[*s],
            }
        }
        match &block.term {
            TTerm::Jmp(l) => cur = l.clone(),
            TTerm::Br { cond, then_l, else_l } => {
                cur = if regs[*cond] != 0 {
                    then_l.clone()
                } else {
                    else_l.clone()
                };
            }
            TTerm::Ret(r) => return Ok(regs[*r]),
        }
    }
}
