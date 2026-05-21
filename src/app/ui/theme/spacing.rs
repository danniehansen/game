/// Height of a compact button, input field, table row, or toast pill.
/// Anything sized to this value visually aligns with a `ButtonDensity::Compact`
/// button.
pub(in crate::app::ui) const COMPACT_ROW_HEIGHT: f32 = 34.0;

/// Height of a menu-density button (used for primary main-menu actions).
pub(in crate::app::ui) const MENU_ROW_HEIGHT: f32 = 46.0;

/// Vertical breathing room reserved between a bounded screen panel and the
/// top/bottom of the window. Chosen so panels don't bump against the edge on
/// any supported resolution while still letting tables grow on tall monitors.
pub(in crate::app::ui) const BOUNDED_PANEL_VERTICAL_PADDING: f32 = 56.0;
