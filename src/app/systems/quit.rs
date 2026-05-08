use bevy::{
    app::AppExit,
    prelude::*,
    window::{PrimaryWindow, WindowCloseRequested},
};

use super::super::state::MenuState;

pub(crate) fn app_quit_system(
    mut menu: ResMut<MenuState>,
    primary_window: Query<Entity, With<PrimaryWindow>>,
    mut close_requested: MessageWriter<WindowCloseRequested>,
    mut app_exit: MessageWriter<AppExit>,
) {
    if !menu.quit_requested {
        return;
    }

    menu.quit_requested = false;
    if let Ok(window) = primary_window.single() {
        close_requested.write(WindowCloseRequested { window });
    } else {
        app_exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::window::Window;

    #[test]
    fn quit_request_is_consumed_without_app_exit_when_window_exists() {
        let mut app = App::new();
        app.insert_resource(MenuState {
            quit_requested: true,
            ..Default::default()
        });
        app.world_mut().spawn((PrimaryWindow, Window::default()));
        app.add_message::<WindowCloseRequested>();
        app.add_systems(Update, app_quit_system);

        app.update();

        assert!(!app.world().resource::<MenuState>().quit_requested);
        let close_requests = app.world().resource::<Messages<WindowCloseRequested>>();
        assert_eq!(close_requests.len(), 1);
        let app_exit = app.world().resource::<Messages<AppExit>>();
        assert_eq!(app_exit.len(), 0);
    }
}
