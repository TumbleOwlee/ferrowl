//! The single declarative macro that materializes a whole OCPP version.

// `define_ocpp_version!` interleaves a generated `#[cfg(test)]` drift-check module with the
// test-only helpers (`ocpp_validate_flag!`, `validate_probe`, `ocpp_has_validate!`) that support
// it; that's intentional, not accidental item ordering.
#![allow(clippy::items_after_test_module)]

/// Expand to the per-variant validation arm. `rust_ocpp` derives `validator::Validate` on only
/// some request types, so each table row declares `yes` (call `Validate::validate`) or `no`
/// (skip). This is fully deterministic — no specialization tricks — and degrades safely if a
/// request gains a `Validate` impl upstream (we simply keep skipping it until the table is
/// regenerated).
macro_rules! ocpp_validate_arm {
    (yes, $req:expr) => {
        ::validator::Validate::validate($req).map_err($crate::error::ValidationError)
    };
    (no, $req:expr) => {{
        let _ = $req;
        ::core::result::Result::Ok(())
    }};
}

pub(crate) use ocpp_validate_arm;

/// Generate a version's `Action`/`Response` enums plus its [`Version`](crate::action::Version)
/// implementation from a table of `Variant => RequestPath, ResponsePath, validate;` rows.
///
/// The variant identifier doubles as the wire action name (via `stringify!`), which matches
/// `rust_ocpp`'s struct naming for every action, so no separate wire-name literal is needed. The
/// request/response paths use `rust_ocpp`'s real (snake_case) module spelling, which can differ
/// from the wire name (e.g. module `heart_beat`, variant/wire `Heartbeat`). The trailing `yes`/`no`
/// flag selects whether the request type's `validator::Validate` impl is invoked.
macro_rules! define_ocpp_version {
    (
        $version_ty:ident, $subprotocol:literal,
        cs = [ $( $cs_variant:ident ),* $(,)? ];
        csms = [ $( $csms_variant:ident => $scope:ident ),* $(,)? ];
        $( $variant:ident => $req:path, $resp:path, $validate:tt ; )+
    ) => {
        /// Full action set for this OCPP version; each variant wraps `rust_ocpp`'s request struct.
        #[derive(Debug, Clone, PartialEq)]
        pub enum Action { $( $variant(Box<$req>), )+ }

        /// Full response set for this OCPP version; each variant wraps `rust_ocpp`'s response struct.
        #[derive(Debug, Clone, PartialEq)]
        pub enum Response { $( $variant(Box<$resp>), )+ }

        /// Zero-sized marker type implementing [`Version`](crate::action::Version).
        #[derive(Debug, Clone, Copy, Default)]
        pub struct $version_ty;

        impl $crate::action::Version for $version_ty {
            type Action = Action;
            type Response = Response;

            fn action_name(action: &Action) -> &'static str {
                match action { $( Action::$variant(_) => stringify!($variant), )+ }
            }

            fn action_names() -> &'static [&'static str] {
                &[ $( stringify!($variant), )+ ]
            }

            fn cs_actions() -> &'static [&'static str] {
                &[ $( stringify!($cs_variant), )* ]
            }

            fn csms_actions() -> &'static [(&'static str, $crate::action::ConnectorScope)] {
                &[ $( (stringify!($csms_variant), $crate::action::ConnectorScope::$scope), )* ]
            }

            fn default_action(name: &str) -> ::core::option::Option<Action> {
                match name {
                    $( n if n == stringify!($variant) =>
                        Some(Action::$variant(Box::default())), )+
                    _ => None,
                }
            }

            fn default_response(name: &str) -> ::core::option::Option<Response> {
                match name {
                    $( n if n == stringify!($variant) =>
                        Some(Response::$variant(Box::default())), )+
                    _ => None,
                }
            }

            fn subprotocol() -> &'static str { $subprotocol }

            fn decode_call(action_name: &str, payload: ::serde_json::Value)
                -> ::core::result::Result<Action, $crate::error::OcppError>
            {
                match action_name {
                    $( name if name == stringify!($variant) =>
                        Ok(Action::$variant(::serde_json::from_value(payload)?)), )+
                    other => Err($crate::error::OcppError::UnknownAction(other.to_owned())),
                }
            }

            fn validate(action: &Action)
                -> ::core::result::Result<(), $crate::error::ValidationError>
            {
                match action {
                    $( Action::$variant(req) =>
                        $crate::action::macros::ocpp_validate_arm!($validate, req.as_ref()), )+
                }
            }

            fn encode_response(response: &Response)
                -> ::core::result::Result<::serde_json::Value, $crate::error::OcppError>
            {
                match response {
                    $( Response::$variant(resp) => Ok(::serde_json::to_value(resp)?), )+
                }
            }

            fn encode_action(action: &Action)
                -> ::core::result::Result<::serde_json::Value, $crate::error::OcppError>
            {
                match action {
                    $( Action::$variant(req) => Ok(::serde_json::to_value(req)?), )+
                }
            }

            fn decode_result(action: &Action, payload: ::serde_json::Value)
                -> ::core::result::Result<Response, $crate::error::OcppError>
            {
                match action {
                    $( Action::$variant(_) =>
                        Ok(Response::$variant(::serde_json::from_value(payload)?)), )+
                }
            }
        }

        // Catches drift between each row's hand-maintained `yes`/`no` validate flag and whether
        // the request type actually derives `validator::Validate` (see `ocpp_validate_arm!`'s
        // doc comment — this exact drift bit 24 v2.0.1 actions previously).
        #[cfg(test)]
        mod ut_validate_flags {
            #[test]
            /// OC-R-008 — each action's validate flag matches whether its request type actually derives `Validate`,
            /// so the version's validation rules are applied to exactly the actions that have them.
            fn ut_validate_flag_matches_request_type() {
                $(
                    assert_eq!(
                        $crate::action::macros::ocpp_validate_flag!($validate),
                        $crate::action::macros::ocpp_has_validate!($req),
                        "{}: table says validate = {}, but request type's actual Validate impl says {}",
                        stringify!($variant),
                        stringify!($validate),
                        $crate::action::macros::ocpp_has_validate!($req),
                    );
                )+
            }
        }
    };
}

