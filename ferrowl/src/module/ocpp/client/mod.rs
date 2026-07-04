//! OCPP charging-station (client) module: a version-generic backend and a single generic view
//! [`ClientView<V>`] over the [`view::ClientVersion`] trait, with one per-version binding (1.6 /
//! 2.0.1) supplying the version seams.

pub mod backend;
pub mod config;
pub mod lua_sim;
pub mod v1_6;
pub mod v2_0_1;
pub mod v2_1;
pub mod v2_common;
pub mod view;

pub use view::ClientView;

use ferrowl_ocpp::{V1_6, V2_0_1, V2_1};

use crate::module::ocpp::config::device::OcppDeviceConfig;
use crate::module::ocpp::config::session::{OcppSpec, OcppVersion};
use crate::module::view::ModuleView;

/// Build the concrete client view for a spec's OCPP version, carrying the device-config path and
/// the loaded device config (role/version/timeout/scripts).
pub fn build_client_view(
    spec: OcppSpec,
    device_path: String,
    device: OcppDeviceConfig,
) -> Box<dyn ModuleView> {
    match spec.version {
        OcppVersion::V1_6 => Box::new(ClientView::<V1_6>::new(spec, device_path, device)),
        OcppVersion::V2_0_1 => Box::new(ClientView::<V2_0_1>::new(spec, device_path, device)),
        OcppVersion::V2_1 => Box::new(ClientView::<V2_1>::new(spec, device_path, device)),
    }
}
