//! Tests for `#[derive(Module)]`.

use ferrowl_lua::module::Module;
use ferrowl_lua_derive::Module;

#[derive(Module)]
#[module = "C_Log"]
struct Log;

#[test]
fn ut_derived_name_matches_attr() {
    assert_eq!(<Log as Module>::module(), "C_Log");
}

// Works through generics, like the real `Log<S>` / `Register<T>`.
#[derive(Module)]
#[module = "C_OCPP"]
struct Ocpp<H> {
    #[allow(dead_code)]
    handle: H,
}

#[test]
fn ut_generic_struct_compiles_and_reports_name() {
    assert_eq!(<Ocpp<()> as Module>::module(), "C_OCPP");
}

// Bounded generic with a where-clause, like `Register<T: Read + Write>`.
trait Marker {}
impl Marker for u8 {}

#[derive(Module)]
#[module = "C_Register"]
struct Register<T>
where
    T: Marker,
{
    #[allow(dead_code)]
    inner: T,
}

#[test]
fn ut_where_clause_generic_reports_name() {
    assert_eq!(<Register<u8> as Module>::module(), "C_Register");
}
