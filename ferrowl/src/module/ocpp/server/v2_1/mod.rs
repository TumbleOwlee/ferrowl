//! OCPP 2.1 CSMS (server) binding. 2.1 reuses the 2.0.1 CSMS behaviour, so it shares the observed
//! state types ([`crate::module::ocpp::server::v2_0_1::state`]) and the handler / [`ServerVersion`]
//! bodies ([`crate::module::ocpp::server::v2_common`]), instantiated here for `V2_1`. The CSMS
//! action list and specs are reused from 2.0.1.

pub(crate) mod handler;

use crate::module::ocpp::server::v2_common::define_server_version;

define_server_version!(V2_1, v2_1, v2_0_1);
