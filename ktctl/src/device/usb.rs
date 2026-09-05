//! Native USB transport via `rusb`/libusb (roadmap Phase 2).
//!
//! **Unvalidated against hardware.** The interface number and bulk IN/OUT
//! endpoint addresses are open Phase 0 questions — the Android app resolves them
//! at runtime from the claimed interface's descriptors rather than hardcoding
//! them. This module therefore *auto-discovers* a vendor-class interface with a
//! bulk IN + bulk OUT endpoint pair, and lets the operator override any of it
//! via [`UsbConfig`] once a capture pins the real numbers down.

use std::time::Duration;

use rusb::{Context, DeviceHandle, Direction as UsbDir, TransferType, UsbContext};

use super::{DeviceError, Transport, JA11_PID, JA11_VID};

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

/// Walk the active configuration's interfaces to find a vendor-class interface
/// (class `0xFF`) carrying a bulk IN + bulk OUT endpoint pair, honouring any
/// explicit overrides in `config`.
fn resolve_endpoints(
    device: &rusb::Device<Context>,
    desc: &rusb::DeviceDescriptor,
    config: &UsbConfig,
) -> Result<(u8, u8, u8), DeviceError> {
    let config_desc = device
        .config_descriptor(0)
        .map_err(|e| DeviceError::Io(format!("config descriptor: {e}")))?;
    let _ = desc;

    let mut best: Option<(u8, u8, u8, bool)> = None; // (iface, out, in, is_vendor_class)

    for interface in config_desc.interfaces() {
        for iface_desc in interface.descriptors() {
            if let Some(want) = config.interface {
                if iface_desc.interface_number() != want {
                    continue;
                }
            }
            let is_vendor = iface_desc.class_code() == 0xFF;
            let mut ep_out = None;
            let mut ep_in = None;
            for ep in iface_desc.endpoint_descriptors() {
                if ep.transfer_type() != TransferType::Bulk {
                    continue;
                }
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
                    is_vendor,
                );
                // Prefer a vendor-class interface; otherwise take the first fit.
                match &best {
                    None => best = Some(candidate),
                    Some((_, _, _, prev_vendor)) if is_vendor && !prev_vendor => {
                        best = Some(candidate)
                    }
                    _ => {}
                }
            }
        }
    }

    match best {
        Some((iface, o, i, _)) => Ok((iface, o, i)),
        None => Err(DeviceError::Io(
            "no bulk IN/OUT endpoint pair found on any interface".to_string(),
        )),
    }
}
