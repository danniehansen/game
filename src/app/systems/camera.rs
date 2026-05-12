use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    post_process::{dof::DepthOfField, motion_blur::MotionBlur},
    prelude::*,
    render::camera::TemporalJitter,
};

use super::super::{
    EYE_HEIGHT,
    scene::{MainCamera, NetworkPlayer, menu_backdrop_depth_of_field},
    state::{ClientRuntime, MenuState, Screen},
};

const MENU_BACKDROP_EYE: Vec3 = Vec3::new(-5.8, EYE_HEIGHT, 7.2);
const MENU_BACKDROP_LOOK_AT: Vec3 = Vec3::new(0.4, 0.85, -3.6);
const MENU_BACKDROP_PAN_SPEED: f32 = 0.055;
const MENU_BACKDROP_PAN_RADIUS: Vec3 = Vec3::new(0.42, 0.035, 0.32);
const MENU_BACKDROP_LOOK_RADIUS: Vec3 = Vec3::new(0.22, 0.03, 0.18);

type MenuBackdropCameraData = (
    Entity,
    &'static mut Transform,
    &'static mut Msaa,
    Option<&'static DepthOfField>,
    Option<&'static TemporalAntiAliasing>,
    Option<&'static TemporalJitter>,
    Option<&'static MotionBlur>,
);
type MenuBackdropCameraFilter = (With<MainCamera>, Without<NetworkPlayer>);

pub(crate) fn menu_backdrop_camera_system(
    mut commands: Commands,
    menu: Res<MenuState>,
    time: Option<Res<Time>>,
    mut camera: Query<MenuBackdropCameraData, MenuBackdropCameraFilter>,
) {
    let Ok((
        entity,
        mut camera_transform,
        mut msaa,
        depth_of_field,
        temporal_aa,
        temporal_jitter,
        motion_blur,
    )) = camera.single_mut()
    else {
        return;
    };

    if menu.screen == Screen::InGame {
        if *msaa != Msaa::Sample4 {
            *msaa = Msaa::Sample4;
        }
        if depth_of_field.is_some()
            || temporal_aa.is_some()
            || temporal_jitter.is_some()
            || motion_blur.is_some()
        {
            commands.entity(entity).remove::<(
                DepthOfField,
                TemporalAntiAliasing,
                TemporalJitter,
                MotionBlur,
            )>();
        }
        return;
    }

    if *msaa != Msaa::Off {
        *msaa = Msaa::Off;
    }
    let elapsed_seconds = time
        .as_ref()
        .map(|time| time.elapsed_secs())
        .unwrap_or_default();
    *camera_transform = menu_backdrop_transform(elapsed_seconds);
    if depth_of_field.is_none() {
        commands
            .entity(entity)
            .insert(menu_backdrop_depth_of_field());
    }
}

pub(crate) fn camera_follow_system(
    runtime: Res<ClientRuntime>,
    menu: Res<MenuState>,
    mut camera: Query<&mut Transform, (With<MainCamera>, Without<NetworkPlayer>)>,
) {
    if menu.screen != Screen::InGame {
        return;
    }

    let Ok(mut camera_transform) = camera.single_mut() else {
        return;
    };
    let Some(player) = runtime.local_view() else {
        return;
    };

    let feet = Vec3::new(player.position.x, player.position.y, player.position.z);
    let eye = feet + Vec3::Y * EYE_HEIGHT;
    camera_transform.translation = eye;
    camera_transform.rotation = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
}

