use bevy::prelude::*;

use super::builder::{
    BARK_DARK, BARK_MID, BIRCH_BARK, BIRCH_BARK_BAND, DEAD_WOOD, DEAD_WOOD_DARK, LEAF_BIRCH,
    LEAF_BIRCH_DARK, LEAF_BIRCH_LIGHT, LEAF_PINE, LEAF_PINE_DARK, LEAF_PINE_LIGHT,
    LowPolyMeshBuilder, MeshColor,
};

// Each pine variant has a clear bare trunk before foliage starts; this lets
// the player read "real tree" rather than "shrub" at a distance. Larger
// variants get more foliage layers so the silhouette stays full at height.

pub(crate) fn low_poly_pine_tree_small_mesh() -> Mesh {
    let mut builder = LowPolyMeshBuilder::default();
    // Trunk: ~1.4m visible before foliage. Flared base, tapering up.
    builder.add_box([0.0, 0.10, 0.0], [0.20, 0.10, 0.20], BARK_DARK);
    builder.add_box([0.0, 0.40, 0.0], [0.16, 0.20, 0.16], BARK_MID);
    builder.add_box([0.0, 0.85, 0.0], [0.14, 0.25, 0.14], BARK_DARK);
    builder.add_box([0.0, 1.25, 0.0], [0.12, 0.15, 0.12], BARK_MID);
    // Foliage cones overlap the upper trunk and stack to ~4.5m.
    builder.add_cone(1.10, 0.95, 1.30, 8, LEAF_PINE_DARK);
    builder.add_cone(1.85, 0.95, 1.10, 8, LEAF_PINE);
    builder.add_cone(2.55, 0.85, 0.88, 8, LEAF_PINE_DARK);
    builder.add_cone(3.20, 0.75, 0.66, 7, LEAF_PINE);
    builder.add_cone(3.80, 0.55, 0.44, 7, LEAF_PINE_LIGHT);
    builder.add_cone(4.20, 0.30, 0.22, 6, LEAF_PINE_LIGHT);
    builder.build()
}

pub(crate) fn low_poly_pine_tree_medium_mesh() -> Mesh {
    let mut builder = LowPolyMeshBuilder::default();
    // Trunk: substantial bare trunk to ~2.2m, then foliage to ~6.5m.
    builder.add_box([0.0, 0.12, 0.0], [0.26, 0.12, 0.26], BARK_DARK);
    builder.add_box([0.0, 0.50, 0.0], [0.21, 0.26, 0.21], BARK_MID);
    builder.add_box([0.0, 1.05, 0.0], [0.18, 0.29, 0.18], BARK_DARK);
    builder.add_box([0.0, 1.60, 0.0], [0.16, 0.26, 0.16], BARK_MID);
    builder.add_box([0.0, 2.05, 0.0], [0.14, 0.19, 0.14], BARK_DARK);
    // Foliage cones stack to 6.5m. Wider base layers; tighter top.
    builder.add_cone(1.85, 1.30, 1.85, 9, LEAF_PINE_DARK);
    builder.add_cone(2.85, 1.20, 1.55, 9, LEAF_PINE);
    builder.add_cone(3.80, 1.10, 1.25, 8, LEAF_PINE_DARK);
    builder.add_cone(4.65, 0.95, 0.98, 8, LEAF_PINE);
    builder.add_cone(5.40, 0.80, 0.72, 7, LEAF_PINE_LIGHT);
    builder.add_cone(6.05, 0.55, 0.46, 7, LEAF_PINE_LIGHT);
    builder.add_cone(6.40, 0.20, 0.18, 6, LEAF_PINE_LIGHT);
    builder.build()
}

