//! Flag API validation.
use spatial::compile;
fn reject(source: &str, message: &str) {
    let error = compile(source).expect_err("invalid operation must fail");
    assert!(format!("{error:?}").contains(message), "{error:?}");
}

#[test]
fn flag_operations_have_explicit_names_and_arguments() {
    compile("flag a, b export fn f() { set_flags(a, b) set_flags(a) clear_flags() }").unwrap();
    reject(
        "flag a export fn f() { set_flags() }",
        "needs at least one flag",
    );
    reject(
        "flag a export fn f() { clear_flags(a) }",
        "takes no arguments",
    );
    reject(
        "pin p: in export fn f() { set_flags(p) }",
        "only takes flags",
    );
    reject("flag a export fn f() { flags(a) }", "has been renamed");
    reject(
        "flag a export fn f() { set_flag(a) }",
        "individual flag updates",
    );
}
