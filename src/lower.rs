//! Lowering: rewrite the fully-allocated IR into target machine code. After
//! allocation every vreg has a color, so this is a near one-to-one mapping.
//! Self-moves left by coalesced copies are dropped.

use crate::ir::*;
use crate::regalloc::Allocation;
use crate::target::*;

pub fn lower(alloc: &Allocation) -> TFunc {
    let f = &alloc.func;
    let reg = |v: &VReg| -> usize {
        *alloc
            .coloring
            .get(v)
            .unwrap_or_else(|| panic!("uncolored vreg {v} reached lowering"))
    };

    let blocks = f
        .blocks
        .iter()
        .map(|b| {
            let mut insts = Vec::new();
            for i in &b.insts {
                match i {
                    Inst::Const { dst, val } => insts.push(TInst::Movi {
                        d: reg(dst),
                        imm: *val,
                    }),
                    Inst::Copy { dst, src } => {
                        let (d, s) = (reg(dst), reg(src));
                        if d != s {
                            insts.push(TInst::Mov { d, s });
                        }
                    }
                    Inst::Bin { dst, op, a, b } => insts.push(TInst::Bin {
                        d: reg(dst),
                        op: *op,
                        a: reg(a),
                        b: reg(b),
                    }),
                    Inst::Load { dst, slot } => insts.push(TInst::Load {
                        d: reg(dst),
                        slot: *slot,
                    }),
                    Inst::Store { src, slot } => insts.push(TInst::Store {
                        slot: *slot,
                        s: reg(src),
                    }),
                    Inst::Phi { .. } => unreachable!("phi must be eliminated before lowering"),
                }
            }
            let term = match &b.term {
                Term::Jmp(l) => TTerm::Jmp(l.clone()),
                Term::Br { cond, then_l, else_l } => TTerm::Br {
                    cond: reg(cond),
                    then_l: then_l.clone(),
                    else_l: else_l.clone(),
                },
                Term::Ret(v) => TTerm::Ret(reg(v)),
            };
            TBlock {
                label: b.label.clone(),
                insts,
                term,
            }
        })
        .collect();

    TFunc {
        name: f.name.clone(),
        num_regs: alloc.num_regs,
        num_slots: alloc.num_slots,
        param_regs: f.params.iter().map(reg).collect(),
        blocks,
    }
}
