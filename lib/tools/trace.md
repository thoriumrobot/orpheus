# Trace.Tool — the ray tracer
Written in Latte, Anvil-compiled: checkered ground, sky, key + fill lights, shadows,
speculars, graded reflections.

w [field: w=96]  h [field: h=72]
[Render](run: trace w=$w h=$h) [Render large](run: trace w=160 h=120)

## The scene is Latte data
A list of [center [radius [color [refl 0]]]] (v3 makes vectors; signed nums [sign mag]×1000;
refl 0..1000):
[One red mirror sphere](run: trace w=$w h=$h scene=[ [ (v3 [0 0] [0 800] [0 500]) [ [0 800] [ (v3 [0 950] [0 300] [0 300]) [ 700 0 ] ] ] ] 0 ])

Or edit the built-in scene (the spheres arm), Compile, re-render:
[Open lib/trace.lat](run: System.Open trace)
