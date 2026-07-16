//! OCPP 2.1 binding for the generic charging-station view. The `ClientVersion` method bodies are
//! shared with 2.0.1 as plain free functions in
//! [`v2_common`](crate::module::ocpp::client::v2_common); this `impl` wires in the 2.1 inbound
//! handler. `action_spec`/`json_actions` below point at `spec::v2_1`, which classifies the 2.1-only
//! actions and delegates everything else to `spec::v2_0_1` (the 64 shared actions carry over
//! unchanged).

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use parking_lot::RwLock;

use crate::module::ocpp::action_dialog::ActionSpec;
use crate::module::ocpp::client::backend::Messages;
use crate::module::ocpp::client::v2_0_1::state::CsState;
use crate::module::ocpp::client::v2_1::handler::CsStateHandler;
use crate::module::ocpp::client::v2_common as common;
use crate::module::ocpp::client::view::{
    ClientVersion, EditField, EditKind, EditOverlay, ResolvedEdit,
};
use crate::module::ocpp::config::device::ConnectorRef;
use crate::module::ocpp::scope::Scope;
use ferrowl_ocpp::V2_1;

impl ClientVersion for V2_1 {
    type Cs = CsState;
    type Handler = CsStateHandler;

    fn handler(
        online: Arc<AtomicBool>,
        messages: Messages,
        state: Arc<RwLock<CsState>>,
    ) -> CsStateHandler {
        CsStateHandler::new(online, messages, state)
    }

    fn state_driven() -> &'static [&'static str] {
        &common::STATE_DRIVEN
    }

    fn config_title() -> &'static str {
        common::config_title()
    }

    fn add_connector_placeholder() -> &'static str {
        common::add_connector_placeholder()
    }

    fn has_tx_shortcuts() -> bool {
        common::has_tx_shortcuts()
    }

    fn action_spec(name: &str) -> Option<ActionSpec> {
        crate::module::ocpp::spec::v2_1::action_spec(name)
    }

    fn json_actions() -> &'static [&'static str] {
        crate::module::ocpp::spec::v2_1::json_actions()
    }

    fn json_template(name: &str) -> Option<serde_json::Value> {
        crate::module::ocpp::spec::v2_1::json_template(name)
    }

    fn scope_of(s: &CsState, idx: usize) -> Scope {
        common::scope_of(s, idx)
    }

    fn connector_index(s: &CsState, scope: Scope) -> Option<usize> {
        common::connector_index(s, scope)
    }

    fn connector_index_for_state(s: &CsState, scope: Scope) -> Option<usize> {
        common::connector_index_for_state(s, scope)
    }

    fn add_connector(s: &mut CsState, raw: &str) -> Option<i64> {
        common::add_connector(s, raw)
    }

    fn seed_connector(s: &mut CsState, c: &ConnectorRef) {
        common::seed_connector(s, c)
    }

    fn connector_ref(s: &CsState, idx: usize) -> ConnectorRef {
        common::connector_ref(s, idx)
    }

    fn conn_edit_field(row: usize) -> Option<EditField> {
        common::conn_edit_field(row)
    }

    fn edit_kind(s: &CsState, scope: Scope, cs: bool, field: EditField) -> Option<EditKind> {
        common::edit_kind(s, scope, cs, field)
    }

    fn apply_edit(s: &mut CsState, edit: &EditOverlay, value: ResolvedEdit) {
        common::apply_edit(s, edit, value)
    }

    fn state_payload(s: &CsState, name: &str, scope: Scope) -> serde_json::Value {
        common::state_payload(s, name, scope)
    }

    fn start_event(s: &mut CsState, scope: Scope) -> serde_json::Value {
        common::start_event(s, scope)
    }

    fn stop_event(s: &mut CsState, scope: Scope) -> Option<serde_json::Value> {
        common::stop_event(s, scope)
    }

    fn apply_post_send(
        s: &mut CsState,
        name: &str,
        scope: Scope,
        started_tx: Option<&str>,
        response: &serde_json::Value,
    ) {
        common::apply_post_send(s, name, scope, started_tx, response)
    }

    fn rollback_tx(s: &mut CsState, scope: Scope, started_tx: Option<&str>) {
        common::rollback_tx(s, scope, started_tx)
    }

    fn active_meter_scopes(s: &CsState) -> Vec<Scope> {
        common::active_meter_scopes(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> CsState {
        let mut s = CsState::default();
        s.connectors.clear();
        assert!(s.add_connector(1, 1));
        s
    }

    /// OC-R-059 — the 2.1 binding exposes the shared state-driven set and per-version specs.
    #[test]
    fn ut_static_seams() {
        assert!(<V2_1 as ClientVersion>::state_driven().contains(&"BootNotification"));
        assert_eq!(<V2_1 as ClientVersion>::config_title(), "Variables");
        assert!(<V2_1 as ClientVersion>::has_tx_shortcuts());
        assert!(!<V2_1 as ClientVersion>::json_actions().is_empty());
        assert!(<V2_1 as ClientVersion>::action_spec("GetVariables").is_some());
    }

    /// OC-R-058 — connector-addressing seams delegate to the shared 2.x logic.
    #[test]
    fn ut_connector_seams_delegate() {
        let mut s = state();
        let sc = Scope::evse(1, None);
        assert_eq!(<V2_1 as ClientVersion>::connector_index(&s, sc), Some(0));
        assert_eq!(<V2_1 as ClientVersion>::scope_of(&s, 0), sc);
        assert_eq!(<V2_1 as ClientVersion>::add_connector(&mut s, "2/4"), Some(4));
        assert!(matches!(
            <V2_1 as ClientVersion>::conn_edit_field(0),
            Some(EditField::EvseId)
        ));
        assert!(matches!(
            <V2_1 as ClientVersion>::edit_kind(&s, sc, false, EditField::Voltage),
            Some(EditKind::Number(_))
        ));
    }

    /// OC-R-070 — the transaction-shortcut seams (start/stop/rollback) delegate to shared 2.x logic.
    #[test]
    fn ut_transaction_seams_delegate() {
        let mut s = state();
        let sc = Scope::evse(1, None);
        let ev = <V2_1 as ClientVersion>::start_event(&mut s, sc);
        assert_eq!(ev["eventType"], "Started");
        assert_eq!(
            <V2_1 as ClientVersion>::state_payload(&s, "Heartbeat", sc),
            serde_json::json!({})
        );
        assert!(<V2_1 as ClientVersion>::stop_event(&mut s, sc).is_some());
        assert!(<V2_1 as ClientVersion>::active_meter_scopes(&s).is_empty());
    }
}
