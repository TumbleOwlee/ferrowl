//! The spec-fixed list of OCPP-J `CallError` codes.

/// OCPP-J error codes carried in a `CallError` frame. This is the fixed set shared by OCPP 1.6
/// and 2.0.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallErrorCode {
    NotImplemented,
    NotSupported,
    InternalError,
    ProtocolError,
    SecurityError,
    FormationViolation,
    PropertyConstraintViolation,
    OccurenceConstraintViolation,
    TypeConstraintViolation,
    GenericError,
}

impl CallErrorCode {
    /// The exact wire spelling of this code.
    pub fn as_str(&self) -> &'static str {
        match self {
            CallErrorCode::NotImplemented => "NotImplemented",
            CallErrorCode::NotSupported => "NotSupported",
            CallErrorCode::InternalError => "InternalError",
            CallErrorCode::ProtocolError => "ProtocolError",
            CallErrorCode::SecurityError => "SecurityError",
            CallErrorCode::FormationViolation => "FormationViolation",
            CallErrorCode::PropertyConstraintViolation => "PropertyConstraintViolation",
            CallErrorCode::OccurenceConstraintViolation => "OccurenceConstraintViolation",
            CallErrorCode::TypeConstraintViolation => "TypeConstraintViolation",
            CallErrorCode::GenericError => "GenericError",
        }
    }

    /// Parse a wire spelling back into a code, falling back to [`CallErrorCode::GenericError`] for
    /// anything unrecognized.
    pub fn from_wire(s: &str) -> Self {
        match s {
            "NotImplemented" => CallErrorCode::NotImplemented,
            "NotSupported" => CallErrorCode::NotSupported,
            "InternalError" => CallErrorCode::InternalError,
            "ProtocolError" => CallErrorCode::ProtocolError,
            "SecurityError" => CallErrorCode::SecurityError,
            "FormationViolation" => CallErrorCode::FormationViolation,
            "PropertyConstraintViolation" => CallErrorCode::PropertyConstraintViolation,
            "OccurenceConstraintViolation" => CallErrorCode::OccurenceConstraintViolation,
            "TypeConstraintViolation" => CallErrorCode::TypeConstraintViolation,
            _ => CallErrorCode::GenericError,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CallErrorCode;

    /// The complete, spec-fixed set of ten codes.
    const ALL: [CallErrorCode; 10] = [
        CallErrorCode::NotImplemented,
        CallErrorCode::NotSupported,
        CallErrorCode::InternalError,
        CallErrorCode::ProtocolError,
        CallErrorCode::SecurityError,
        CallErrorCode::FormationViolation,
        CallErrorCode::PropertyConstraintViolation,
        CallErrorCode::OccurenceConstraintViolation,
        CallErrorCode::TypeConstraintViolation,
        CallErrorCode::GenericError,
    ];

    #[test]
    /// OC-R-012 — the errorCode is one of exactly ten spec-fixed codes, each round-tripping through its wire spelling.
    fn ut_error_codes_are_exactly_ten_and_round_trip() {
        assert_eq!(ALL.len(), 10);
        for code in ALL {
            assert_eq!(CallErrorCode::from_wire(code.as_str()), code);
        }
    }

    #[test]
    /// OC-R-012 — an unrecognized wire code is accepted and mapped to GenericError rather than failing the frame.
    fn ut_unknown_wire_code_maps_to_generic_error() {
        assert_eq!(
            CallErrorCode::from_wire("SomeFutureCode"),
            CallErrorCode::GenericError
        );
        assert_eq!(CallErrorCode::from_wire(""), CallErrorCode::GenericError);
    }
}
