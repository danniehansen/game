use bevy::prelude::*;

use super::super::state::{ClientSettings, ClientSettingsStore};

pub(crate) fn save_client_settings_system(
    settings: Res<ClientSettings>,
    store: Res<ClientSettingsStore>,
) {
    if !settings.is_changed() {
        return;
    }

    if let Err(error) = store.save(&settings) {
        warn!("could not save client settings: {error}");
    }
}
