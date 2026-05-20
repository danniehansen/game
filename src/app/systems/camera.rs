use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    post_process::{dof::DepthOfField, motion_blur::MotionBlur},
    prelude::*,
    render::camera::TemporalJitter,
};

use crate::{
    controller::{SPRINT_SPEED, WALK_SPEED},
    items::ToolKind,
};

use super::super::{
    EYE_HEIGHT,
    scene::{MainCamera, NetworkPlayer, menu_backdrop_depth_of_field},
    state::{ClientRuntime, MenuState, Screen},
};

const AXE_KICK_PITCH: f32 = 0.010;
const AXE_KICK_DOWN: f32 = 0.005;
const AXE_KICK_DURATION: f32 = 0.08;
const PICKAXE_KICK_PITCH: f32 = 0.038;
const PICKAXE_KICK_DOWN: f32 = 0.024;
const PICKAXE_KICK_DURATION: f32 = 0.18;

// Head bob: walk-speed cadence is ~2 footsteps/sec, which is one full sine
// cycle per second (a step is half a cycle). BOB_FREQ_CYCLES_PER_METER *
// walk_speed ≈ 1.0 cycle/sec keeps the bob in step with the player's gait.
const BOB_FREQ_CYCLES_PER_METER: f32 = 0.192;
// Peak bob displacement at walk speed. Sprint scales up linearly until
// `BOB_AMP_SPEED_CAP_FRACTION` of walk speed, then plateaus so very fast
// motion doesn't shake the camera apart.
const BOB_BASE_AMP_METERS: f32 = 0.012;
const BOB_AMP_SPEED_CAP_FRACTION: f32 = 1.5;
const BOB_AMP_LERP_RATE: f32 = 12.0;

// Sprint FOV: full +SPRINT_FOV_BOOST_DEG when horizontal speed reaches
// SPRINT_SPEED, linear ramp from WALK_SPEED upward. The boost is small on
// purpose — enough to register peripherally without warping the geometry.
const BASE_FOV_DEG: f32 = 65.0;
const SPRINT_FOV_BOOST_DEG: f32 = 5.0;
const FOV_LERP_RATE: f32 = 8.0;

// Landing dip: half-sine pulse on touchdown. Triggered when the player goes
// from airborne to grounded with a downward velocity below the minimum
// trigger, scaled toward the max amplitude at terminal fall speed.
const LANDING_DIP_TRIGGER_SPEED: f32 = 2.0;
const LANDING_DIP_MAX_FALL_SPEED: f32 = 22.0;
const LANDING_DIP_MAX_METERS: f32 = 0.085;
const LANDING_DIP_DURATION: f32 = 0.22;

#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct CameraMotionEffects {
    bob_phase: f32,
    bob_amp_smooth: f32,
    fov_offset_deg: f32,
    was_grounded: bool,
    prev_fall_speed: f32,
    dip_elapsed: f32,
    dip_amplitude: f32,
}

impl Default for CameraMotionEffects {
    fn default() -> Self {
        Self {
            bob_phase: 0.0,
            bob_amp_smooth: 0.0,
            fov_offset_deg: 0.0,
            // Default to "grounded" so the first frame after a session
            // start doesn't trigger a phantom landing dip.
            was_grounded: true,
            prev_fall_speed: 0.0,
            dip_elapsed: 0.0,
            dip_amplitude: 0.0,
        }
    }
}

impl CameraMotionEffects {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn advance(&mut self, dt: f32, horizontal_speed: f32, grounded: bool, fall_speed: f32) {
        let dt = dt.max(0.0);

        let bob_amp_target = if grounded {
            let fraction = (horizontal_speed / WALK_SPEED).min(BOB_AMP_SPEED_CAP_FRACTION);
            fraction * BOB_BASE_AMP_METERS
        } else {
            0.0
        };
        let bob_lerp = (BOB_AMP_LERP_RATE * dt).clamp(0.0, 1.0);
        self.bob_amp_smooth += (bob_amp_target - self.bob_amp_smooth) * bob_lerp;
        if grounded {
            self.bob_phase +=
                horizontal_speed * BOB_FREQ_CYCLES_PER_METER * std::f32::consts::TAU * dt;
            // Keep phase bounded so very long sessions don't lose precision.
            if self.bob_phase > std::f32::consts::TAU * 64.0 {
                self.bob_phase -= std::f32::consts::TAU * 64.0;
            }
        }

