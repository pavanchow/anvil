//! IR interpreter correctness on arithmetic, branches and loops.

mod common;

use anvil::parse::parse_function_str;
use anvil::{run_ir, Error};

fn run(src: &str, args: &[i64]) -> i64 {
    let f = parse_function_str(src).expect("parse");
    run_ir(&f, args).expect("run")
}

#[test]
fn arithmetic() {
    assert_eq!(run(common::ADD, &[2, 3]), 5);
    assert_eq!(run(common::ADD, &[-4, 9]), 5);
    assert_eq!(run(common::ADD, &[100, -100]), 0);
}

#[test]
fn branches() {
    // a+10 truthy -> (a+10)*b ; else 0
    assert_eq!(run(common::BRANCH, &[3, 4]), 13 * 4);
    assert_eq!(run(common::BRANCH, &[-10, 5]), 0); // a+10 == 0 -> else
    assert_eq!(run(common::BRANCH, &[0, 7]), 10 * 7);
}

#[test]
fn sum_loop() {
    assert_eq!(run(common::SUM, &[0]), 0);
    assert_eq!(run(common::SUM, &[1]), 1);
    assert_eq!(run(common::SUM, &[5]), 15);
    assert_eq!(run(common::SUM, &[10]), 55);
    assert_eq!(run(common::SUM, &[100]), 5050);
}

#[test]
fn factorial() {
    assert_eq!(run(common::FACT, &[0]), 1);
    assert_eq!(run(common::FACT, &[1]), 1);
    assert_eq!(run(common::FACT, &[5]), 120);
    assert_eq!(run(common::FACT, &[10]), 3_628_800);
}

#[test]
fn fibonacci() {
    let fib = [0, 1, 1, 2, 3, 5, 8, 13, 21, 34, 55];
    for (n, want) in fib.iter().enumerate() {
        assert_eq!(run(common::FIB, &[n as i64]), *want, "fib({n})");
    }
}

#[test]
fn division_by_zero_is_a_defined_error() {
    let f = parse_function_str(common::DIVZERO).unwrap();
    assert_eq!(run_ir(&f, &[42]), Err(Error::Runtime("division by zero".into())));
}
