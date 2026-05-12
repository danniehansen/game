use std::collections::HashMap;

use bevy::prelude::*;

use crate::{
    app::{
        EYE_HEIGHT,
        scene::{
            HeldItemVisual, ItemVisualAssets, MainCamera, NetworkDroppedItem, NetworkResourceNode,
            ResourceVisualAssets,
        },
        state::{ClientRuntime, GatherInputState, LookState, MenuState, PickupTargetState, Screen},
    },
    items::{ItemModel, item_definition, pickup_anchor, pickup_anchor_from_position, pickup_score},
    protocol::{DroppedWorldItem, QuatNet, ResourceNodeState},
    resources::{
        ResourceNodeModel, best_resource_node_target, resource_node_anchor,
        resource_node_definition,
    },
};

use std::f32::consts::PI;

const HELD_ITEM_FORWARD_OFFSET: f32 = 0.62;
const HELD_ITEM_RIGHT_OFFSET: f32 = 0.28;
const HELD_ITEM_DOWN_OFFSET: f32 = 0.24;
const DROPPED_ITEM_INTERPOLATION_SECONDS: f32 = 0.1;
const DROPPED_ITEM_INTERPOLATION_SNAP_DISTANCE: f32 = 6.0;

pub(crate) fn apply_dropped_items_system(
    mut commands: Commands,
    time: Res<Time>,
    runtime: Res<ClientRuntime>,
    assets: Res<ItemVisualAssets>,
    mut dropped_entities: Query<(
        Entity,
        &NetworkDroppedItem,
        &Transform,
        &mut DroppedItemInterpolation,
    )>,
) {
    let Some(snapshot) = &runtime.snapshot else {
        for (entity, _, _, _) in &dropped_entities {
            commands.entity(entity).despawn();
        }
        return;
    };

    let existing = dropped_entities
        .iter()
        .map(|(entity, dropped, _, _)| (dropped.id, entity))
        .collect::<HashMap<_, _>>();

    for item in &snapshot.dropped_items {
        let target = dropped_item_transform(item);
        if let Some(entity) = existing.get(&item.id) {
            if let Ok((_, _, current, mut interpolation)) = dropped_entities.get_mut(*entity) {
                interpolation.retarget(snapshot.tick, current, target);
                let transform = interpolation.advance(time.delta_secs());
                commands.entity(*entity).insert((transform,));
            }
        } else {
            commands.spawn((
                Name::new(format!("Dropped Item {}", item.id)),
                NetworkDroppedItem { id: item.id },
                DroppedItemInterpolation::new(snapshot.tick, target),
                Mesh3d(assets.dropped_mesh.clone()),
                MeshMaterial3d(assets.dropped_material.clone()),
                target,
                Visibility::Visible,
            ));
        }
    }

    for (entity, dropped, _, _) in &dropped_entities {
        if !snapshot
            .dropped_items
            .iter()
            .any(|item| item.id == dropped.id)
        {
            commands.entity(entity).despawn();
        }
    }
}

