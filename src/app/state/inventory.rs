use bevy::prelude::*;
use bevy_egui::egui;

use crate::protocol::{DroppedItemId, ItemContainerSlot, ItemStack, ResourceNodeId, Vec3Net};

const GATHER_SWING_SECONDS: f32 = 0.85;
const GATHER_IMPACT_FRACTION: f32 = 0.5;

#[derive(Resource, Default)]
pub(crate) struct InventoryUiState {
    pub(crate) drag: Option<InventoryDrag>,
    pub(crate) hovered_slot: Option<ItemContainerSlot>,
    pub(crate) inventory_rect: Option<egui::Rect>,
    pub(crate) actionbar_rect: Option<egui::Rect>,
    pub(crate) was_open: bool,
}

impl InventoryUiState {
    pub(crate) fn begin_frame(&mut self) {
        self.hovered_slot = None;
        self.inventory_rect = None;
        self.actionbar_rect = None;
    }

    pub(crate) fn cancel_drag(&mut self) {
        self.drag = None;
    }
}

#[derive(Debug, Clone)]
pub(crate) struct InventoryDrag {
    pub(crate) source: ItemContainerSlot,
    pub(crate) stack: ItemStack,
    pub(crate) quantity: u16,
    pub(crate) button: InventoryDragButton,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InventoryDragButton {
    Primary,
    Secondary,
}

#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct PickupTargetState {
    pub(crate) dropped_item_id: Option<DroppedItemId>,
    pub(crate) stack: Option<ItemStack>,
    pub(crate) resource_node_id: Option<ResourceNodeId>,
    pub(crate) resource_definition_id: Option<String>,
    pub(crate) resource_storage: Vec<ItemStack>,
    pub(crate) world_position: Option<Vec3Net>,
    pub(crate) screen_position: Option<Vec2>,
}

impl PickupTargetState {
    pub(crate) fn clear(&mut self) {
        self.dropped_item_id = None;
        self.stack = None;
        self.resource_node_id = None;
        self.resource_definition_id = None;
        self.resource_storage.clear();
        self.world_position = None;
        self.screen_position = None;
    }
}

#[derive(Resource, Debug, Clone)]
pub(crate) struct GatherInputState {
    active_target: Option<ResourceNodeId>,
    swing_elapsed: f32,
    impact_sent: bool,
}

impl Default for GatherInputState {
    fn default() -> Self {
        Self {
            active_target: None,
            swing_elapsed: 0.0,
            impact_sent: false,
        }
    }
}

impl GatherInputState {
    pub(crate) fn update(
        &mut self,
        delta_seconds: f32,
        just_pressed: bool,
        pressed: bool,
        target: Option<ResourceNodeId>,
    ) -> Option<ResourceNodeId> {
        if self.active_target.is_none()
            && (just_pressed || pressed)
            && let Some(target) = target
        {
            self.start_swing(target);
        }

        let active_target = self.active_target?;
        let previous_elapsed = self.swing_elapsed;
        self.swing_elapsed =
            (self.swing_elapsed + delta_seconds.max(0.0)).min(GATHER_SWING_SECONDS);

        let impact_time = GATHER_SWING_SECONDS * GATHER_IMPACT_FRACTION;
        let impact_target = (!self.impact_sent
            && previous_elapsed < impact_time
            && self.swing_elapsed >= impact_time)
            .then_some(active_target);
        if impact_target.is_some() {
            self.impact_sent = true;
        }

        if self.swing_elapsed >= GATHER_SWING_SECONDS {
            if pressed {
                if let Some(target) = target {
                    self.start_swing(target);
                } else {
                    self.cancel();
                }
            } else {
                self.cancel();
            }
        }

        impact_target
    }

    pub(crate) fn cancel(&mut self) {
        self.active_target = None;
        self.swing_elapsed = 0.0;
        self.impact_sent = false;
    }

    fn start_swing(&mut self, target: ResourceNodeId) {
        self.active_target = Some(target);
        self.swing_elapsed = 0.0;
        self.impact_sent = false;
    }

    pub(crate) fn swing_fraction(&self) -> f32 {
        if self.active_target.is_none() {
            0.0
        } else {
            (self.swing_elapsed / GATHER_SWING_SECONDS).clamp(0.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ItemContainerSlot, ItemStack, Vec3Net};

    #[test]
    fn inventory_ui_state_resets_frame_and_drag_state() {
        let mut state = InventoryUiState {
            drag: Some(InventoryDrag {
                source: ItemContainerSlot::inventory(2),
                stack: ItemStack::new("ore", 4),
                quantity: 2,
                button: InventoryDragButton::Secondary,
            }),
            hovered_slot: Some(ItemContainerSlot::actionbar(1)),
            inventory_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(10.0, 10.0),
            )),
            actionbar_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(5.0, 5.0),
            )),
            was_open: true,
        };

        state.begin_frame();

        assert!(state.hovered_slot.is_none());
        assert!(state.inventory_rect.is_none());
        assert!(state.actionbar_rect.is_none());
        assert!(state.drag.is_some());
        assert!(state.was_open);

        state.cancel_drag();
        assert!(state.drag.is_none());
    }

    #[test]
    fn pickup_target_clear_removes_cached_target() {
        let mut state = PickupTargetState {
            dropped_item_id: Some(7),
            stack: Some(ItemStack::new("ore", 1)),
            resource_node_id: Some(8),
            resource_definition_id: Some("node".to_owned()),
            resource_storage: vec![ItemStack::new("wood", 2)],
            world_position: Some(Vec3Net::new(1.0, 2.0, 3.0)),
            screen_position: Some(Vec2::new(10.0, 20.0)),
        };

        state.clear();

        assert!(state.dropped_item_id.is_none());
        assert!(state.stack.is_none());
        assert!(state.resource_node_id.is_none());
        assert!(state.resource_definition_id.is_none());
        assert!(state.resource_storage.is_empty());
        assert!(state.world_position.is_none());
        assert!(state.screen_position.is_none());
    }

    #[test]
    fn gather_input_sends_at_swing_impact_and_repeats_while_held() {
        let mut state = GatherInputState::default();

        assert_eq!(state.update(0.1, true, true, Some(4)), None);
        assert!(state.swing_fraction() > 0.0);

        assert_eq!(
            state.update(
                GATHER_SWING_SECONDS * GATHER_IMPACT_FRACTION,
                false,
                true,
                Some(4)
            ),
            Some(4)
        );
        assert_eq!(state.update(0.1, false, true, Some(4)), None);

        let _ = state.update(GATHER_SWING_SECONDS, false, true, Some(5));
        assert!(state.swing_fraction() < 0.2);
    }
}