pub(crate) fn low_poly_pine_tree_large_mesh() -> Mesh {
    let mut builder = LowPolyMeshBuilder::default();
    // Old-growth pine: wide flared base, prominent bare trunk to ~3.5m.
    builder.add_box([0.0, 0.16, 0.0], [0.36, 0.16, 0.36], BARK_DARK);
    builder.add_box([0.0, 0.60, 0.0], [0.29, 0.30, 0.29], BARK_MID);
    builder.add_box([0.0, 1.25, 0.0], [0.25, 0.35, 0.25], BARK_DARK);
    builder.add_box([0.0, 1.90, 0.0], [0.22, 0.32, 0.22], BARK_MID);
    builder.add_box([0.0, 2.55, 0.0], [0.20, 0.32, 0.20], BARK_DARK);
    builder.add_box([0.0, 3.15, 0.0], [0.18, 0.28, 0.18], BARK_MID);
    // Seven foliage layers stack to 9m for a dense canopy silhouette.
    builder.add_cone(2.60, 1.60, 2.40, 10, LEAF_PINE_DARK);
    builder.add_cone(3.85, 1.50, 2.10, 10, LEAF_PINE);
    builder.add_cone(5.00, 1.35, 1.75, 9, LEAF_PINE_DARK);
    builder.add_cone(6.05, 1.20, 1.40, 9, LEAF_PINE);
    builder.add_cone(7.00, 1.05, 1.05, 8, LEAF_PINE_DARK);
    builder.add_cone(7.85, 0.85, 0.72, 8, LEAF_PINE_LIGHT);
    builder.add_cone(8.55, 0.55, 0.44, 7, LEAF_PINE_LIGHT);
    builder.add_cone(8.90, 0.20, 0.20, 6, LEAF_PINE_LIGHT);
    builder.build()
}

pub(crate) fn low_poly_birch_tree_small_mesh() -> Mesh {
    let mut builder = LowPolyMeshBuilder::default();
    // Birch trunks are slender; bands give the papery look at any size.
    // Segments stack with cumulative y so the trunk reads as one continuous
    // pole rather than separated discs. Extra-tall top segment so the trunk
    // climbs into the canopy and doesn't read as detached from the green
    // mass above.
    stack_birch_trunk(
        &mut builder,
        &[
            (0.155, 0.24, BIRCH_BARK),
            (0.158, 0.08, BIRCH_BARK_BAND),
            (0.150, 0.46, BIRCH_BARK),
            (0.153, 0.07, BIRCH_BARK_BAND),
            (0.145, 0.46, BIRCH_BARK),
            (0.148, 0.07, BIRCH_BARK_BAND),
            (0.140, 0.60, BIRCH_BARK),
        ],
    );
    // Canopy clusters above the trunk reach ~3.6m. `add_octa_rock`'s bottom
    // vertex sits at `cy - 0.82 * sy`, so canopy centers must be low enough
    // that the lowest bottom vertex dips into the trunk top (~y=1.98) and
    // visually reads as one continuous tree.
    builder.add_octa_rock([0.0, 2.55, 0.0], [1.10, 0.85, 1.05], LEAF_BIRCH);
    builder.add_octa_rock([-0.55, 2.30, 0.18], [0.68, 0.58, 0.60], LEAF_BIRCH_DARK);
    builder.add_octa_rock([0.55, 2.35, -0.12], [0.62, 0.54, 0.58], LEAF_BIRCH_DARK);
    builder.add_octa_rock([0.18, 2.90, 0.28], [0.58, 0.46, 0.54], LEAF_BIRCH_LIGHT);
    builder.add_octa_rock([-0.32, 2.85, -0.30], [0.52, 0.42, 0.48], LEAF_BIRCH_LIGHT);
    builder.add_octa_rock([0.05, 3.20, -0.02], [0.36, 0.36, 0.36], LEAF_BIRCH_LIGHT);
    builder.build()
}

/// Stacks birch trunk segments end-to-end so the resulting cylinder reads as
/// continuous — no horizontal gaps between bark and band sections. Each entry
/// is `(half_width, total_height, color)`.
fn stack_birch_trunk(builder: &mut LowPolyMeshBuilder, segments: &[(f32, f32, MeshColor)]) {
    let mut y = 0.0;
    for &(half_width, height, color) in segments {
        let half_height = height * 0.5;
        let center_y = y + half_height;
        builder.add_box(
            [0.0, center_y, 0.0],
            [half_width, half_height, half_width],
            color,
        );
        y += height;
    }
}

