use crate::{
    device::{Adapter, Candidate, Endpoint, Link},
    protocol,
};
use bluer::{
    AdapterEvent, Address, AddressType, Device, DiscoveryFilter, DiscoveryTransport, Uuid,
    gatt::{
        CharacteristicReader, WriteOp,
        remote::{Characteristic, CharacteristicWriteRequest},
    },
};
use futures_util::StreamExt;
use std::{collections::BTreeMap, pin::pin, time::Duration};
use tokio::time::{sleep, timeout};

type Result<T> = std::result::Result<T, String>;
const EDIFIER_COMPANY: u16 = 2016;

async fn default_adapter() -> Result<bluer::Adapter> {
    let session = bluer::Session::new()
        .await
        .map_err(|error| format!("Could not open the BlueZ system bus. {error}"))?;
    session.default_adapter().await.map_err(|error| {
        format!("Could not find a Bluetooth adapter. Check that BlueZ is running. {error}")
    })
}

pub async fn adapter() -> Result<Adapter> {
    let adapter = default_adapter().await?;
    let address = adapter.address().await.map_err(|error| error.to_string())?;
    let powered = adapter
        .is_powered()
        .await
        .map_err(|error| error.to_string())?;
    Ok(Adapter {
        address: address.to_string(),
        name: adapter
            .alias()
            .await
            .unwrap_or_else(|_| adapter.name().to_owned()),
        radio: if powered { "On" } else { "Off" }.into(),
        ..Adapter::default()
    })
}

async fn candidate(device: &Device, discovery_uuid: Uuid, service_uuid: Uuid) -> Option<Candidate> {
    let rssi = device.rssi().await.ok().flatten();
    // BlueZ may drop advertising properties while an existing GATT link is active.
    if rssi.is_none() && !device.is_connected().await.ok()? {
        return None;
    }
    let name = device.name().await.ok().flatten().unwrap_or_default();
    let mr3 = device
        .uuids()
        .await
        .ok()
        .flatten()
        .is_some_and(|uuids| uuids.contains(&discovery_uuid) || uuids.contains(&service_uuid));
    let advertisement = device
        .manufacturer_data()
        .await
        .ok()
        .flatten()
        .and_then(|mut data| data.remove(&EDIFIER_COMPANY));
    if !mr3 && advertisement.is_none() && !name.to_ascii_lowercase().starts_with("edifier") {
        return None;
    }
    let [a, b, c, d, e, f] = device.address().0;
    Some(Candidate {
        endpoint: Endpoint {
            address: u64::from_be_bytes([0, 0, a, b, c, d, e, f]),
            random: device.address_type().await.ok()? == AddressType::LeRandom,
        },
        rssi,
        name,
        mr3,
        company: advertisement.as_ref().map(|_| EDIFIER_COMPANY),
        advertisement: advertisement.unwrap_or_default(),
    })
}

pub async fn scan(duration: Duration) -> Result<Vec<Candidate>> {
    let adapter = default_adapter().await?;
    if !adapter
        .is_powered()
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("Turn on Bluetooth in your system settings and scan again.".into());
    }
    adapter
        .set_discovery_filter(DiscoveryFilter {
            transport: DiscoveryTransport::Le,
            ..Default::default()
        })
        .await
        .map_err(|error| format!("Could not set the BlueZ discovery filter. {error}"))?;
    let events = adapter
        .discover_devices_with_changes()
        .await
        .map_err(|error| format!("Could not start Bluetooth LE discovery. {error}"))?;
    let mut events = pin!(events);
    let deadline = sleep(duration);
    let mut deadline = pin!(deadline);
    let discovery_uuid = protocol::DISCOVERY_UUID
        .parse()
        .expect("valid discovery UUID");
    let service_uuid = protocol::SERVICE_UUID.parse().expect("valid service UUID");
    let mut candidates = BTreeMap::new();
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            event = events.next() => match event {
                Some(AdapterEvent::DeviceAdded(address)) => {
                    if let Ok(device) = adapter.device(address)
                        && let Ok(Some(found)) = timeout(Duration::from_millis(300), candidate(&device, discovery_uuid, service_uuid)).await
                    {
                        candidates.insert(address, found);
                    }
                }
                Some(AdapterEvent::DeviceRemoved(address)) => {
                    candidates.remove(&address);
                }
                None => return Err("BlueZ stopped Bluetooth discovery.".into()),
                _ => {}
            }
        }
    }
    let mut results = candidates.into_values().collect::<Vec<_>>();
    results.sort_by_key(|device| (!device.mr3, std::cmp::Reverse(device.rssi)));
    Ok(results)
}

pub struct Connection {
    device: Device,
    owned_link: bool,
    link: Link,
    tx: Characteristic,
    write_request: CharacteristicWriteRequest,
    notifications: CharacteristicReader,
}

