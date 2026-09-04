//! SSA destruction preserves semantics, including the parallel-copy swap case.

mod common;

use anvil::ir::Inst;
use anvil::parse::parse_function_str;
use anvil::{interp, ssa};

fn assert_preserved(src: &str, args_list: &[Vec<i64>]) {
    let f = parse_function_str(src).expect("parse");
    let destructed = ssa::destruct(&f);

    // No phis remain.
    for b in &destructed.blocks {
        assert!(
            !b.insts.iter().any(|i| matches!(i, Inst::Phi { .. })),
            "phi survived destruction in {}",
            b.label
        );
    }

    for args in args_list {
        let before = interp::run(&f, args);
        let after = interp::run(&destructed, args);
        assert_eq!(before, after, "destruction changed result for args {args:?}");
    }
}

#[test]
fn preserves_loops_and_branches() {
    assert_preserved(common::SUM, &[vec![0], vec![5], vec![10]]);
    assert_preserved(common::FACT, &[vec![0], vec![5]]);
    assert_preserved(common::FIB, &[vec![0], vec![1], vec![10]]);
    assert_preserved(common::BRANCH, &[vec![3, 4], vec![-10, 5]]);
}

#[test]
fn handles_phi_swap_cycle() {
    // n even -> x stays 10, n odd -> x becomes 20 (swap each iteration).
    let f = parse_function_str(common::SWAP).unwrap();
    let d = ssa::destruct(&f);
    for n in 0..8 {
        let want = if n % 2 == 0 { 10 } else { 20 };
        assert_eq!(interp::run(&f, &[n]).unwrap(), want, "oracle swap n={n}");
        assert_eq!(interp::run(&d, &[n]).unwrap(), want, "destructed swap n={n}");
    }
}
