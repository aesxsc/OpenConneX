use crate::{
    device::{Adapter, Candidate, Endpoint, Link},
    protocol,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::mpsc, time::sleep};
use windows::{
    Devices::Bluetooth::{
        Advertisement::{
            BluetoothLEAdvertisementReceivedEventArgs, BluetoothLEAdvertisementWatcher,
            BluetoothLEAdvertisementWatcherStatus, BluetoothLEScanningMode,
        },
        BluetoothAdapter, BluetoothAddressType, BluetoothCacheMode, BluetoothConnectionStatus,
        BluetoothLEDevice,
        GenericAttributeProfile::{
            GattCharacteristic, GattCharacteristicProperties,
            GattClientCharacteristicConfigurationDescriptorValue, GattCommunicationStatus,
            GattDeviceService, GattSession, GattValueChangedEventArgs, GattWriteOption,
        },
    },
    Devices::{Enumeration::DeviceInformation, Radios::RadioState},
    Foundation::TypedEventHandler,
    Storage::Streams::{DataReader, DataWriter, IBuffer},
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::{GUID, Result as WinResult},
};

const DISCOVERY: GUID = GUID::from_u128(0x0000f300_0000_1000_8000_00805f9b34fb);
const SERVICE: GUID = GUID::from_u128(0x4809f301_1a48_11e9_ab14_d663bd873d93);
const RX: GUID = GUID::from_u128(0x48090001_1a48_11e9_ab14_d663bd873d93);
const TX: GUID = GUID::from_u128(0x48090002_1a48_11e9_ab14_d663bd873d93);
pub type Result<T> = std::result::Result<T, String>;

pub struct Apartment;

