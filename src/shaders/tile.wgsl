// Tile of a layer stack pass. Scratch textures (accumulators, masks) cover
// the tile's scratch rect; texture set textures cover the whole set.
struct TileParams {
    // xy: global texel of dispatch texel (0, 0)
    origin: vec4<i32>,
    // xy: dispatch extent, zw: set resolution
    size: vec4<i32>,
    // xy: global texel of scratch texel (0, 0)
    scratch: vec4<i32>,
    // owned rect (min xy, max xy exclusive), global
    inner: vec4<i32>,
};
@group(1) @binding(0) var<uniform> T: TileParams;

fn in_tile(l: vec2<i32>) -> bool {
    return all(l < T.size.xy);
}

// Scratch texel to global texel (passes dispatched over the scratch rect).
fn to_global(l: vec2<i32>) -> vec2<i32> {
    return l + T.scratch.xy;
}
