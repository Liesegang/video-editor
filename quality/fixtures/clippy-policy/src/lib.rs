#![allow(dead_code, reason = "each function is selected by a self-test feature")]

pub fn checked_increment(value: u8) -> Option<u8> {
    value.checked_add(1)
}

#[cfg(test)]
mod tests {
    #[test]
    fn assertion_operations_are_allowed_in_tests() {
        assert_eq!("1".parse::<u8>().unwrap(), 1);
        assert_eq!("2".parse::<u8>().expect("fixture invariant"), 2);
    }

    #[test]
    #[should_panic(expected = "intentional assertion panic")]
    fn panic_is_allowed_in_tests() {
        panic!("intentional assertion panic");
    }
}

#[cfg(feature = "bad-allow-without-reason")]
#[allow(unused_variables)]
fn allow_without_reason(value: u8) {}

#[cfg(feature = "bad-case-sensitive-extension")]
fn case_sensitive_extension(path: &str) -> bool {
    path.ends_with(".mp4")
}

#[cfg(feature = "bad-dbg")]
fn debug_macro() -> u8 {
    dbg!(1)
}

#[cfg(feature = "bad-ignored-result")]
fn ignored_result() {
    let _ = "not a number".parse::<u8>();
}

#[cfg(feature = "bad-large-stack-array")]
fn large_stack_array() -> u8 {
    let bytes = [0_u8; 600_000];
    bytes[0]
}

#[cfg(feature = "bad-large-value")]
fn consume_large_value(bytes: [u8; 1_024]) -> u8 {
    bytes[0]
}

#[cfg(feature = "bad-non-send-field")]
mod non_send_field {
    use std::rc::Rc;

    pub struct IncorrectlySend {
        _value: Rc<u8>,
    }

    // SAFETY: Deliberately false for this executable policy fixture.
    unsafe impl Send for IncorrectlySend {}
}

#[cfg(feature = "bad-redundant-clone")]
fn redundant_clone() -> String {
    let original = String::from("value");
    let cloned = original.clone();
    cloned
}

#[cfg(feature = "bad-string-slice")]
fn string_slice(value: &str) -> &str {
    &value[0..1]
}

#[cfg(feature = "bad-todo")]
fn unfinished() -> u8 {
    todo!()
}

#[cfg(feature = "bad-undocumented-unsafe")]
fn undocumented_unsafe(pointer: *const u8) -> u8 {
    unsafe { *pointer }
}

#[cfg(feature = "bad-unimplemented")]
fn not_implemented() -> u8 {
    unimplemented!()
}

#[cfg(feature = "bad-unreachable")]
fn unreachable_branch(value: bool) -> u8 {
    if value { 1 } else { unreachable!() }
}

#[cfg(feature = "bad-unused-result-ok")]
#[allow(
    unused_must_use,
    reason = "isolate Clippy's unused_result_ok policy in this invalid fixture"
)]
fn unused_result_ok() {
    "not a number".parse::<u8>().ok();
}

#[cfg(feature = "bad-unwrap-in-result")]
fn unwrap_in_result() -> Result<u8, std::num::ParseIntError> {
    Ok("1".parse::<u8>().unwrap())
}

#[cfg(feature = "bad-unwrap")]
fn unwrap_value() -> u8 {
    Some(1).unwrap()
}

#[cfg(feature = "bad-expect")]
fn expect_value() -> u8 {
    Some(1).expect("fixture value")
}

#[cfg(feature = "bad-panic")]
fn panic_value() -> u8 {
    panic!("fixture panic")
}
