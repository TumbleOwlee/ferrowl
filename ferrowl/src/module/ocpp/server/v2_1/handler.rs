//! OCPP 2.1 inbound (CS→CSMS) handler for the CSMS role: the shared 2.x handler body
//! ([`define_csms_handler!`](crate::module::ocpp::server::v2_common::define_csms_handler))
//! instantiated for `V2_1`.

use crate::module::ocpp::server::v2_common::define_csms_handler;

define_csms_handler!(V2_1, Action21, Response21);
