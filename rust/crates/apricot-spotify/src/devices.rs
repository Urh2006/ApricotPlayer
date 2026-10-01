//! Spotify Connect devices of the account (`docs/SPOTIFY_PLAN.md` 4.2
//! Devices). The list comes from the last cluster this device received
//! (patched `librespot-connect`); Apricot never guesses it.

use librespot_connect::ConnectDevices;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotifyDevice {
    pub id: String,
    pub name: String,
    /// This device plays now.
    pub active: bool,
    /// Apricot's own Connect device.
    pub this_device: bool,
}

/// Devices that can play, Apricot's own first, then by name. Offline
/// devices and devices that cannot play are left out.
pub fn devices(connect: &ConnectDevices, own_id: &str) -> Vec<SpotifyDevice> {
    let mut devices: Vec<SpotifyDevice> = connect
        .devices
        .iter()
        .filter(|device| !device.is_offline && (device.can_play || device.device_id == own_id))
        .map(|device| SpotifyDevice {
            id: device.device_id.clone(),
            name: device.name.clone(),
            active: !connect.active_device_id.is_empty()
                && device.device_id == connect.active_device_id,
            this_device: device.device_id == own_id,
        })
        .collect();
    devices.sort_by(|a, b| {
        b.this_device
            .cmp(&a.this_device)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    devices
}

#[cfg(test)]
mod tests {
    use super::*;
    use librespot_protocol::connect::DeviceInfo;

    fn device(id: &str, name: &str, can_play: bool, offline: bool) -> DeviceInfo {
        let mut device = DeviceInfo::new();
        device.device_id = id.into();
        device.name = name.into();
        device.can_play = can_play;
        device.is_offline = offline;
        device
    }

    #[test]
    fn own_device_first_then_names_without_offline_ones() {
        let connect = ConnectDevices {
            active_device_id: "phone".into(),
            devices: vec![
                device("tv", "Living room", true, false),
                device("old", "Old laptop", true, true),
                device("me", "ApricotPlayer (PC)", true, false),
                device("phone", "iPhone", true, false),
                device("web", "Web", false, false),
            ],
        };
        let list = devices(&connect, "me");
        let names: Vec<_> = list.iter().map(|device| device.name.as_str()).collect();
        assert_eq!(names, ["ApricotPlayer (PC)", "iPhone", "Living room"]);
        assert!(list[0].this_device && !list[0].active);
        assert!(list[1].active);
    }

    #[test]
    fn nothing_plays_without_an_active_device() {
        let connect = ConnectDevices {
            active_device_id: String::new(),
            devices: vec![device("me", "ApricotPlayer", true, false)],
        };
        assert!(!devices(&connect, "me")[0].active);
    }
}
