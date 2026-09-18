//! Small general-purpose helpers shared across the ferrowl crates:
//! config file (de)serialization ([`convert`]), user-supplied filesystem path expansion
//! ([`path`]), and wall-clock helpers ([`time`]).

pub mod backoff;
pub mod convert;
pub mod path;
pub mod time;
pub mod tls;

/// Trait providing the `panic()` method that calls the given function and panics with the returned
/// message
///
/// This trait exists to provide the same as `expect()` but with the advantage that you have the
/// error available to include the error into the panic message.
///
/// ```rust
/// #[should_panic]
/// use ferrowl_util::Expect;
///
/// let result: Result<(), &'static str> = Ok(());
/// result.panic(|e| format!("{} just happened", e));
/// ```
pub trait Expect<F: FnOnce(Self::Error) -> String> {
    type Value;
    type Error;

    fn panic(self, f: F) -> Self::Value;
}

/// Generic implementation of Expect for any Result type
impl<T, E, F: FnOnce(E) -> String> Expect<F> for Result<T, E> {
    type Value = T;
    type Error = E;
    fn panic(self, f: F) -> Self::Value {
        match self {
            Ok(v) => v,
            Err(e) => panic!("{}", f(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_expect_ok_returns_value() {
        let result: Result<i32, &str> = Ok(42);
        let val = result.panic(|e| format!("error: {e}"));
        assert_eq!(val, 42);
    }

    #[test]
    #[should_panic(expected = "something went wrong: oops")]
    fn ut_expect_err_panics_with_message() {
        let result: Result<i32, &str> = Err("oops");
        result.panic(|e| format!("something went wrong: {e}"));
    }
}