pub(crate) fn low_poly_birch_tree_medium_mesh() -> Mesh {
    let mut builder = LowPolyMeshBuilder::default();
    // Extended trunk with more bands. Bands stay thin so they read as
    // papery markings rather than separate sections.
    stack_birch_trunk(
        &mut builder,
        &[
            (0.200, 0.36, BIRCH_BARK),
            (0.202, 0.10, BIRCH_BARK_BAND),
            (0.195, 0.60, BIRCH_BARK),
            (0.197, 0.09, BIRCH_BARK_BAND),
            (0.190, 0.60, BIRCH_BARK),
            (0.192, 0.09, BIRCH_BARK_BAND),
            (0.185, 0.60, BIRCH_BARK),
            (0.187, 0.09, BIRCH_BARK_BAND),
            (0.180, 0.60, BIRCH_BARK),
            (0.182, 0.08, BIRCH_BARK_BAND),
            (0.170, 0.40, BIRCH_BARK),
        ],
    );
    // Layered canopy of overlapping octa-rocks centered around 4.0m.
    builder.add_octa_rock([0.0, 4.05, 0.0], [1.55, 1.05, 1.45], LEAF_BIRCH);
    builder.add_octa_rock([-0.70, 3.70, 0.22], [0.92, 0.72, 0.84], LEAF_BIRCH_DARK);
    builder.add_octa_rock([0.74, 3.78, -0.14], [0.86, 0.68, 0.80], LEAF_BIRCH_DARK);
    builder.add_octa_rock([0.22, 4.45, 0.40], [0.82, 0.66, 0.74], LEAF_BIRCH_LIGHT);
    builder.add_octa_rock([-0.48, 4.36, -0.42], [0.74, 0.58, 0.66], LEAF_BIRCH_LIGHT);
    builder.add_octa_rock([0.08, 4.85, -0.04], [0.48, 0.42, 0.48], LEAF_BIRCH_LIGHT);
    builder.build()
}

pub(crate) fn low_poly_birch_tree_large_mesh() -> Mesh {
    let mut builder = LowPolyMeshBuilder::default();
    // Mature birch: thicker trunk, more bands carrying the look up to ~5m.
    stack_birch_trunk(
        &mut builder,
        &[
            (0.260, 0.44, BIRCH_BARK),
            (0.262, 0.12, BIRCH_BARK_BAND),
            (0.255, 0.70, BIRCH_BARK),
            (0.257, 0.10, BIRCH_BARK_BAND),
            (0.250, 0.70, BIRCH_BARK),
            (0.252, 0.10, BIRCH_BARK_BAND),
            (0.245, 0.70, BIRCH_BARK),
            (0.247, 0.10, BIRCH_BARK_BAND),
            (0.240, 0.70, BIRCH_BARK),
            (0.242, 0.10, BIRCH_BARK_BAND),
            (0.235, 0.70, BIRCH_BARK),
            (0.237, 0.09, BIRCH_BARK_BAND),
            (0.225, 0.40, BIRCH_BARK),
        ],
    );
    // Bigger, denser canopy — additional clusters for fullness.
    builder.add_octa_rock([0.0, 5.75, 0.0], [2.10, 1.40, 1.95], LEAF_BIRCH);
    builder.add_octa_rock([-1.00, 5.30, 0.32], [1.20, 0.92, 1.08], LEAF_BIRCH_DARK);
    builder.add_octa_rock([1.04, 5.42, -0.20], [1.14, 0.88, 1.04], LEAF_BIRCH_DARK);
    builder.add_octa_rock([0.30, 6.30, 0.58], [1.08, 0.84, 0.98], LEAF_BIRCH_LIGHT);
    builder.add_octa_rock([-0.68, 6.18, -0.58], [1.00, 0.80, 0.92], LEAF_BIRCH_LIGHT);
    builder.add_octa_rock([0.42, 6.40, -0.40], [0.78, 0.60, 0.72], LEAF_BIRCH_LIGHT);
    builder.add_octa_rock([-0.30, 5.95, 0.50], [0.74, 0.58, 0.68], LEAF_BIRCH_DARK);
    builder.add_octa_rock([0.10, 6.78, 0.05], [0.62, 0.54, 0.62], LEAF_BIRCH_LIGHT);
    builder.build()
}

