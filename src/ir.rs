//! The SSA intermediate representation and its readable text form.
//!
//! A [`Module`] holds [`Function`]s, each a list of [`BasicBlock`]s over
//! single-assignment virtual registers. `Load`/`Store` are the only variants
//! not written by hand: the register allocator inserts them for spills.

use crate::error::{Error, Result};
use std::collections::{BTreeSet, HashMap};
use std::fmt;

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct VReg(pub String);

impl VReg {
    pub fn new(s: impl Into<String>) -> Self {
        VReg(s.into())
    }
}

impl fmt::Display for VReg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "%{}", self.0)
    }
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Label(pub String);

impl Label {
    pub fn new(s: impl Into<String>) -> Self {
        Label(s.into())
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl BinOp {
    pub fn from_name(s: &str) -> Option<BinOp> {
        Some(match s {
            "add" => BinOp::Add,
            "sub" => BinOp::Sub,
            "mul" => BinOp::Mul,
            "div" => BinOp::Div,
            "mod" => BinOp::Mod,
            "and" => BinOp::And,
            "or" => BinOp::Or,
            "xor" => BinOp::Xor,
            "shl" => BinOp::Shl,
            "shr" => BinOp::Shr,
            "eq" => BinOp::Eq,
            "ne" => BinOp::Ne,
            "lt" => BinOp::Lt,
            "le" => BinOp::Le,
            "gt" => BinOp::Gt,
            "ge" => BinOp::Ge,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            BinOp::Add => "add",
            BinOp::Sub => "sub",
            BinOp::Mul => "mul",
            BinOp::Div => "div",
            BinOp::Mod => "mod",
            BinOp::And => "and",
            BinOp::Or => "or",
            BinOp::Xor => "xor",
            BinOp::Shl => "shl",
            BinOp::Shr => "shr",
            BinOp::Eq => "eq",
            BinOp::Ne => "ne",
            BinOp::Lt => "lt",
            BinOp::Le => "le",
            BinOp::Gt => "gt",
            BinOp::Ge => "ge",
        }
    }

    /// Shared arithmetic used by both interpreters. Because both the IR oracle
    /// and the target machine call this, their results agree by construction;
    /// the round-trip check is really testing register allocation and lowering.
    pub fn eval(self, a: i64, b: i64) -> Result<i64> {
        let v = match self {
            BinOp::Add => a.wrapping_add(b),
            BinOp::Sub => a.wrapping_sub(b),
            BinOp::Mul => a.wrapping_mul(b),
            BinOp::Div => {
                if b == 0 {
                    return Err(Error::Runtime("division by zero".into()));
                }
                a.wrapping_div(b)
            }
            BinOp::Mod => {
                if b == 0 {
                    return Err(Error::Runtime("modulo by zero".into()));
                }
                a.wrapping_rem(b)
            }
            BinOp::And => a & b,
            BinOp::Or => a | b,
            BinOp::Xor => a ^ b,
            BinOp::Shl => a.wrapping_shl(b as u32),
            BinOp::Shr => a.wrapping_shr(b as u32),
            BinOp::Eq => (a == b) as i64,
            BinOp::Ne => (a != b) as i64,
            BinOp::Lt => (a < b) as i64,
            BinOp::Le => (a <= b) as i64,
            BinOp::Gt => (a > b) as i64,
            BinOp::Ge => (a >= b) as i64,
        };
        Ok(v)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Inst {
    Const { dst: VReg, val: i64 },
    Bin { dst: VReg, op: BinOp, a: VReg, b: VReg },
    Copy { dst: VReg, src: VReg },
    Phi { dst: VReg, args: Vec<(Label, VReg)> },
    /// Spill reload, inserted only by the register allocator.
    Load { dst: VReg, slot: usize },
    /// Spill store, inserted only by the register allocator.
    Store { src: VReg, slot: usize },
}

impl Inst {
    pub fn def(&self) -> Option<&VReg> {
        match self {
            Inst::Const { dst, .. }
            | Inst::Bin { dst, .. }
            | Inst::Copy { dst, .. }
            | Inst::Phi { dst, .. }
            | Inst::Load { dst, .. } => Some(dst),
            Inst::Store { .. } => None,
        }
    }

    /// Virtual registers read by this instruction. `Phi` arguments are reported
    /// here for completeness, but liveness runs after phi elimination so they
    /// are never present at that point.
    pub fn uses(&self) -> Vec<VReg> {
        match self {
            Inst::Const { .. } | Inst::Load { .. } => vec![],
            Inst::Bin { a, b, .. } => vec![a.clone(), b.clone()],
            Inst::Copy { src, .. } => vec![src.clone()],
            Inst::Store { src, .. } => vec![src.clone()],
            Inst::Phi { args, .. } => args.iter().map(|(_, v)| v.clone()).collect(),
        }
    }
}

impl fmt::Display for Inst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Inst::Const { dst, val } => write!(f, "{dst} = const {val}"),
            Inst::Bin { dst, op, a, b } => write!(f, "{dst} = {} {a}, {b}", op.name()),
            Inst::Copy { dst, src } => write!(f, "{dst} = copy {src}"),
            Inst::Phi { dst, args } => {
                write!(f, "{dst} = phi [")?;
                for (i, (l, v)) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{l}: {v}")?;
                }
                write!(f, "]")
            }
            Inst::Load { dst, slot } => write!(f, "{dst} = load slot{slot}"),
            Inst::Store { src, slot } => write!(f, "store slot{slot}, {src}"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Term {
    Jmp(Label),
    Br { cond: VReg, then_l: Label, else_l: Label },
    Ret(VReg),
}

impl Term {
    pub fn uses(&self) -> Vec<VReg> {
        match self {
            Term::Jmp(_) => vec![],
            Term::Br { cond, .. } => vec![cond.clone()],
            Term::Ret(v) => vec![v.clone()],
        }
    }

    pub fn successors(&self) -> Vec<Label> {
        match self {
            Term::Jmp(l) => vec![l.clone()],
            Term::Br { then_l, else_l, .. } => vec![then_l.clone(), else_l.clone()],
            Term::Ret(_) => vec![],
        }
    }
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Term::Jmp(l) => write!(f, "jmp {l}"),
            Term::Br { cond, then_l, else_l } => write!(f, "br {cond}, {then_l}, {else_l}"),
            Term::Ret(v) => write!(f, "ret {v}"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BasicBlock {
    pub label: Label,
    pub insts: Vec<Inst>,
    pub term: Term,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Function {
    pub name: String,
    pub params: Vec<VReg>,
    pub blocks: Vec<BasicBlock>,
}

impl Function {
    pub fn entry(&self) -> &Label {
        &self.blocks[0].label
    }

    pub fn block(&self, l: &Label) -> Option<&BasicBlock> {
        self.blocks.iter().find(|b| &b.label == l)
    }

    /// Predecessor labels for every block (empty vec if none).
    pub fn preds(&self) -> HashMap<Label, Vec<Label>> {
        let mut m: HashMap<Label, Vec<Label>> = HashMap::new();
        for b in &self.blocks {
            m.entry(b.label.clone()).or_default();
        }
        for b in &self.blocks {
            for s in b.term.successors() {
                m.entry(s).or_default().push(b.label.clone());
            }
        }
        m
    }

    /// Every virtual register mentioned anywhere in the function.
    pub fn vregs(&self) -> BTreeSet<VReg> {
        let mut s: BTreeSet<VReg> = self.params.iter().cloned().collect();
        for b in &self.blocks {
            for i in &b.insts {
                if let Some(d) = i.def() {
                    s.insert(d.clone());
                }
                for u in i.uses() {
                    s.insert(u);
                }
            }
            for u in b.term.uses() {
                s.insert(u);
            }
        }
        s
    }
}

impl fmt::Display for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fn {}(", self.name)?;
        for (i, p) in self.params.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{p}")?;
        }
        writeln!(f, ") {{")?;
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

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Module {
    pub functions: Vec<Function>,
}

impl Module {
    pub fn function(&self, name: &str) -> Option<&Function> {
        self.functions.iter().find(|f| f.name == name)
    }

    /// The `main` function if present, otherwise the first one.
    pub fn entry_function(&self) -> Option<&Function> {
        self.function("main").or_else(|| self.functions.first())
    }
}

impl fmt::Display for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, func) in self.functions.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
                writeln!(f)?;
            }
            write!(f, "{func}")?;
        }
        Ok(())
    }
}
