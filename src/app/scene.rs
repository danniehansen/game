use bevy::prelude::*;

use crate::{
    protocol::ClientId,
    world::{FLOOR_SIZE, TEST_WORLD_BLOCKS},
};

use super::{EYE_HEIGHT, PLAYER_VISUAL_CENTER_Y};

const LOCAL_PLAYER_COLOR: Color = Color::srgb(0.25, 0.68, 0.95);
const REMOTE_PLAYER_COLOR: Color = Color::srgb(0.95, 0.61, 0.25);
const WORLD_COLOR: Color = Color::srgb(0.18, 0.34, 0.22);

#[derive(Resource, Clone)]
pub(crate) struct PlayerVisualAssets {
    pub(crate) mesh: Handle<Mesh>,
    pub(crate) local_material: Handle<StandardMaterial>,
    pub(crate) remote_material: Handle<StandardMaterial>,
}

#[derive(Component)]
pub(crate) struct NetworkPlayer {
    pub(crate) client_id: ClientId,
}

#[derive(Component)]
pub(crate) struct TargetPosition(pub(crate) Vec3);

#[derive(Component)]
pub(crate) struct TargetRotation(pub(crate) Quat);

#[derive(Component)]
pub(crate) struct MainCamera;

pub(crate) fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.72, 0.78, 0.86),
        brightness: 90.0,
        ..default()
    });

    commands.spawn((
        Name::new("Camera"),
        MainCamera,
        Camera3d::default(),
        Projection::from(PerspectiveProjection {
            fov: 65.0_f32.to_radians(),
            ..default()
        }),
        Transform::from_xyz(0.0, EYE_HEIGHT, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        Name::new("Sun"),
        DirectionalLight {
            illuminance: 16_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(-3.0, 8.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        Name::new("Authoritative Plane"),
        Mesh3d(
            meshes.add(
                Plane3d::default()
                    .mesh()
                    .size(FLOOR_SIZE, FLOOR_SIZE)
                    .subdivisions(16),
            ),
        ),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: WORLD_COLOR,
            perceptual_roughness: 0.9,
            cull_mode: None,
            ..default()
        })),
    ));

    let block_materials = [
        materials.add(Color::srgb(0.46, 0.50, 0.48)),
        materials.add(Color::srgb(0.55, 0.48, 0.38)),
        materials.add(Color::srgb(0.36, 0.44, 0.55)),
        materials.add(Color::srgb(0.48, 0.40, 0.52)),
    ];
    for (index, block) in TEST_WORLD_BLOCKS.iter().enumerate() {
        let size = block.size();
        commands.spawn((
            Name::new(format!("Test Cube {}", index + 1)),
            Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))),
            MeshMaterial3d(block_materials[index % block_materials.len()].clone()),
            Transform::from_xyz(block.center.x, block.center.y, block.center.z),
        ));
    }

    commands.insert_resource(PlayerVisualAssets {
        mesh: meshes.add(Capsule3d::new(0.35, 0.9)),
        local_material: materials.add(LOCAL_PLAYER_COLOR),
        remote_material: materials.add(REMOTE_PLAYER_COLOR),
    });
}

pub(crate) fn player_visual_position(feet_position: Vec3) -> Vec3 {
    feet_position + Vec3::Y * PLAYER_VISUAL_CENTER_Y
}
