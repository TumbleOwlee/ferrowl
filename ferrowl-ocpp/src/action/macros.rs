//! The single declarative macro that materializes a whole OCPP version.

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
        pub enum Action { $( $variant($req), )+ }

        /// Full response set for this OCPP version; each variant wraps `rust_ocpp`'s response struct.
        #[derive(Debug, Clone, PartialEq)]
        pub enum Response { $( $variant($resp), )+ }

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
                        Some(Action::$variant(<$req as ::core::default::Default>::default())), )+
                    _ => None,
                }
            }

            fn default_response(name: &str) -> ::core::option::Option<Response> {
                match name {
                    $( n if n == stringify!($variant) =>
                        Some(Response::$variant(<$resp as ::core::default::Default>::default())), )+
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
                        $crate::action::macros::ocpp_validate_arm!($validate, req), )+
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
    };
}

pub(crate) use define_ocpp_version;
