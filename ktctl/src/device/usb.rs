//! Native USB transport via `rusb`/libusb (roadmap Phase 2).
//!
//! **Unvalidated against hardware.** Interface/endpoint discovery *is* now
//! resolved from static RE (Phase 0): the Android app scans for the first
//! interface with `bInterfaceClass == 3` (HID) and exactly two endpoints, then
//! picks OUT/IN by direction bit and force-claims it (detaching the kernel HID
//! driver). This module ports that exact heuristic and still lets the operator
//! override any of it via [`UsbConfig`] once a capture confirms the numbers.

use std::time::Duration;

use rusb::{Context, DeviceHandle, Direction as UsbDir, UsbContext};

use super::ids::label;
use super::{DeviceError, Transport, JA11_PID, JA11_VID};

/// A device found on the bus during enumeration.
#[derive(Debug, Clone)]
pub struct FoundDevice {
    /// USB vendor id.
    pub vid: u16,
    /// USB product id.
    pub pid: u16,
    /// USB bus number.
    pub bus: u8,
    /// USB device address.
    pub address: u8,
    /// Human-readable label from the known-device database.
    pub label: String,
}

/// Enumerate all connected USB devices that match a known VID/PID.
///
/// With `only_known == true`, restricts to entries in the device database;
/// otherwise reports every device (labelled "unknown" if not in the database).
pub fn list_devices(only_known: bool) -> Result<Vec<FoundDevice>, DeviceError> {
    let context = Context::new().map_err(|e| DeviceError::Io(e.to_string()))?;
    let devices = context
        .devices()
        .map_err(|e| DeviceError::Io(e.to_string()))?;
    let mut out = Vec::new();
    for device in devices.iter() {
        let Ok(desc) = device.device_descriptor() else {
            continue;
        };
        let (vid, pid) = (desc.vendor_id(), desc.product_id());
        if only_known && super::ids::identify(vid, pid).is_none() {
            continue;
        }
        out.push(FoundDevice {
            vid,
            pid,
            bus: device.bus_number(),
            address: device.address(),
            label: label(vid, pid),
        });
    }
    Ok(out)
}

/// Tunable USB parameters. Defaults auto-discover; override once known.
#[derive(Debug, Clone)]
pub struct UsbConfig {
    /// Vendor id to match.
    pub vid: u16,
    /// Product id to match.
    pub pid: u16,
    /// Force a specific interface number (else auto-detected).
    pub interface: Option<u8>,
    /// Force the bulk OUT endpoint address (else auto-detected).
    pub ep_out: Option<u8>,
    /// Force the bulk IN endpoint address (else auto-detected).
    pub ep_in: Option<u8>,
    /// I/O timeout.
    pub timeout: Duration,
    /// Max reply size to read in one bulk IN transfer.
    pub read_capacity: usize,
}

impl Default for UsbConfig {
    fn default() -> Self {
        UsbConfig {
            vid: JA11_VID,
            pid: JA11_PID,
            interface: None,
            ep_out: None,
            ep_in: None,
            timeout: Duration::from_millis(1000),
            read_capacity: 64,
        }
    }
}

/// A claimed USB vendor-interface transport.
pub struct UsbTransport {
    handle: DeviceHandle<Context>,
    interface: u8,
    ep_out: u8,
    ep_in: u8,
    timeout: Duration,
    read_capacity: usize,
    bus: u8,
    address: u8,
    /// Whether we detached a kernel driver and should reattach on drop.
    detached_kernel_driver: bool,
}

impl UsbTransport {
    /// Open the first matching device using auto-discovery + `config` overrides.
    pub fn open(config: &UsbConfig) -> Result<Self, DeviceError> {
        let context = Context::new().map_err(|e| DeviceError::Io(e.to_string()))?;
        let devices = context
            .devices()
            .map_err(|e| DeviceError::Io(e.to_string()))?;

        for device in devices.iter() {
            let desc = match device.device_descriptor() {
                Ok(d) => d,
                Err(_) => continue,
            };
            if desc.vendor_id() != config.vid || desc.product_id() != config.pid {
                continue;
            }

            let (interface, ep_out, ep_in) = resolve_endpoints(&device, &desc, config)?;

            #[allow(unused_mut)] // `mut` only needed on Linux (detach path)
            let mut handle = device
                .open()
                .map_err(|e| DeviceError::Io(format!("open failed: {e}")))?;

            // On Linux a kernel driver may hold the interface; detach it.
            #[allow(unused_mut)]
            let mut detached = false;
            #[cfg(target_os = "linux")]
            {
                if handle.kernel_driver_active(interface).unwrap_or(false) {
                    handle
                        .detach_kernel_driver(interface)
                        .map_err(|e| DeviceError::Io(format!("detach failed: {e}")))?;
                    detached = true;
                }
            }

            handle
                .claim_interface(interface)
                .map_err(|e| DeviceError::Io(format!("claim interface {interface} failed: {e}")))?;

            return Ok(UsbTransport {
                bus: device.bus_number(),
                address: device.address(),
                handle,
                interface,
                ep_out,
                ep_in,
                timeout: config.timeout,
                read_capacity: config.read_capacity,
                detached_kernel_driver: detached,
            });
        }

        Err(DeviceError::NotFound {
            vid: config.vid,
            pid: config.pid,
        })
    }
}