pub(crate) fn low_poly_dead_tree_small_mesh() -> Mesh {
    let mut builder = LowPolyMeshBuilder::default();
    // Short snag: gnarled offset trunk, splintered top, a few stubs.
    builder.add_box([0.0, 0.12, 0.0], [0.22, 0.12, 0.22], DEAD_WOOD_DARK);
    builder.add_box([0.03, 0.42, -0.02], [0.18, 0.20, 0.18], DEAD_WOOD);
    builder.add_box([-0.02, 0.85, 0.03], [0.16, 0.25, 0.16], DEAD_WOOD_DARK);
    builder.add_box([0.04, 1.30, -0.03], [0.14, 0.22, 0.14], DEAD_WOOD);
    builder.add_box([-0.03, 1.70, 0.02], [0.13, 0.18, 0.13], DEAD_WOOD_DARK);
    builder.add_box([0.0, 2.05, 0.0], [0.12, 0.14, 0.12], DEAD_WOOD);
    // Splintered top.
    builder.add_tri_prism([[-0.10, 2.20], [0.10, 2.70], [0.12, 2.20]], 0.11, DEAD_WOOD);
    builder.add_tri_prism(
        [[-0.08, 2.20], [-0.04, 2.55], [0.06, 2.20]],
        0.07,
        DEAD_WOOD_DARK,
    );
    // Two short broken branches.
    builder.add_box([0.40, 1.45, 0.06], [0.30, 0.06, 0.06], DEAD_WOOD_DARK);
    builder.add_box([0.62, 1.50, 0.06], [0.07, 0.04, 0.04], DEAD_WOOD);
    builder.add_box([-0.32, 1.70, -0.08], [0.26, 0.06, 0.06], DEAD_WOOD);
    builder.add_box([-0.52, 1.62, -0.08], [0.07, 0.04, 0.04], DEAD_WOOD_DARK);
    // Knots.
    builder.add_box([0.13, 0.65, 0.13], [0.034, 0.034, 0.034], DEAD_WOOD_DARK);
    builder.add_box([-0.12, 1.10, -0.13], [0.034, 0.034, 0.034], DEAD_WOOD_DARK);
    builder.build()
}