pub(crate) fn apply_resource_nodes_system(
    mut commands: Commands,
    runtime: Res<ClientRuntime>,
    assets: Res<ResourceVisualAssets>,
    resource_entities: Query<(Entity, &NetworkResourceNode)>,
) {
    let Some(snapshot) = &runtime.snapshot else {
        for (entity, _) in &resource_entities {
            commands.entity(entity).despawn();
        }
        return;
    };

    let existing = resource_entities
        .iter()
        .map(|(entity, resource)| (resource.id, entity))
        .collect::<HashMap<_, _>>();

    for node in &snapshot.resource_nodes {
        let Some(definition) = resource_node_definition(&node.definition_id) else {
            continue;
        };
        let transform = resource_node_transform(node, definition.model);
        if let Some(entity) = existing.get(&node.id) {
            commands.entity(*entity).insert((transform,));
        } else {
            let (mesh, material) = resource_node_visual(&assets, node, definition.model);
            commands.spawn((
                Name::new(format!("Resource Node {}", node.id)),
                NetworkResourceNode { id: node.id },
                Mesh3d(mesh),
                MeshMaterial3d(material),
                transform,
                Visibility::Visible,
            ));
        }
    }

    for (entity, resource) in &resource_entities {
        if !snapshot
            .resource_nodes
            .iter()
            .any(|node| node.id == resource.id)
        {
            commands.entity(entity).despawn();
        }
    }
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct DroppedItemInterpolation {
    snapshot_tick: u64,
    from: Transform,
    to: Transform,
    elapsed: f32,
}

impl DroppedItemInterpolation {
    fn new(snapshot_tick: u64, transform: Transform) -> Self {
        Self {
            snapshot_tick,
            from: transform,
            to: transform,
            elapsed: DROPPED_ITEM_INTERPOLATION_SECONDS,
        }
    }

    fn retarget(&mut self, snapshot_tick: u64, current: &Transform, target: Transform) {
        if snapshot_tick <= self.snapshot_tick {
            return;
        }

        let distance = current.translation.distance(target.translation);
        self.from = if distance > DROPPED_ITEM_INTERPOLATION_SNAP_DISTANCE {
            target
        } else {
            *current
        };
        self.to = target;
        self.elapsed = 0.0;
        self.snapshot_tick = snapshot_tick;
    }

    fn advance(&mut self, delta_seconds: f32) -> Transform {
        self.elapsed += delta_seconds.max(0.0);
        let alpha = (self.elapsed / DROPPED_ITEM_INTERPOLATION_SECONDS).clamp(0.0, 1.0);
        Transform::from_translation(self.from.translation.lerp(self.to.translation, alpha))
            .with_rotation(self.from.rotation.slerp(self.to.rotation, alpha))
    }
}

pub(crate) fn update_pickup_target_system(
    runtime: Res<ClientRuntime>,
    look: Res<LookState>,
    menu: Res<MenuState>,
    camera: Query<(&Camera, &Transform), With<MainCamera>>,
    dropped_entities: Query<(&NetworkDroppedItem, &Transform)>,
    mut pickup_target: ResMut<PickupTargetState>,
) {
    if menu.screen != Screen::InGame || menu.pause_open || menu.inventory_open || menu.chat_open {
        pickup_target.clear();
        return;
    }

    let Some(snapshot) = &runtime.snapshot else {
        pickup_target.clear();
        return;
    };
    let Some(player) = runtime.local_view() else {
        pickup_target.clear();
        return;
    };

    let eye = player
        .position
        .plus(crate::protocol::Vec3Net::new(0.0, EYE_HEIGHT, 0.0));
    let dropped_target = snapshot
        .dropped_items
        .iter()
        .filter_map(|item| pickup_score(eye, look.yaw, look.pitch, item).map(|score| (item, score)))
        .min_by(|(_, a), (_, b)| a.total_cmp(b));
    let resource_target =
        best_resource_node_target(eye, look.yaw, look.pitch, snapshot.resource_nodes.iter());

    match (dropped_target, resource_target) {
        (Some((item, item_score)), Some((_, node_score))) if item_score <= node_score => {
            set_dropped_pickup_target(&mut pickup_target, item, &camera, &dropped_entities);
        }
        (Some((item, _)), None) => {
            set_dropped_pickup_target(&mut pickup_target, item, &camera, &dropped_entities);
        }
        (_, Some((node, _))) => {
            set_resource_pickup_target(&mut pickup_target, node, &camera);
        }
        (None, None) => {
            pickup_target.clear();
        }
    }
}

fn set_dropped_pickup_target(
    pickup_target: &mut PickupTargetState,
    item: &DroppedWorldItem,
    camera: &Query<(&Camera, &Transform), With<MainCamera>>,
    dropped_entities: &Query<(&NetworkDroppedItem, &Transform)>,
) {
    pickup_target.clear();
    pickup_target.dropped_item_id = Some(item.id);
    pickup_target.stack = Some(item.stack.clone());
    let anchor = dropped_entities
        .iter()
        .find(|(dropped, _)| dropped.id == item.id)
        .map(|(_, transform)| {
            pickup_anchor_from_position(crate::protocol::Vec3Net::new(
                transform.translation.x,
                transform.translation.y,
                transform.translation.z,
            ))
        })
        .unwrap_or_else(|| pickup_anchor(item));
    pickup_target.world_position = Some(anchor);
    pickup_target.screen_position = viewport_position(camera, anchor);
}

fn set_resource_pickup_target(
    pickup_target: &mut PickupTargetState,
    node: &ResourceNodeState,
    camera: &Query<(&Camera, &Transform), With<MainCamera>>,
) {
    pickup_target.clear();
    pickup_target.resource_node_id = Some(node.id);
    pickup_target.resource_definition_id = Some(node.definition_id.clone());
    pickup_target.resource_storage = node.storage.clone();
    let anchor = resource_node_anchor(node);
    pickup_target.world_position = Some(anchor);
    pickup_target.screen_position = viewport_position(camera, anchor);
}

fn viewport_position(
    camera: &Query<(&Camera, &Transform), With<MainCamera>>,
    anchor: crate::protocol::Vec3Net,
) -> Option<Vec2> {
    camera.single().ok().and_then(|(camera, camera_transform)| {
        camera
            .world_to_viewport(
                &GlobalTransform::from(*camera_transform),
                Vec3::new(anchor.x, anchor.y, anchor.z),
            )
            .ok()
    })
}

pub(crate) fn apply_held_item_visual_system(
    mut commands: Commands,
    runtime: Res<ClientRuntime>,
    menu: Res<MenuState>,
    assets: Res<ItemVisualAssets>,
    gather_input: Res<GatherInputState>,
    camera: Query<Entity, With<MainCamera>>,
    held: Query<(Entity, &HeldItemVisual)>,
) {
    let active_item = (menu.screen == Screen::InGame && !menu.pause_open)
        .then(|| {
            runtime.local_player().and_then(|player| {
                player.inventory.active_actionbar_stack().and_then(|stack| {
                    item_definition(&stack.item_id)
                        .map(|definition| (stack.item_id.clone(), definition))
                })
            })
        })
        .flatten();

    let Some((item_id, definition)) = active_item.filter(|(_, definition)| definition.equipable)
    else {
        for (entity, _) in &held {
            commands.entity(entity).despawn();
        }
        return;
    };

    let Ok(camera_entity) = camera.single() else {
        return;
    };
    let transform = held_item_local_transform(definition.model, gather_input.swing_fraction());
    let (mesh, material) = held_item_visual(&assets, definition.model);
    if let Some((entity, held_visual)) = held.iter().next() {
        let mut entity_commands = commands.entity(entity);
        entity_commands.insert((ChildOf(camera_entity), transform, Visibility::Visible));
        if held_visual.item_id != item_id {
            entity_commands.insert((
                HeldItemVisual {
                    item_id: item_id.clone(),
                },
                Mesh3d(mesh),
                MeshMaterial3d(material),
            ));
        }
    } else {
        commands.spawn((
            Name::new("Held Item"),
            HeldItemVisual { item_id },
            ChildOf(camera_entity),
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            Visibility::Visible,
        ));
    }
}

fn dropped_item_transform(item: &DroppedWorldItem) -> Transform {
    Transform::from_xyz(item.position.x, item.position.y, item.position.z)
        .with_rotation(dropped_item_rotation(item.rotation, item.yaw))
}

fn dropped_item_rotation(rotation: QuatNet, fallback_yaw: f32) -> Quat {
    let len_sq = rotation.x.mul_add(
        rotation.x,
        rotation.y.mul_add(
            rotation.y,
            rotation.z.mul_add(rotation.z, rotation.w * rotation.w),
        ),
    );
    if len_sq.is_finite() && len_sq > f32::EPSILON {
        Quat::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.w).normalize()
    } else {
        Quat::from_rotation_y(fallback_yaw)
    }
}

