//! Small general-purpose helpers shared across the ferrowl crates:
//! config file (de)serialization ([`convert`]), tracked tokio task spawning
//! ([`tokio`]), and a few ergonomic macros and traits.

pub mod convert;
pub mod tokio;

/// Simple macro to prevent boilerplate of `.to_owned()`
///
/// The macro returns a `String` from the given `&str` value. It removes the boilerplate
/// that normally exists because of various `.to_owned()` calls.
///
/// # Examples
///
/// ```rust
/// use crate::ferrowl_util::str;
///
/// let value: String = str!("Some custom string");
/// ```
#[macro_export]
macro_rules! str {
    ($a:expr) => {
        $a.to_owned()
    };
}

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

/// Clones the listed bindings, then moves the clones into an `async move`
/// block — shorthand for the common "clone before spawning" pattern.
///
/// # Examples
///
/// ```rust
/// use ferrowl_util::async_cloned;
///
/// let name = String::from("ferrowl");
/// let fut = async_cloned!(name; {
///     format!("hello {}", name)
/// });
/// // `name` is still usable here; the future owns a clone.
/// assert_eq!(name, "ferrowl");
/// ```
#[macro_export]
macro_rules! async_cloned {
    ($($n:ident),+; $body:block) => (
        {
            $( let $n = $n.clone(); )+
            async move { $body }
        }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_str_macro_returns_string() {
        let s: String = str!("hello");
        assert_eq!(s, "hello".to_owned());
    }

    #[test]
    fn ut_expect_ok_returns_value() {
        let result: Result<i32, &str> = Ok(42);
        let val = result.panic(|e| format!("error: {}", e));
        assert_eq!(val, 42);
    }

    #[test]
    #[should_panic(expected = "something went wrong: oops")]
    fn ut_expect_err_panics_with_message() {
        let result: Result<i32, &str> = Err("oops");
        result.panic(|e| format!("something went wrong: {}", e));
    }
}