pub(crate) fn low_poly_dead_tree_medium_mesh() -> Mesh {
    let mut builder = LowPolyMeshBuilder::default();
    // Mid-size dead tree: weathered, leaning trunk with longer broken
    // branches sticking out at multiple heights.
    builder.add_box([0.0, 0.16, 0.0], [0.28, 0.16, 0.28], DEAD_WOOD_DARK);
    builder.add_box([0.03, 0.55, -0.02], [0.23, 0.25, 0.23], DEAD_WOOD);
    builder.add_box([-0.02, 1.05, 0.03], [0.20, 0.27, 0.20], DEAD_WOOD_DARK);
    builder.add_box([0.04, 1.55, -0.03], [0.18, 0.25, 0.18], DEAD_WOOD);
    builder.add_box([-0.03, 2.05, 0.02], [0.17, 0.27, 0.17], DEAD_WOOD_DARK);
    builder.add_box([0.04, 2.55, -0.02], [0.15, 0.25, 0.15], DEAD_WOOD);
    builder.add_box([-0.02, 3.00, 0.02], [0.14, 0.20, 0.14], DEAD_WOOD_DARK);
    builder.add_box([0.0, 3.35, 0.0], [0.12, 0.16, 0.12], DEAD_WOOD);
    // Splintered top reaches ~4.5m.
    builder.add_tri_prism([[-0.12, 3.55], [0.10, 4.20], [0.14, 3.55]], 0.13, DEAD_WOOD);
    builder.add_tri_prism(
        [[-0.10, 3.55], [-0.05, 3.95], [0.06, 3.55]],
        0.08,
        DEAD_WOOD_DARK,
    );
    builder.add_tri_prism([[0.04, 3.55], [0.18, 4.45], [0.16, 3.55]], 0.06, DEAD_WOOD);
    // Long broken branches at three heights — give a stark dead silhouette.
    builder.add_box([0.55, 1.85, 0.06], [0.45, 0.07, 0.07], DEAD_WOOD_DARK);
    builder.add_box([0.85, 1.92, 0.06], [0.08, 0.05, 0.05], DEAD_WOOD);
    builder.add_box([-0.50, 2.40, -0.10], [0.42, 0.07, 0.07], DEAD_WOOD);
    builder.add_box([-0.78, 2.30, -0.10], [0.08, 0.05, 0.05], DEAD_WOOD_DARK);
    builder.add_box([0.08, 1.30, 0.55], [0.07, 0.06, 0.40], DEAD_WOOD_DARK);
    builder.add_box([-0.12, 0.95, -0.48], [0.07, 0.06, 0.36], DEAD_WOOD);
    // Knots.
    builder.add_box([0.16, 0.85, 0.16], [0.04, 0.04, 0.04], DEAD_WOOD_DARK);
    builder.add_box([-0.14, 1.40, -0.16], [0.04, 0.04, 0.04], DEAD_WOOD_DARK);
    builder.add_box([0.10, 2.20, -0.13], [0.038, 0.038, 0.038], DEAD_WOOD_DARK);
    builder.build()
}