fn menu_backdrop_transform(elapsed_seconds: f32) -> Transform {
    let phase = elapsed_seconds * MENU_BACKDROP_PAN_SPEED;
    let eye = MENU_BACKDROP_EYE
        + Vec3::new(
            phase.sin() * MENU_BACKDROP_PAN_RADIUS.x,
            (phase * 0.7).sin() * MENU_BACKDROP_PAN_RADIUS.y,
            phase.cos() * MENU_BACKDROP_PAN_RADIUS.z,
        );
    let look_at = MENU_BACKDROP_LOOK_AT
        + Vec3::new(
            (phase * 0.65).cos() * MENU_BACKDROP_LOOK_RADIUS.x,
            (phase * 0.5).sin() * MENU_BACKDROP_LOOK_RADIUS.y,
            (phase * 0.8).sin() * MENU_BACKDROP_LOOK_RADIUS.z,
        );
    Transform::from_translation(eye).looking_at(look_at, Vec3::Y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        controller::PlayerController,
        protocol::{MAX_HEALTH, PlayerInventoryState, PlayerState, Vec3Net},
    };
    use bevy::post_process::dof::DepthOfFieldMode;

    #[derive(Resource, Default)]
    struct MsaaChangeLog(Vec<bool>);

    fn app_with_camera(menu: MenuState) -> App {
        let mut app = App::new();
        app.insert_resource(menu);
        app.add_systems(Startup, |mut commands: Commands| {
            commands.spawn((
                MainCamera,
                Camera3d::default(),
                Msaa::Off,
                menu_backdrop_depth_of_field(),
                Transform::from_xyz(0.0, EYE_HEIGHT, 3.0),
            ));
        });
        app.add_systems(Update, menu_backdrop_camera_system);
        app
    }

    fn record_msaa_change(
        mut changes: ResMut<MsaaChangeLog>,
        camera: Query<Ref<Msaa>, With<MainCamera>>,
    ) {
        let Ok(msaa) = camera.single() else {
            return;
        };
        changes.0.push(msaa.is_changed());
    }

    fn player_state(position: Vec3Net, yaw: f32, pitch: f32) -> PlayerState {
        PlayerState {
            client_id: 1,
            steam_id: 1,
            name: "Player 1".to_owned(),
            position,
            velocity: Vec3Net::ZERO,
            yaw,
            pitch,
            health: MAX_HEALTH,
            grounded: true,
            last_processed_input: 0,
            is_admin: false,
            inventory: PlayerInventoryState::default(),
        }
    }

    #[test]
    fn menu_backdrop_camera_sets_soft_panning_world_view() {
        let mut app = app_with_camera(MenuState::default());
        app.update();
        app.update();

        let mut query = app
            .world_mut()
            .query_filtered::<(&Transform, &DepthOfField), With<MainCamera>>();
        let (transform, depth_of_field) = query
            .single(app.world())
            .expect("menu camera should have dof");

        assert!(transform.translation.distance(MENU_BACKDROP_EYE) <= 0.6);
        assert_eq!(depth_of_field.mode, DepthOfFieldMode::Gaussian);
        assert!(depth_of_field.aperture_f_stops < 1.0);
    }

    #[test]
    fn gameplay_camera_removes_depth_of_field() {
        let menu = MenuState {
            screen: Screen::InGame,
            ..Default::default()
        };
        let mut app = app_with_camera(menu);
        app.update();

        let camera = app
            .world_mut()
            .query_filtered::<Entity, With<MainCamera>>()
            .single(app.world())
            .expect("camera should exist");
        app.world_mut().entity_mut(camera).insert((
            TemporalAntiAliasing::default(),
            TemporalJitter::default(),
            MotionBlur::default(),
        ));

        app.update();

        let mut query = app
            .world_mut()
            .query_filtered::<&DepthOfField, With<MainCamera>>();
        assert!(query.single(app.world()).is_err());
        let mut query = app
            .world_mut()
            .query_filtered::<&TemporalAntiAliasing, With<MainCamera>>();
        assert!(query.single(app.world()).is_err());
        let mut query = app
            .world_mut()
            .query_filtered::<&TemporalJitter, With<MainCamera>>();
        assert!(query.single(app.world()).is_err());
        let mut query = app
            .world_mut()
            .query_filtered::<&MotionBlur, With<MainCamera>>();
        assert!(query.single(app.world()).is_err());
    }

    #[test]
    fn gameplay_camera_uses_multisample_antialiasing() {
        let mut app = app_with_camera(MenuState::default());
        app.update();

        app.world_mut().resource_mut::<MenuState>().screen = Screen::InGame;
        app.update();

        let mut query = app.world_mut().query_filtered::<&Msaa, With<MainCamera>>();
        let msaa = query.single(app.world()).expect("camera should use msaa");
        assert_eq!(*msaa, Msaa::Sample4);
    }

    #[test]
    fn gameplay_camera_msaa_settles_after_transition() {
        let menu = MenuState {
            screen: Screen::InGame,
            ..Default::default()
        };
        let mut app = app_with_camera(menu);
        app.insert_resource(MsaaChangeLog::default());
        app.add_systems(
            Update,
            record_msaa_change.after(menu_backdrop_camera_system),
        );

        app.update();
        app.update();

        let changes = app.world().resource::<MsaaChangeLog>();
        assert_eq!(changes.0, vec![true, false]);
    }

    #[test]
    fn menu_camera_keeps_msaa_off_for_depth_of_field() {
        let mut app = app_with_camera(MenuState::default());
        app.update();

        let mut query = app.world_mut().query_filtered::<&Msaa, With<MainCamera>>();
        let msaa = query.single(app.world()).expect("camera should use msaa");
        assert_eq!(*msaa, Msaa::Off);
    }

    #[test]
    fn gameplay_camera_uses_predicted_pose_as_single_source() {
        let mut app = App::new();
        let yaw = 0.8;
        let pitch = -0.2;
        app.insert_resource(MenuState {
            screen: Screen::InGame,
            ..Default::default()
        });
        app.insert_resource(ClientRuntime {
            predicted_local: Some(PlayerController::from_player_state(&player_state(
                Vec3Net::new(2.0, 1.0, -3.0),
                yaw,
                pitch,
            ))),
            ..Default::default()
        });
        app.world_mut().spawn((
            MainCamera,
            Transform::from_xyz(0.0, 0.0, 0.0),
            GlobalTransform::default(),
        ));
        app.add_systems(Update, camera_follow_system);

        app.update();

        let mut query = app
            .world_mut()
            .query_filtered::<&Transform, With<MainCamera>>();
        let transform = query.single(app.world()).expect("camera transform");
        assert_eq!(
            transform.translation,
            Vec3::new(2.0, 1.0 + EYE_HEIGHT, -3.0)
        );
        assert!(
            transform
                .rotation
                .abs_diff_eq(Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0), 0.0001)
        );
    }
}
