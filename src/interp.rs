//! The reference interpreter: the oracle. It runs the SSA IR directly over an
//! unbounded environment of virtual registers. Everything the backend produces
//! is checked against this. It also runs phi-free IR (with spill Load/Store),
//! so the same code validates SSA destruction.

use crate::error::{Error, Result};
use crate::ir::*;
use std::collections::HashMap;

const STEP_LIMIT: u64 = 100_000_000;

pub fn run(func: &Function, args: &[i64]) -> Result<i64> {
    if args.len() != func.params.len() {
        return Err(Error::Runtime(format!(
            "expected {} args, got {}",
            func.params.len(),
            args.len()
        )));
    }
    let mut env: HashMap<VReg, i64> = HashMap::new();
    for (p, a) in func.params.iter().zip(args) {
        env.insert(p.clone(), *a);
    }
    let mut slots: Vec<i64> = Vec::new();
    let mut cur = func.entry().clone();
    let mut prev: Option<Label> = None;
    let mut steps = 0u64;

    loop {
        steps += 1;
        if steps > STEP_LIMIT {
            return Err(Error::Runtime("step limit exceeded".into()));
        }
        let block = func
            .block(&cur)
            .ok_or_else(|| Error::Runtime(format!("no block {cur}")))?;

        // All phis in a block read their incoming values simultaneously, so we
        // evaluate them against the pre-existing environment before committing.
        let mut phi_updates = Vec::new();
        for inst in &block.insts {
            if let Inst::Phi { dst, args } = inst {
                let p = prev
                    .as_ref()
                    .ok_or_else(|| Error::Runtime("phi reached without a predecessor".into()))?;
                let (_, val) = args
                    .iter()
                    .find(|(l, _)| l == p)
                    .ok_or_else(|| Error::Runtime(format!("phi has no entry for pred {p}")))?;
                phi_updates.push((dst.clone(), get(&env, val)?));
            }
        }
        for (d, v) in phi_updates {
            env.insert(d, v);
        }

        for inst in &block.insts {
            match inst {
                Inst::Phi { .. } => {}
                Inst::Const { dst, val } => {
                    env.insert(dst.clone(), *val);
                }
                Inst::Bin { dst, op, a, b } => {
                    let v = op.eval(get(&env, a)?, get(&env, b)?)?;
                    env.insert(dst.clone(), v);
                }
                Inst::Copy { dst, src } => {
                    let v = get(&env, src)?;
                    env.insert(dst.clone(), v);
                }
                Inst::Load { dst, slot } => {
                    let v = *slots
                        .get(*slot)
                        .ok_or_else(|| Error::Runtime(format!("load from unset slot{slot}")))?;
                    env.insert(dst.clone(), v);
                }
                Inst::Store { src, slot } => {
                    let v = get(&env, src)?;
                    if *slot >= slots.len() {
                        slots.resize(*slot + 1, 0);
                    }
                    slots[*slot] = v;
                }
            }
        }

        match &block.term {
            Term::Jmp(l) => {
                prev = Some(cur.clone());
                cur = l.clone();
            }
            Term::Br { cond, then_l, else_l } => {
                let c = get(&env, cond)?;
                prev = Some(cur.clone());
                cur = if c != 0 { then_l.clone() } else { else_l.clone() };
            }
            Term::Ret(v) => return get(&env, v),
        }
    }
}

fn get(env: &HashMap<VReg, i64>, v: &VReg) -> Result<i64> {
    env.get(v)
        .copied()
        .ok_or_else(|| Error::Runtime(format!("use of undefined {v}")))
}