impl Apartment {
    pub fn enter() -> Result<Self> {
        // The worker and its current-thread runtime stay on one MTA thread.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(error)?;
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

pub struct Connection {
    device: BluetoothLEDevice,
    service: GattDeviceService,
    session: GattSession,
    tx: GattCharacteristic,
    rx: GattCharacteristic,
    rx_token: Option<i64>,
    notifications: mpsc::Receiver<Vec<u8>>,
    candidate: Option<Candidate>,
    endpoint: Endpoint,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.detach_notifications();
        let _ = self.session.SetMaintainConnection(false);
        let _ = self.service.Close();
        let _ = self.session.Close();
        let _ = self.device.Close();
    }
}

fn error(err: windows::core::Error) -> String {
    err.to_string()
}

fn check(status: GattCommunicationStatus) -> Result<()> {
    match status {
        GattCommunicationStatus::Success => Ok(()),
        GattCommunicationStatus::Unreachable => Err("The Bluetooth control service is unreachable. Check the speaker input and close any other controller.".into()),
        GattCommunicationStatus::AccessDenied => Err("Windows denied Bluetooth access.".into()),
        _ => Err(format!("Bluetooth GATT error ({})", status.0)),
    }
}

fn read_buffer(buffer: &IBuffer) -> WinResult<Vec<u8>> {
    let reader = DataReader::FromBuffer(buffer)?;
    let mut bytes = vec![0; buffer.Length()? as usize];
    reader.ReadBytes(&mut bytes)?;
    Ok(bytes)
}

pub async fn adapter() -> Result<Adapter> {
    let adapter = BluetoothAdapter::GetDefaultAsync()
        .map_err(error)?
        .await
        .map_err(error)?;
    let radio = adapter
        .GetRadioAsync()
        .map_err(error)?
        .await
        .map_err(error)?;
    let info = DeviceInformation::CreateFromIdAsync(&adapter.DeviceId().map_err(error)?)
        .map_err(error)?
        .await
        .ok();
    let radio_name = info
        .and_then(|info| info.Name().ok())
        .or_else(|| radio.Name().ok())
        .unwrap_or_default();
    let radio_state = match radio.State().map_err(error)? {
        RadioState::On => "On",
        RadioState::Off => "Off",
        RadioState::Disabled => "Disabled",
        _ => "Unknown",
    };
    Ok(Adapter {
        address: protocol::address_display(adapter.BluetoothAddress().map_err(error)?),
        name: radio_name.to_string(),
        radio: radio_state.into(),
        supports_le: adapter.IsLowEnergySupported().ok(),
        supports_classic: adapter.IsClassicSupported().ok(),
    })
}

pub async fn scan(duration: Duration) -> Result<Vec<Candidate>> {
    let watcher = BluetoothLEAdvertisementWatcher::new().map_err(error)?;
    watcher
        .SetScanningMode(BluetoothLEScanningMode::Active)
        .map_err(error)?;
    let results = Arc::new(Mutex::new(BTreeMap::<u64, Candidate>::new()));
    let found = results.clone();
    let handler = TypedEventHandler::<
        BluetoothLEAdvertisementWatcher,
        BluetoothLEAdvertisementReceivedEventArgs,
    >::new(move |_, args| {
        let Some(args) = args.as_ref() else {
            return Ok(());
        };
        let ad = args.Advertisement()?;
        let name = ad.LocalName()?.to_string();
        let mr3 = ad.ServiceUuids()?.into_iter().any(|id| id == DISCOVERY);
        let company = ad
            .ManufacturerData()?
            .into_iter()
            .find(|data| data.CompanyId().ok() == Some(2016));
        if !mr3 && company.is_none() && !name.to_ascii_lowercase().starts_with("edifier") {
            return Ok(());
        }
        let address = args.BluetoothAddress()?;
        let random = args.BluetoothAddressType()? == BluetoothAddressType::Random;
        let rssi = args.RawSignalStrengthInDBm()?;
        if let Ok(mut devices) = found.lock() {
            let device = devices.entry(address).or_insert(Candidate {
                endpoint: Endpoint { address, random },
                rssi,
                name: String::new(),
                mr3: false,
                company: None,
                advertisement: Vec::new(),
            });
            device.rssi = rssi;
            if !name.is_empty() {
                device.name = name;
            }
            if let Some(data) = company {
                device.company = Some(data.CompanyId()?);
                device.advertisement = read_buffer(&data.Data()?)?;
            }
            device.mr3 |= mr3;
        }
        Ok(())
    });
    let token = watcher.Received(&handler).map_err(error)?;
    watcher.Start().map_err(error)?;
    sleep(duration).await;
    let scan_status = watcher.Status().map_err(error);
    let _ = watcher.Stop();
    let _ = watcher.RemoveReceived(token);
    if scan_status? == BluetoothLEAdvertisementWatcherStatus::Aborted {
        return Err("Windows stopped the Bluetooth scan. Check that Bluetooth is enabled.".into());
    }
    let mut devices = results
        .lock()
        .map_err(|_| "Bluetooth scan failed")?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    devices.sort_by_key(|device| (!device.mr3, -i32::from(device.rssi)));
    Ok(devices)
}

impl Connection {
    pub async fn open(endpoint: Endpoint, candidate: Option<Candidate>) -> Result<Self> {
        let kind = if endpoint.random {
            BluetoothAddressType::Random
        } else {
            BluetoothAddressType::Public
        };
        let device = BluetoothLEDevice::FromBluetoothAddressWithBluetoothAddressTypeAsync(
            endpoint.address,
            kind,
        )
        .map_err(error)?
        .await
        .map_err(error)?;
        let services = device
            .GetGattServicesForUuidWithCacheModeAsync(SERVICE, BluetoothCacheMode::Uncached)
            .map_err(error)?
            .await
            .map_err(error)?;
        check(services.Status().map_err(error)?)
            .map_err(|error| format!("Service discovery failed. {error}"))?;
        let service = services
            .Services()
            .map_err(error)?
            .GetAt(0)
            .map_err(|_| "The device does not expose the MR3 control service.".to_owned())?;
        let all = service
            .GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Uncached)
            .map_err(error)?
            .await
            .map_err(error)?;
        check(all.Status().map_err(error)?)
            .map_err(|error| format!("Characteristic discovery failed. {error}"))?;
        let find = |uuid| -> Result<GattCharacteristic> {
            all.Characteristics()
                .map_err(error)?
                .into_iter()
                .find(|item| item.Uuid().ok() == Some(uuid))
                .ok_or_else(|| "The speaker control characteristics are missing.".into())
        };
        let rx = find(RX)?;
        let tx = find(TX)?;
        let session = service.Session().map_err(error)?;
        let (sender, notifications) = mpsc::channel(128);
        let handler = TypedEventHandler::<GattCharacteristic, GattValueChangedEventArgs>::new(
            move |_, args| {
                if let Some(args) = args.as_ref() {
                    let _ = sender.try_send(read_buffer(&args.CharacteristicValue()?)?);
                }
                Ok(())
            },
        );
        let rx_token = rx.ValueChanged(&handler).map_err(error)?;
        let connection = Self {
            device,
            service,
            session,
            tx,
            rx,
            rx_token: Some(rx_token),
            notifications,
            candidate,
            endpoint,
        };
        connection
            .session
            .SetMaintainConnection(true)
            .map_err(error)?;
        let status = connection
            .rx
            .WriteClientCharacteristicConfigurationDescriptorAsync(
                GattClientCharacteristicConfigurationDescriptorValue::Notify,
            )
            .map_err(error)?
            .await
            .map_err(error)?;
        check(status).map_err(|error| format!("Notification subscription failed. {error}"))?;
        Ok(connection)
    }