fn resource_node_transform(node: &ResourceNodeState, model: ResourceNodeModel) -> Transform {
    let (height_offset, scale) = match model {
        ResourceNodeModel::CoalOre => (0.34, Vec3::new(1.0, 1.0, 1.0)),
        ResourceNodeModel::IronOre => (0.36, Vec3::new(1.1, 1.05, 0.95)),
        ResourceNodeModel::SulfurOre => (0.32, Vec3::new(0.96, 0.92, 1.06)),
        ResourceNodeModel::PineTree => (0.0, Vec3::new(1.0, 1.16, 1.0)),
        ResourceNodeModel::BirchTree => (0.0, Vec3::new(0.82, 1.0, 0.82)),
        ResourceNodeModel::DeadTree => (0.0, Vec3::new(0.72, 0.86, 0.72)),
    };
    Transform::from_xyz(
        node.position.x,
        node.position.y + height_offset,
        node.position.z,
    )
    .with_rotation(Quat::from_rotation_y(node.yaw))
    .with_scale(scale)
}

fn resource_node_visual(
    assets: &ResourceVisualAssets,
    node: &ResourceNodeState,
    model: ResourceNodeModel,
) -> (Handle<Mesh>, Handle<StandardMaterial>) {
    match model {
        ResourceNodeModel::CoalOre => (
            ore_mesh_variant(assets, node.id),
            assets.coal_material.clone(),
        ),
        ResourceNodeModel::IronOre => (
            ore_mesh_variant(assets, node.id),
            assets.iron_material.clone(),
        ),
        ResourceNodeModel::SulfurOre => (
            ore_mesh_variant(assets, node.id),
            assets.sulfur_material.clone(),
        ),
        ResourceNodeModel::PineTree => (
            assets.pine_tree_mesh.clone(),
            assets.vertex_material.clone(),
        ),
        ResourceNodeModel::BirchTree => (
            assets.birch_tree_mesh.clone(),
            assets.vertex_material.clone(),
        ),
        ResourceNodeModel::DeadTree => (
            assets.dead_tree_mesh.clone(),
            assets.vertex_material.clone(),
        ),
    }
}

