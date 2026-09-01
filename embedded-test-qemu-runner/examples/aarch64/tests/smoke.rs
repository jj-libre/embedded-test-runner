#![no_std]
#![no_main]

use aarch64_rt::entry;

// aarch64-rt enters at the fn named by entry!, where embedded-test exports its
// entry as `main`.
entry!(start);
fn start(_: u64, _: u64, _: u64, _: u64) -> ! {
    extern "Rust" {
        fn main() -> !;
    }
    unsafe { main() }
}

#[cfg(test)]
#[embedded_test::tests]
mod tests {
    #[test]
    fn it_passes() {
        assert_eq!(2 + 2, 4);
    }

    #[test]
    fn arithmetic_holds() {
        let xs = [1u32, 2, 3, 4];
        assert_eq!(xs.iter().sum::<u32>(), 10);
    }

    #[test]
    fn it_fails() {
        assert_eq!(2 + 2, 5);
    }

    #[test]
    #[should_panic]
    fn it_panics_as_expected() {
        panic!("boom");
    }

    #[test]
    #[should_panic]
    fn should_panic_but_doesnt() {
        // Should be reported as failure: marked should_panic but exits cleanly.
    }

    #[test]
    #[ignore]
    fn it_is_ignored() {
        assert!(false, "this would fail if executed");
    }

    #[test]
    #[timeout(2)]
    fn it_times_out() {
        loop {}
    }
}
