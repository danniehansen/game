use bevy_egui::egui;

use crate::app::state::{ConfirmationAction, MenuState, SaveStore};

use super::{
    modal::{self, ConfirmationChoice},
    worlds::refresh_worlds,
};

pub(super) fn confirmation_ui(ctx: &egui::Context, menu: &mut MenuState, store: &SaveStore) {
    let Some(dialog) = menu.confirmation.as_mut() else {
        return;
    };

    let output = modal::confirmation_modal(
        ctx,
        "confirmation_modal",
        &dialog.title,
        &dialog.body,
        &dialog.confirm_label,
        &dialog.cancel_label,
        !dialog.closing,
    );

    if let Some(choice) = output.choice {
        dialog.closing = true;
        dialog.confirmed = choice == ConfirmationChoice::Confirm;
        ctx.request_repaint();
    }

    if output.finished_closing {
        let Some(dialog) = menu.confirmation.take() else {
            return;
        };

        if dialog.confirmed {
            apply_confirmation_action(dialog.action, menu, store);
        }
    }
}

fn apply_confirmation_action(action: ConfirmationAction, menu: &mut MenuState, store: &SaveStore) {
    match action {
        ConfirmationAction::DeleteWorld { world_id } => match store.0.delete_world(world_id) {
            Ok(()) => refresh_worlds(menu, store),
            Err(error) => menu.status = Some(format!("delete failed: {error}")),
        },
    }
}
