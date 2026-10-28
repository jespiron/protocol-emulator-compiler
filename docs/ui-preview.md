# The Spatial animation

The chosen direction is a large, slowly rotating Klein bottle on deep indigo.
Lavender shadows turn to ice-blue highlights. The surface is drawn with
`· : + * #`, so its curvature and depth remain visible even without color.

The screen has only the shape and an operation/status pair, for example:

```text
spatial build
Checking constraints…
```

There are no borders, slogans, progress bars, percentages, or decorative task
lists. A full rotation takes about 2.22 seconds (20/9 seconds), rendered at about
30 frames per second. Short real commands must never wait for an animation to finish.

## Try it

From the compiler checkout:

```sh
cargo run -- preview
```

With the compiler installed:

```sh
spatial preview
```

**This command is still a visual preview.** It does not build, create, install,
download, or simulate anything. The operation/status pairs are static examples;
they are not reports of work happening behind the animation. The real commands
continue using plain text until the renderer is connected to their real events.

The controls stay off-screen to keep the presentation quiet:

| Key | Action |
|---|---|
| Space | Pause/resume rotation |
| B | Show the build example |
| N | Show the new-project example |
| R | Show the run example |
| I | Show the first-launch example |
| Q, Esc, Ctrl-C | Exit |

The shape scales to the terminal. On a tiny terminal the status remains available
and the shape has less detail. For a plain-text frame without an interactive
terminal:

```sh
spatial preview --snapshot
```

This uses the same renderer without terminal control codes or colors. The middle
dot and ellipsis are Unicode; the rest of the surface alphabet is ASCII.

## Shape and shading

This is the classic **Klein-bottle immersion**, with its neck returning through
its own wall. It is not a spinning torus. The parametrization was checked against
[Three.js's official Klein-bottle implementation](https://github.com/mrdoob/three.js/blob/dev/examples/jsm/geometries/ParametricFunctions.js).

For angles `u` and `v` in `[0, 2π]`, let:

```text
r = 2 - cos(u)
c = 3 cos(u) (1 + sin(u))

u < π:  x = c + r cos(u) cos(v)    y = 8 sin(u) + r sin(u) cos(v)
u ≥ π:  x = c - r cos(v)          y = 8 sin(u)

z = -r sin(v)
```

The renderer recenters and scales those coordinates. At the closing seam,
`(0, v)` joins `(2π, π-v)`: the cross-section reverses orientation. A unit test
checks that seam relation. The unavoidable crossing is part of the representation
in three dimensions, not a hole in the mathematical surface.

The renderer samples the surface and its tangent normals once, then rotates and
projects those points for each frame. A depth buffer keeps the nearest surface
sample in each character cell, so the back of the bottle cannot overwrite its
front. Lighting, a small highlight, and depth attenuation choose each character
and color. Both surface sides are shaded because a Klein bottle has no globally
consistent inside/outside. Character-cell aspect correction keeps it from looking
stretched on a typical terminal.

The palette is:

| Role | Color |
|---|---|
| Background | Deep indigo `#101120` |
| Surface shadows / operation | Lavender `#b59dff` |
| Surface highlights / status | Ice blue `#7bcff6` |

## Implementation boundaries

`src/preview.rs` contains the parametric surface, projection/depth/shading, drawing,
and the small preview event loop. Rendering takes operation/status text separately
from the geometry, so a real command can eventually supply its actual stage.
No compiler or simulator functions are called by the preview.

Ratatui owns terminal setup and restoration. The event loop handles pause and
quit; it does not invent completion events. Tests cover the orientation-reversing
seam, nearest-surface depth behavior, slow rotation, pause, distinct shaded frames,
small terminals, keyboard controls, and escape-free snapshots.

A first-launch animation is possible later. `cargo install` owns its own output;
we have not added a separate installer or inserted an animation into Cargo.