pub(crate) use define_ocpp_version;

/// Expand the `yes`/`no` table tag to the matching `bool` literal, for tests that compare the
/// hand-maintained flag against the request type's actual `validator::Validate` impl.
#[cfg(test)]
macro_rules! ocpp_validate_flag {
    (yes) => {
        true
    };
    (no) => {
        false
    };
}

#[cfg(test)]
pub(crate) use ocpp_validate_flag;

/// Autoref-specialization probe: detects, for a concrete type, whether it implements
/// `validator::Validate` — without nightly specialization. This only resolves correctly when
/// `$t` is concrete at the expansion site (trait selection for an unresolved generic parameter
/// can't be specialized this way), which holds here since `define_ocpp_version!` expands one
/// arm per action with `$req` bound to a concrete `rust_ocpp` path.
///
/// `(&<$t>::default()).__ferrowl_probe_validate()` picks whichever of the two `Probe*` impls is
/// more specific for `$t`: `ProbeValidate` (blanket on bare `T: Validate`) is an exact by-value
/// match and wins over `ProbeDefault` (blanket on `&T`, reached only via extra autoref), so it
/// resolves to `ProbeValidate` exactly when `$t: Validate`.
#[cfg(test)]
pub(crate) mod validate_probe {
    pub trait ProbeDefault {
        fn __ferrowl_probe_validate(&self) -> bool {
            false
        }
    }
    impl<T> ProbeDefault for &T {}

    pub trait ProbeValidate {
        fn __ferrowl_probe_validate(&self) -> bool {
            true
        }
    }
    impl<T: ::validator::Validate> ProbeValidate for T {}
}

#[cfg(test)]
macro_rules! ocpp_has_validate {
    ($t:ty) => {{
        #[allow(unused_imports)]
        use $crate::action::macros::validate_probe::{ProbeDefault, ProbeValidate};
        (&<$t>::default()).__ferrowl_probe_validate()
    }};
}

#[cfg(test)]
pub(crate) use ocpp_has_validate;