        let speed_above_walk = (horizontal_speed - WALK_SPEED).max(0.0);
        let speed_fraction =
            (speed_above_walk / (SPRINT_SPEED - WALK_SPEED).max(f32::EPSILON)).clamp(0.0, 1.0);
        let fov_target = SPRINT_FOV_BOOST_DEG * speed_fraction;
        let fov_lerp = (FOV_LERP_RATE * dt).clamp(0.0, 1.0);
        self.fov_offset_deg += (fov_target - self.fov_offset_deg) * fov_lerp;

        // Landing detection: airborne → grounded transition with enough
        // downward speed to be felt. Use the *previous* frame's fall speed
        // because grounding zeroes vy in the simulator.
        if !self.was_grounded && grounded && self.prev_fall_speed >= LANDING_DIP_TRIGGER_SPEED {
            let intensity = ((self.prev_fall_speed - LANDING_DIP_TRIGGER_SPEED)
                / (LANDING_DIP_MAX_FALL_SPEED - LANDING_DIP_TRIGGER_SPEED))
                .clamp(0.0, 1.0);
            self.dip_amplitude = LANDING_DIP_MAX_METERS * (0.35 + 0.65 * intensity); // small but felt even on light landings
            self.dip_elapsed = 0.0;
        }
        if self.dip_amplitude > 0.0 {
            self.dip_elapsed += dt;
            if self.dip_elapsed >= LANDING_DIP_DURATION {
                self.dip_amplitude = 0.0;
                self.dip_elapsed = 0.0;
            }
        }

        // Cache for the next tick's landing detection.
        self.prev_fall_speed = if grounded { 0.0 } else { fall_speed };
        self.was_grounded = grounded;
    }

    fn bob_offset_y(&self) -> f32 {
        self.bob_phase.sin() * self.bob_amp_smooth
    }

    fn landing_dip_y(&self) -> f32 {
        if self.dip_amplitude <= 0.0 {
            return 0.0;
        }
        let t = (self.dip_elapsed / LANDING_DIP_DURATION).clamp(0.0, 1.0);
        let pulse = (t * std::f32::consts::PI).sin();
        self.dip_amplitude * pulse
    }

    fn fov_radians(&self) -> f32 {
        (BASE_FOV_DEG + self.fov_offset_deg).to_radians()
    }
}

#[derive(Resource, Debug, Default, Clone, Copy)]
pub(crate) struct CameraImpactKick {
    pitch_magnitude: f32,
    down_magnitude: f32,
    duration: f32,
    elapsed: f32,
}

impl CameraImpactKick {
    pub(crate) fn trigger(&mut self, tool: ToolKind) {
        let (pitch, down, duration) = match tool {
            ToolKind::Axe => (AXE_KICK_PITCH, AXE_KICK_DOWN, AXE_KICK_DURATION),
            ToolKind::Pickaxe => (PICKAXE_KICK_PITCH, PICKAXE_KICK_DOWN, PICKAXE_KICK_DURATION),
        };
        // If a previous kick is still decaying, take the stronger of the two so
        // rapid hits accumulate rather than stomp on each other.
        self.pitch_magnitude = self.pitch_magnitude.max(pitch);
        self.down_magnitude = self.down_magnitude.max(down);
        self.duration = duration;
        self.elapsed = 0.0;
    }

