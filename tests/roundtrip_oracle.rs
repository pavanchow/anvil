//! The headline correctness property:
//!
//!   interp_ir(program, args) == interp_target(lower(regalloc(program, K)), args)
//!
//! for several programs, across K=2 and K=3 (which force spilling) and a K large
//! enough that no spill happens. If these ever disagree, the backend is wrong.

mod common;

use anvil::parse::parse_function_str;
use anvil::regalloc;
use anvil::ssa;
use anvil::{run_ir, run_target};

const BIG_K: usize = 16;

#[test]
fn ir_and_target_agree_across_k() {
    for (name, src, args_list) in common::programs_with_args() {
        let f = parse_function_str(src).expect("parse");
        for k in [2usize, 3, BIG_K] {
            for args in &args_list {
                let expected = run_ir(&f, args);
                let got = run_target(&f, k, args);
                assert_eq!(
                    expected, got,
                    "mismatch for {name} K={k} args={args:?}: ir={expected:?} target={got:?}"
                );
            }
        }
    }
}

#[test]
fn spilling_actually_occurs_at_low_k() {
    // The sum loop keeps four values live across the loop, so K=2 must spill.
    let f = parse_function_str(common::SUM).unwrap();
    let phi_free = ssa::destruct(&f);
    let alloc = regalloc::allocate(&phi_free, 2).unwrap();
    assert!(
        !alloc.spilled.is_empty(),
        "expected spills at K=2 but got none"
    );
    assert!(alloc.num_slots >= 1, "expected at least one spill slot");
}

#[test]
fn no_spilling_at_large_k() {
    for (name, src, _) in common::programs_with_args() {
        let f = parse_function_str(src).unwrap();
        let phi_free = ssa::destruct(&f);
        let alloc = regalloc::allocate(&phi_free, BIG_K).unwrap();
        assert!(
            alloc.spilled.is_empty(),
            "unexpected spills for {name} at K={BIG_K}: {:?}",
            alloc.spilled
        );
    }
}

#[test]
fn division_by_zero_matches_across_backends() {
    let f = parse_function_str(common::DIVZERO).unwrap();
    for k in [2usize, 3, BIG_K] {
        assert_eq!(run_ir(&f, &[7]), run_target(&f, k, &[7]));
    }
}

#[test]
fn spilling_is_correct_at_k2_and_k3() {
    // Re-assert the property explicitly on the high-pressure programs with a
    // spot check that a real value flows through spill slots.
    let f = parse_function_str(common::SUM).unwrap();
    assert_eq!(run_ir(&f, &[100]).unwrap(), 5050);
    assert_eq!(run_target(&f, 2, &[100]).unwrap(), 5050);
    assert_eq!(run_target(&f, 3, &[100]).unwrap(), 5050);

    let f = parse_function_str(common::FACT).unwrap();
    assert_eq!(run_target(&f, 2, &[10]).unwrap(), 3_628_800);
}
