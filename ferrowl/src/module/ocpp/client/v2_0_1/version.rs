//! OCPP 2.0.1 binding for the generic charging-station view. The `ClientState` surface over
//! `CsState` and the `ClientVersion` body are shared with 2.1 in
//! [`crate::module::ocpp::client::v2_common`]; this module instantiates the `ClientVersion` body
//! for `V2_0_1`, wiring in the 2.0.1 inbound handler and action specs.

use crate::module::ocpp::client::v2_common::define_client_version;

define_client_version!(V2_0_1, v2_0_1, v2_0_1);