async fn open_gatt(
    device: &Device,
    connect: bool,
) -> Result<(
    Characteristic,
    CharacteristicWriteRequest,
    CharacteristicReader,
)> {
    if connect {
        device
            .connect()
            .await
            .map_err(|error| format!("BlueZ could not connect to the speaker. {error}"))?;
    }
    let service_uuid = protocol::SERVICE_UUID
        .parse::<Uuid>()
        .expect("valid service UUID");
    let rx_uuid = protocol::RX_UUID
        .parse::<Uuid>()
        .expect("valid notify UUID");
    let tx_uuid = protocol::TX_UUID.parse::<Uuid>().expect("valid write UUID");
    for service in device
        .services()
        .await
        .map_err(|error| format!("GATT service discovery failed. {error}"))?
    {
        if service.uuid().await.ok() != Some(service_uuid) {
            continue;
        }
        let mut rx = None;
        let mut tx = None;
        for characteristic in service
            .characteristics()
            .await
            .map_err(|error| error.to_string())?
        {
            match characteristic.uuid().await {
                Ok(uuid) if uuid == rx_uuid => rx = Some(characteristic),
                Ok(uuid) if uuid == tx_uuid => tx = Some(characteristic),
                _ => {}
            }
        }
        let (Some(rx), Some(tx)) = (rx, tx) else {
            return Err("The MR3 service is missing its control characteristics.".into());
        };
        let flags = tx.flags().await.map_err(|error| error.to_string())?;
        let op_type = if flags.write {
            WriteOp::Request
        } else {
            WriteOp::Command
        };
        let notifications = rx
            .notify_io()
            .await
            .map_err(|error| format!("Could not subscribe to MR3 notifications. {error}"))?;
        return Ok((
            tx,
            CharacteristicWriteRequest {
                op_type,
                ..Default::default()
            },
            notifications,
        ));
    }
    Err("The speaker did not expose the MR3 Bluetooth control service.".into())
}

async fn disconnect(device: &Device) {
    let _ = timeout(Duration::from_secs(1), device.disconnect()).await;
}

impl Connection {
    pub async fn open(endpoint: Endpoint, candidate: Option<Candidate>) -> Result<Self> {
        let adapter = default_adapter().await?;
        let address = Address(endpoint.address.to_be_bytes()[2..].try_into().unwrap());
        let device = adapter.device(address).map_err(|error| error.to_string())?;
        let owned_link = !device.is_connected().await.map_err(|error| {
            format!("BlueZ could not find the speaker. Scan nearby before connecting. {error}")
        })?;
        let result = timeout(Duration::from_secs(14), open_gatt(&device, owned_link))
            .await
            .unwrap_or_else(|_| Err("BlueZ timed out opening the MR3 control service.".into()));
        let (tx, write_request, notifications) = match result {
            Ok(connection) => connection,
            Err(error) => {
                if owned_link {
                    disconnect(&device).await;
                }
                return Err(error);
            }
        };
        let mut connection = Self {
            link: Link {
                endpoint,
                name: device.name().await.ok().flatten().unwrap_or_default(),
                att_mtu: notifications.mtu() as u16,
                rssi: None,
                company: None,
                advertisement: Vec::new(),
            },
            device,
            owned_link,
            tx,
            write_request,
            notifications,
        };
        if let Some(candidate) = candidate {
            connection.update_candidate(candidate);
        }
        Ok(connection)
    }

    pub fn details(&self) -> Link {
        self.link.clone()
    }

    pub fn update_candidate(&mut self, candidate: Candidate) {
        self.link.rssi = candidate.rssi;
        self.link.company = candidate.company;
        self.link.advertisement = candidate.advertisement;
    }

    pub async fn is_connected(&self) -> bool {
        matches!(
            timeout(Duration::from_secs(2), self.device.is_connected()).await,
            Ok(Ok(true))
        )
    }

    pub async fn receive(&mut self) -> Option<Vec<u8>> {
        self.notifications
            .recv()
            .await
            .ok()
            .filter(|bytes| !bytes.is_empty())
    }

    pub async fn write(&self, frame: &[u8]) -> Result<()> {
        let payload_size = self.notifications.mtu().saturating_sub(3).max(20);
        for fragment in frame.chunks(payload_size) {
            self.tx
                .write_ext(fragment, &self.write_request)
                .await
                .map_err(|error| format!("BlueZ could not write to the speaker. {error}"))?;
        }
        Ok(())
    }

    pub async fn close(self) {
        drop(self.notifications);
        if self.owned_link {
            disconnect(&self.device).await;
        }
    }
}

pub fn os_version() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|version| version.trim().to_owned())
}
