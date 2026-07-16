//! A charge-point entry's target scope within a charging station, shared by the CSMS (server) view
//! — where it keys connected stations' connector entries — and the charging-station (client) view —
//! where it keys the local connector entries multiplexed over the one websocket.

/// A charge-point entry's target scope within a charging station: CS-level (both `None`), a 1.6
/// connector (`{evse: None, connector: Some}`), or a 2.0.1 EVSE/connector (`{evse: Some, connector:
/// Some|None}`). Used as the per-entry key and for action filtering / display.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Scope {
    pub evse: Option<i64>,
    pub connector: Option<i64>,
}

/// The EVSE id an inbound 2.x Call targets: a nested `evse.id` (per the OCPP 2.x schema), falling
/// back to a flat `evseId` (some Calls, e.g. `TransactionEvent`, carry it top-level instead).
/// `None` when neither key is present (a CS-level Call).
pub(crate) fn evse_id(request: &serde_json::Value) -> Option<i64> {
    request["evse"]["id"]
        .as_i64()
        .or_else(|| request["evseId"].as_i64())
}

impl Scope {
    /// The charge-point-wide (CS-level) scope.
    pub const CS: Scope = Scope {
        evse: None,
        connector: None,
    };

    /// A 1.6 connector scope (no EVSE dimension).
    pub fn connector(connector: i64) -> Scope {
        Scope {
            evse: None,
            connector: Some(connector),
        }
    }

    /// A 2.0.1 EVSE scope with an optional connector.
    pub fn evse(evse: i64, connector: Option<i64>) -> Scope {
        Scope {
            evse: Some(evse),
            connector,
        }
    }

    /// Whether this targets a connector/EVSE (vs the CS-level entry).
    pub fn is_connector(&self) -> bool {
        self.evse.is_some() || self.connector.is_some()
    }

    /// Display label for the connection table's connector column: `e{evse}/c{connector}` (2.0.1),
    /// `{connector}` (1.6), or empty (CS-level).
    pub fn label(&self) -> String {
        match (self.evse, self.connector) {
            (None, None) => String::new(),
            (None, Some(c)) => c.to_string(),
            (Some(e), Some(c)) => format!("{e}/{c}"),
            (Some(e), None) => format!("{e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Scope;

    /// OC-R-078 — a message scope labels the charge point, a 1.6 connector, or a 2.0.1 EVSE/connector for display and logging.
    #[test]
    fn ut_scope_label_and_is_connector() {
        assert_eq!(Scope::CS.label(), "");
        assert!(!Scope::CS.is_connector());
        // 1.6 connector: bare connector number.
        assert_eq!(Scope::connector(1).label(), "1");
        assert!(Scope::connector(1).is_connector());
        // 2.0.1 EVSE + connector, and EVSE-only.
        assert_eq!(Scope::evse(1, Some(2)).label(), "1/2");
        assert_eq!(Scope::evse(3, None).label(), "3");
        assert!(Scope::evse(3, None).is_connector());
    }
}