pub(crate) fn low_poly_dead_tree_large_mesh() -> Mesh {
    let mut builder = LowPolyMeshBuilder::default();
    // Ancient dead tree: wide flared base with exposed root buttresses,
    // dramatic taper from base to top, a forked split where a major limb
    // snapped off, and a wide splintered top suggesting the upper trunk
    // tore away rather than weathering smoothly.

    // Root buttresses — multiple offset boxes at ground level imply roots
    // that have eroded out around the base. Spread further than the trunk.
    builder.add_box([0.0, 0.12, 0.0], [0.42, 0.12, 0.42], DEAD_WOOD_DARK);
    builder.add_box([0.30, 0.08, 0.05], [0.14, 0.08, 0.10], DEAD_WOOD_DARK);
    builder.add_box([-0.28, 0.09, -0.04], [0.12, 0.09, 0.10], DEAD_WOOD_DARK);
    builder.add_box([0.06, 0.07, 0.32], [0.11, 0.07, 0.12], DEAD_WOOD_DARK);
    builder.add_box([-0.04, 0.08, -0.30], [0.12, 0.08, 0.12], DEAD_WOOD_DARK);

    // Heavily tapered trunk — base is wide, top is barely a finger.
    builder.add_box([0.0, 0.42, 0.0], [0.36, 0.18, 0.36], DEAD_WOOD);
    builder.add_box([0.05, 0.92, -0.04], [0.30, 0.32, 0.30], DEAD_WOOD_DARK);
    builder.add_box([-0.04, 1.52, 0.05], [0.26, 0.28, 0.26], DEAD_WOOD);
    builder.add_box([0.06, 2.08, -0.05], [0.22, 0.28, 0.22], DEAD_WOOD_DARK);
    builder.add_box([-0.05, 2.62, 0.06], [0.19, 0.26, 0.19], DEAD_WOOD);
    // Fork point: the trunk briefly widens where a major limb tore off.
    builder.add_box([0.07, 3.10, -0.05], [0.21, 0.22, 0.18], DEAD_WOOD_DARK);
    // Broken fork: a stubby second trunk leans out to +x and is snapped
    // short, with jagged tri_prism splinters at its top. Kept at z=0
    // because `add_tri_prism` builds its splinters around z=0 — offsetting
    // the fork in z would float the splinters beside the broken stump.
    builder.add_box([0.30, 3.32, 0.0], [0.18, 0.20, 0.14], DEAD_WOOD);
    builder.add_box([0.40, 3.66, 0.0], [0.14, 0.16, 0.11], DEAD_WOOD_DARK);
    builder.add_tri_prism([[0.28, 3.78], [0.44, 4.20], [0.52, 3.78]], 0.11, DEAD_WOOD);
    builder.add_tri_prism(
        [[0.32, 3.78], [0.38, 4.02], [0.46, 3.78]],
        0.08,
        DEAD_WOOD_DARK,
    );
    // Main trunk continues thinning past the fork.
    builder.add_box([-0.02, 3.52, 0.07], [0.15, 0.20, 0.15], DEAD_WOOD);
    builder.add_box([0.06, 3.96, -0.04], [0.12, 0.22, 0.12], DEAD_WOOD_DARK);
    builder.add_box([-0.04, 4.42, 0.05], [0.10, 0.22, 0.10], DEAD_WOOD);
    builder.add_box([0.04, 4.80, -0.03], [0.085, 0.16, 0.085], DEAD_WOOD_DARK);
    // Wide snapped-trunk top: four offset splinters of decreasing height
    // suggest the upper trunk tore off rather than weathered to a point.
    builder.add_tri_prism([[-0.13, 4.96], [0.06, 5.85], [0.13, 4.96]], 0.14, DEAD_WOOD);
    builder.add_tri_prism(
        [[-0.10, 4.96], [-0.04, 5.55], [0.04, 4.96]],
        0.09,
        DEAD_WOOD_DARK,
    );
    builder.add_tri_prism([[0.02, 4.96], [0.16, 5.30], [0.18, 4.96]], 0.07, DEAD_WOOD);
    builder.add_tri_prism(
        [[-0.18, 4.96], [-0.12, 5.20], [-0.08, 4.96]],
        0.06,
        DEAD_WOOD_DARK,
    );

    // Major broken limbs: long jagged horizontal branches snapped off at
    // various lengths. Each long branch ends in a knotty stub that has to
    // overlap the branch in all three axes — small overlaps look like
    // floating debris from a distance.
    builder.add_box([0.74, 2.20, 0.10], [0.58, 0.10, 0.10], DEAD_WOOD_DARK);
    builder.add_box([1.20, 2.20, 0.10], [0.10, 0.06, 0.06], DEAD_WOOD);
    builder.add_box([-0.30, 1.85, -0.04], [0.14, 0.07, 0.07], DEAD_WOOD); // short snapped stub at trunk
    builder.add_box([-0.70, 2.70, -0.12], [0.55, 0.10, 0.10], DEAD_WOOD);
    builder.add_box([-1.15, 2.70, -0.12], [0.10, 0.06, 0.06], DEAD_WOOD_DARK);
    builder.add_box([0.12, 1.55, 0.72], [0.10, 0.09, 0.58], DEAD_WOOD_DARK);
    builder.add_box([0.12, 1.55, 1.25], [0.06, 0.05, 0.10], DEAD_WOOD);
    // -z side branch + knotty end stub (replaces an earlier floating stub
    // that had no connecting branch into the trunk).
    builder.add_box([-0.10, 1.20, -0.55], [0.09, 0.08, 0.40], DEAD_WOOD);
    builder.add_box([-0.10, 1.20, -1.00], [0.05, 0.05, 0.10], DEAD_WOOD_DARK);
    builder.add_box([-0.42, 3.85, 0.08], [0.32, 0.07, 0.07], DEAD_WOOD_DARK);
    // Knots and gouges along the trunk.
    builder.add_box([0.20, 1.20, 0.20], [0.050, 0.050, 0.050], DEAD_WOOD_DARK);
    builder.add_box([-0.18, 1.95, -0.20], [0.050, 0.050, 0.050], DEAD_WOOD_DARK);
    builder.add_box([0.14, 2.95, -0.18], [0.045, 0.045, 0.045], DEAD_WOOD_DARK);
    builder.add_box([-0.10, 3.70, 0.13], [0.042, 0.042, 0.042], DEAD_WOOD_DARK);
    builder.build()
}