impl Transport for UsbTransport {
    fn transceive(&mut self, request: &[u8]) -> Result<Vec<u8>, DeviceError> {
        self.handle
            .write_bulk(self.ep_out, request, self.timeout)
            .map_err(|e| DeviceError::Io(format!("bulk OUT failed: {e}")))?;

        let mut buf = vec![0u8; self.read_capacity];
        let n = self
            .handle
            .read_bulk(self.ep_in, &mut buf, self.timeout)
            .map_err(|e| DeviceError::Io(format!("bulk IN failed: {e}")))?;
        buf.truncate(n);
        Ok(buf)
    }

    fn describe(&self) -> String {
        format!(
            "USB JA11 bus {} addr {} iface {} (out {:#04x}/in {:#04x})",
            self.bus, self.address, self.interface, self.ep_out, self.ep_in
        )
    }
}

impl Drop for UsbTransport {
    fn drop(&mut self) {
        let _ = self.handle.release_interface(self.interface);
        #[cfg(target_os = "linux")]
        if self.detached_kernel_driver {
            let _ = self.handle.attach_kernel_driver(self.interface);
        }
        // Silence the unused-field warning on non-Linux where we never reattach.
        let _ = self.detached_kernel_driver;
    }
}

/// USB HID interface class code — the JA11's control interface, per Phase 0.
const HID_CLASS: u8 = 0x03;

/// Walk the active configuration's interfaces to find the one the Android app
/// uses: `bInterfaceClass == 3` (HID) with exactly two endpoints, OUT/IN picked
/// by direction bit. Honours any explicit overrides in `config`, and falls back
/// to any two-endpoint interface if no HID one is present.
fn resolve_endpoints(
    device: &rusb::Device<Context>,
    desc: &rusb::DeviceDescriptor,
    config: &UsbConfig,
) -> Result<(u8, u8, u8), DeviceError> {
    let config_desc = device
        .config_descriptor(0)
        .map_err(|e| DeviceError::Io(format!("config descriptor: {e}")))?;
    let _ = desc;

    let mut best: Option<(u8, u8, u8, bool)> = None; // (iface, out, in, is_hid)

    for interface in config_desc.interfaces() {
        for iface_desc in interface.descriptors() {
            if let Some(want) = config.interface {
                if iface_desc.interface_number() != want {
                    continue;
                }
            }
            let is_hid = iface_desc.class_code() == HID_CLASS;
            // The app keys on exactly two endpoints; enforce that here too.
            if iface_desc.num_endpoints() != 2 {
                continue;
            }
            let mut ep_out = None;
            let mut ep_in = None;
            for ep in iface_desc.endpoint_descriptors() {
                // Endpoints on HID interfaces are typically Interrupt, not Bulk;
                // accept either and let direction decide (the app uses the raw
                // endpoint regardless of declared transfer type).
                match ep.direction() {
                    UsbDir::Out => ep_out = Some(ep.address()),
                    UsbDir::In => ep_in = Some(ep.address()),
                }
            }
            if let (Some(o), Some(i)) = (ep_out, ep_in) {
                let iface = iface_desc.interface_number();
                let candidate = (
                    iface,
                    config.ep_out.unwrap_or(o),
                    config.ep_in.unwrap_or(i),
                    is_hid,
                );
                // Prefer a HID-class interface; otherwise keep the first fit.
                match &best {
                    None => best = Some(candidate),
                    Some((_, _, _, prev_hid)) if is_hid && !prev_hid => best = Some(candidate),
                    _ => {}
                }
            }
        }
    }

    match best {
        Some((iface, o, i, _)) => Ok((iface, o, i)),
        None => Err(DeviceError::Io(
            "no HID-class (or 2-endpoint) interface found to claim".to_string(),
        )),
    }
}