    fn advance(&mut self, dt: f32) -> (f32, f32) {
        if self.duration <= 0.0 {
            return (0.0, 0.0);
        }
        self.elapsed += dt.max(0.0);
        if self.elapsed >= self.duration {
            self.pitch_magnitude = 0.0;
            self.down_magnitude = 0.0;
            self.duration = 0.0;
            self.elapsed = 0.0;
            return (0.0, 0.0);
        }
        // Half-sine pulse: ramps in fast, settles smoothly.
        let t = self.elapsed / self.duration;
        let pulse = (t * std::f32::consts::PI).sin();
        (self.pitch_magnitude * pulse, self.down_magnitude * pulse)
    }
}

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

type FollowCameraData = (&'static mut Transform, Option<&'static mut Projection>);
type FollowCameraFilter = (With<MainCamera>, Without<NetworkPlayer>);

pub(crate) fn camera_follow_system(
    runtime: Res<ClientRuntime>,
    menu: Res<MenuState>,
    time: Res<Time>,
    mut kick: ResMut<CameraImpactKick>,
    mut motion: ResMut<CameraMotionEffects>,
    mut camera: Query<FollowCameraData, FollowCameraFilter>,
) {
    if menu.screen != Screen::InGame {
        motion.reset();
        return;
    }

    let Ok((mut camera_transform, projection)) = camera.single_mut() else {
        return;
    };
    let Some(player) = runtime.local_view() else {
        motion.reset();
        return;
    };

    let (pitch_kick, down_kick) = kick.advance(time.delta_secs());

    let (horizontal_speed, fall_speed, grounded) = runtime
        .predicted_local
        .as_ref()
        .map(|controller| {
            let vx = controller.velocity.x;
            let vz = controller.velocity.z;
            let speed = vx.mul_add(vx, vz * vz).sqrt();
            // Positive fall_speed = falling. Velocity.y is downward when
            // negative, so we negate (clamped to 0 on the way up).
            let fall = (-controller.velocity.y).max(0.0);
            (speed, fall, controller.grounded)
        })
        .unwrap_or((0.0, 0.0, true));

    motion.advance(time.delta_secs(), horizontal_speed, grounded, fall_speed);

    let feet = Vec3::new(player.position.x, player.position.y, player.position.z);
    let eye = feet + Vec3::Y * EYE_HEIGHT;
    let bob_y = motion.bob_offset_y();
    let dip_y = motion.landing_dip_y();
    let base_rotation = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
    let rotation = base_rotation * Quat::from_rotation_x(-pitch_kick);
    // Apply the downward drop in world space — feels like the shoulders
    // absorbing the strike without the camera diving along the look vector.
    // Head bob and landing dip stack on the same axis: bob adds a small
    // periodic offset, dip pulls down briefly on touchdown.
    camera_transform.translation = eye + Vec3::Y * (bob_y - dip_y - down_kick);
    camera_transform.rotation = rotation;

    if let Some(mut projection) = projection
        && let Projection::Perspective(perspective) = projection.as_mut()
    {
        perspective.fov = motion.fov_radians();
    }
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
        protocol::{MAX_HEALTH, PlayerState, Vec3Net},
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
            inventory: None,
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
        app.insert_resource(Time::<()>::default());
        app.insert_resource(CameraImpactKick::default());
        app.insert_resource(CameraMotionEffects::default());
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

    #[test]
    fn camera_kick_pulses_and_decays_after_trigger() {
        let mut kick = CameraImpactKick::default();
        assert_eq!(kick.advance(0.1), (0.0, 0.0));

        kick.trigger(ToolKind::Pickaxe);
        let (mid_pitch, mid_drop) = kick.advance(PICKAXE_KICK_DURATION * 0.5);
        assert!(mid_pitch > 0.0);
        assert!(mid_drop > 0.0);

        let (after_pitch, after_drop) = kick.advance(PICKAXE_KICK_DURATION);
        assert_eq!((after_pitch, after_drop), (0.0, 0.0));
    }

    #[test]
    fn motion_effects_reset_to_neutral_state() {
        let mut motion = CameraMotionEffects {
            bob_phase: 1.0,
            bob_amp_smooth: 0.05,
            fov_offset_deg: 3.0,
            was_grounded: false,
            prev_fall_speed: 10.0,
            dip_elapsed: 0.1,
            dip_amplitude: 0.04,
        };

        motion.reset();

        assert_eq!(motion.bob_amp_smooth, 0.0);
        assert_eq!(motion.fov_offset_deg, 0.0);
        assert_eq!(motion.dip_amplitude, 0.0);
        assert!(
            motion.was_grounded,
            "reset should default to grounded so it cannot trigger a phantom dip"
        );
        assert_eq!(motion.bob_offset_y(), 0.0);
        assert_eq!(motion.landing_dip_y(), 0.0);
    }

    #[test]
    fn head_bob_amplitude_scales_with_horizontal_speed_while_grounded() {
        let mut motion = CameraMotionEffects::default();
        // Several short steps so the smoothed amplitude has time to ramp up.
        for _ in 0..40 {
            motion.advance(1.0 / 60.0, WALK_SPEED, true, 0.0);
        }
        let walking_amp = motion.bob_amp_smooth;
        assert!(walking_amp > 0.0);

        let mut sprinting = CameraMotionEffects::default();
        for _ in 0..40 {
            sprinting.advance(1.0 / 60.0, SPRINT_SPEED, true, 0.0);
        }
        assert!(sprinting.bob_amp_smooth > walking_amp);
    }

    #[test]
    fn head_bob_disengages_in_the_air() {
        let mut motion = CameraMotionEffects::default();
        for _ in 0..40 {
            motion.advance(1.0 / 60.0, WALK_SPEED, true, 0.0);
        }
        let grounded_amp = motion.bob_amp_smooth;
        assert!(grounded_amp > 0.0);

        for _ in 0..40 {
            motion.advance(1.0 / 60.0, WALK_SPEED, false, 0.0);
        }
        assert!(motion.bob_amp_smooth < grounded_amp * 0.1);
    }

    #[test]
    fn sprint_fov_offset_ramps_up_with_speed() {
        let mut motion = CameraMotionEffects::default();
        for _ in 0..120 {
            motion.advance(1.0 / 60.0, SPRINT_SPEED, true, 0.0);
        }
        assert!(motion.fov_offset_deg > SPRINT_FOV_BOOST_DEG * 0.85);

        for _ in 0..120 {
            motion.advance(1.0 / 60.0, WALK_SPEED, true, 0.0);
        }
        assert!(motion.fov_offset_deg < 0.05);
    }

    #[test]
    fn landing_dip_triggers_on_fast_touchdown_and_decays() {
        let mut motion = CameraMotionEffects::default();
        // Airborne with a hard downward velocity.
        motion.advance(1.0 / 60.0, 0.0, false, 12.0);
        // Touchdown.
        motion.advance(1.0 / 60.0, 0.0, true, 0.0);
        let initial_dip = motion.landing_dip_y();
        assert!(initial_dip > 0.0);

        for _ in 0..30 {
            motion.advance(1.0 / 60.0, 0.0, true, 0.0);
        }
        assert_eq!(motion.landing_dip_y(), 0.0);
    }

    #[test]
    fn landing_dip_ignores_gentle_touchdowns() {
        let mut motion = CameraMotionEffects::default();
        motion.advance(1.0 / 60.0, 0.0, false, 0.5);
        motion.advance(1.0 / 60.0, 0.0, true, 0.0);
        assert_eq!(motion.landing_dip_y(), 0.0);
    }

    #[test]
    fn pickaxe_kick_is_heavier_than_axe_kick() {
        let mut axe_kick = CameraImpactKick::default();
        axe_kick.trigger(ToolKind::Axe);
        let (axe_peak, _) = axe_kick.advance(AXE_KICK_DURATION * 0.5);

        let mut pickaxe_kick = CameraImpactKick::default();
        pickaxe_kick.trigger(ToolKind::Pickaxe);
        let (pickaxe_peak, _) = pickaxe_kick.advance(PICKAXE_KICK_DURATION * 0.5);

        assert!(pickaxe_peak > axe_peak);
    }
}