fn ore_mesh_variant(assets: &ResourceVisualAssets, node_id: u64) -> Handle<Mesh> {
    match node_id % 3 {
        0 => assets.ore_mesh_low.clone(),
        1 => assets.ore_mesh_ridge.clone(),
        _ => assets.ore_mesh_cluster.clone(),
    }
}

fn held_item_visual(
    assets: &ItemVisualAssets,
    model: ItemModel,
) -> (Handle<Mesh>, Handle<StandardMaterial>) {
    match model {
        ItemModel::Bag => (
            assets.held_bag_mesh.clone(),
            assets.held_bag_material.clone(),
        ),
        ItemModel::Hatchet => (
            assets.held_hatchet_mesh.clone(),
            assets.held_tool_material.clone(),
        ),
        ItemModel::Pickaxe => (
            assets.held_pickaxe_mesh.clone(),
            assets.held_tool_material.clone(),
        ),
    }
}

fn held_item_local_transform(model: ItemModel, swing_fraction: f32) -> Transform {
    let phase = swing_fraction.clamp(0.0, 1.0);
    let model_down_offset = match model {
        ItemModel::Bag => HELD_ITEM_DOWN_OFFSET,
        ItemModel::Hatchet | ItemModel::Pickaxe => HELD_ITEM_DOWN_OFFSET - 0.03,
    };

    let (swing_translation, base_rotation, model_rotation) = match model {
        ItemModel::Bag => {
            let swing = (phase * PI).sin();
            let windup = (0.5 - phase).max(0.0) * 0.28;
            (
                Vec3::NEG_Z * (swing * 0.06) - Vec3::Y * (swing * 0.05),
                Quat::from_euler(
                    EulerRot::XYZ,
                    -0.35 + windup - swing * 0.9,
                    0.25 + swing * 0.12,
                    0.18 - swing * 0.18,
                ),
                Quat::IDENTITY,
            )
        }
        ItemModel::Hatchet => {
            let pose = hatchet_swing_pose(phase);
            (
                Vec3::NEG_Z * pose.forward + Vec3::X * pose.right + Vec3::Y * pose.up,
                Quat::from_euler(EulerRot::XYZ, pose.pitch, pose.yaw, pose.roll),
                Quat::IDENTITY,
            )
        }
        ItemModel::Pickaxe => {
            let pose = pickaxe_swing_pose(phase);
            (
                Vec3::NEG_Z * pose.forward + Vec3::X * pose.right + Vec3::Y * pose.up,
                Quat::from_euler(EulerRot::XYZ, pose.pitch, pose.yaw, pose.roll),
                Quat::from_rotation_y(PI * 0.5),
            )
        }
    };

    let translation = Vec3::NEG_Z * HELD_ITEM_FORWARD_OFFSET + Vec3::X * HELD_ITEM_RIGHT_OFFSET
        - Vec3::Y * model_down_offset
        + swing_translation;
    Transform::from_translation(translation).with_rotation(base_rotation * model_rotation)
}

#[derive(Debug, Clone, Copy)]
struct ToolSwingPose {
    pitch: f32,
    yaw: f32,
    roll: f32,
    forward: f32,
    right: f32,
    up: f32,
}

fn hatchet_swing_pose(phase: f32) -> ToolSwingPose {
    if phase <= 0.34 {
        let t = smoothstep(phase / 0.34);
        return ToolSwingPose {
            pitch: lerp(-0.34, -0.20, t),
            yaw: lerp(0.22, -0.86, t),
            roll: lerp(0.06, 0.82, t),
            forward: lerp(0.0, -0.04, t),
            right: lerp(0.0, 0.06, t),
            up: lerp(0.0, 0.05, t),
        };
    }

    if phase <= 0.56 {
        let t = smoothstep((phase - 0.34) / 0.22);
        return ToolSwingPose {
            pitch: lerp(-0.20, -0.58, t),
            yaw: lerp(-0.86, 0.34, t),
            roll: lerp(0.82, 0.50, t),
            forward: lerp(-0.04, 0.06, t),
            right: lerp(0.06, -0.10, t),
            up: lerp(0.05, -0.06, t),
        };
    }

    let t = smoothstep((phase - 0.56) / 0.44);
    ToolSwingPose {
        pitch: lerp(-0.58, -0.34, t),
        yaw: lerp(0.34, 0.22, t),
        roll: lerp(0.50, 0.06, t),
        forward: lerp(0.06, 0.0, t),
        right: lerp(-0.10, 0.0, t),
        up: lerp(-0.06, 0.0, t),
    }
}

