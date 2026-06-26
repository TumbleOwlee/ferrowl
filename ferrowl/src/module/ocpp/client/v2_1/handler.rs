//! OCPP 2.1 inbound (CSMS→CS) handler: the shared 2.x handler body
//! ([`define_cs_state_handler!`](crate::module::ocpp::client::v2_common::define_cs_state_handler))
//! instantiated for `V2_1`, building typed responses from `rust_ocpp::v2_1`.

use crate::module::ocpp::client::v2_common::define_cs_state_handler;

define_cs_state_handler!(V2_1, v2_1, Action21, Response21);