    fn detach_notifications(&mut self) {
        if let Some(token) = self.rx_token.take() {
            let _ = self.rx.RemoveValueChanged(token);
        }
    }

    pub async fn close(mut self) {
        self.detach_notifications();
        // Windows can still have a GATT callback in flight when its token is removed.
        sleep(Duration::from_millis(100)).await;
    }

    pub fn update_candidate(&mut self, candidate: Candidate) {
        self.candidate = Some(candidate);
    }

    pub fn details(&self) -> Link {
        Link {
            endpoint: self.endpoint,
            name: self.device.Name().unwrap_or_default().to_string(),
            att_mtu: self.session.MaxPduSize().unwrap_or(23),
            rssi: self.candidate.as_ref().map(|candidate| candidate.rssi),
            company: self
                .candidate
                .as_ref()
                .and_then(|candidate| candidate.company),
            advertisement: self
                .candidate
                .as_ref()
                .map(|candidate| candidate.advertisement.clone())
                .unwrap_or_default(),
        }
    }

    pub async fn is_connected(&self) -> bool {
        self.device.ConnectionStatus().ok() == Some(BluetoothConnectionStatus::Connected)
    }

    pub async fn receive(&mut self) -> Option<Vec<u8>> {
        self.notifications.recv().await
    }

    pub async fn write(&self, frame: &[u8]) -> Result<()> {
        let mtu = usize::from(self.session.MaxPduSize().map_err(error)?)
            .saturating_sub(3)
            .max(20);
        let properties = self.tx.CharacteristicProperties().map_err(error)?;
        let option = if properties.contains(GattCharacteristicProperties::Write) {
            GattWriteOption::WriteWithResponse
        } else {
            GattWriteOption::WriteWithoutResponse
        };
        for fragment in frame.chunks(mtu) {
            let writer = DataWriter::new().map_err(error)?;
            writer.WriteBytes(fragment).map_err(error)?;
            let data = writer.DetachBuffer().map_err(error)?;
            check(
                self.tx
                    .WriteValueWithOptionAsync(&data, option)
                    .map_err(error)?
                    .await
                    .map_err(error)?,
            )?;
        }
        Ok(())
    }
}

pub fn os_version() -> Option<String> {
    use windows::{
        Wdk::System::SystemServices::RtlGetVersion,
        Win32::System::SystemInformation::OSVERSIONINFOW,
    };
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // The structure is initialized to the documented size and written synchronously.
    let result = unsafe { RtlGetVersion(&mut version) };
    (result.0 >= 0).then(|| {
        format!(
            "{}.{}.{}",
            version.dwMajorVersion, version.dwMinorVersion, version.dwBuildNumber
        )
    })
}