fn pickaxe_swing_pose(phase: f32) -> ToolSwingPose {
    if phase <= 0.34 {
        let t = smoothstep(phase / 0.34);
        return ToolSwingPose {
            pitch: lerp(-0.34, 0.72, t),
            yaw: lerp(0.16, 0.04, t),
            roll: lerp(0.04, 0.12, t),
            forward: lerp(0.0, -0.09, t),
            right: lerp(0.0, 0.02, t),
            up: lerp(0.0, 0.14, t),
        };
    }

    if phase <= 0.54 {
        let t = smoothstep((phase - 0.34) / 0.20);
        return ToolSwingPose {
            pitch: lerp(0.72, -1.44, t),
            yaw: lerp(0.04, 0.10, t),
            roll: lerp(0.12, -0.14, t),
            forward: lerp(-0.09, 0.18, t),
            right: lerp(0.02, -0.01, t),
            up: lerp(0.14, -0.13, t),
        };
    }

    let t = smoothstep((phase - 0.54) / 0.46);
    ToolSwingPose {
        pitch: lerp(-1.44, -0.34, t),
        yaw: lerp(0.10, 0.16, t),
        roll: lerp(-0.14, 0.04, t),
        forward: lerp(0.18, 0.0, t),
        right: lerp(-0.01, 0.0, t),
        up: lerp(-0.13, 0.0, t),
    }
}

fn smoothstep(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_item_interpolation_blends_between_snapshot_targets() {
        let current = Transform::from_xyz(0.0, 0.0, 0.0);
        let target = Transform::from_xyz(4.0, 0.0, 0.0)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::PI));
        let mut interpolation = DroppedItemInterpolation::new(1, current);

        interpolation.retarget(2, &current, target);
        let halfway = interpolation.advance(DROPPED_ITEM_INTERPOLATION_SECONDS * 0.5);

        assert!((halfway.translation.x - 2.0).abs() < 0.001);
        assert!(halfway.rotation.angle_between(current.rotation) > 0.1);
        assert!(halfway.rotation.angle_between(target.rotation) > 0.1);
    }

    #[test]
    fn dropped_item_interpolation_snaps_extreme_corrections() {
        let current = Transform::from_xyz(0.0, 0.0, 0.0);
        let target = Transform::from_xyz(DROPPED_ITEM_INTERPOLATION_SNAP_DISTANCE + 1.0, 0.0, 0.0);
        let mut interpolation = DroppedItemInterpolation::new(1, current);

        interpolation.retarget(2, &current, target);
        let corrected = interpolation.advance(0.0);

        assert_eq!(corrected.translation, target.translation);
    }

    #[test]
    fn hatchet_swing_pose_sweeps_across_the_body() {
        let ready = hatchet_swing_pose(0.0);
        let windup = hatchet_swing_pose(0.34);
        let impact = hatchet_swing_pose(0.54);

        assert!(windup.right < ready.right + 0.08);
        assert!(windup.forward > ready.forward - 0.06);
        assert!(impact.right < windup.right - 0.14);
        assert!(windup.yaw < ready.yaw - 1.0);
        assert!(impact.yaw > windup.yaw + 1.1);
        assert!(impact.forward < windup.forward + 0.12);
        assert!(impact.roll > windup.roll - 0.40);
    }

    #[test]
    fn pickaxe_swing_pose_stays_centered_for_vertical_strike() {
        let ready = pickaxe_swing_pose(0.0);
        let windup = pickaxe_swing_pose(0.34);
        let impact = pickaxe_swing_pose(0.50);

        assert!(windup.up > ready.up + 0.12);
        assert!(impact.up < ready.up - 0.09);
        assert!(windup.pitch > ready.pitch + 1.0);
        assert!(impact.pitch < windup.pitch - 1.8);
        assert!(windup.right.abs() < 0.03);
        assert!(impact.right.abs() < 0.03);
    }
}
