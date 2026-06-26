//! OCPP 2.1 binding for the generic charging-station view: the shared `ClientVersion` body
//! ([`define_client_version!`](crate::module::ocpp::client::v2_common::define_client_version))
//! instantiated for `V2_1`, reusing 2.0.1's `CsState` and action specs.

use crate::module::ocpp::client::v2_common::define_client_version;

define_client_version!(V2_1, v2_1, v2_0_1);
