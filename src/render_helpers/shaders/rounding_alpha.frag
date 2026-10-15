// Negative radii indicate a straight (chamfered) corner. Radii are fitted
// before reaching this shader, so corner regions never overlap.
float niri_rounding_alpha(vec2 coords, vec2 size, vec4 corner_radius) {
    vec4 radii = abs(corner_radius);
    vec2 local;
    float radius;
    bool chamfered;

    if (coords.x < radii.x && coords.y < radii.x) {
        radius = radii.x;
        local = coords;
        chamfered = corner_radius.x < 0.0;
    } else if (size.x - radii.y < coords.x && coords.y < radii.y) {
        radius = radii.y;
        local = vec2(size.x - coords.x, coords.y);
        chamfered = corner_radius.y < 0.0;
    } else if (size.x - radii.z < coords.x && size.y - radii.z < coords.y) {
        radius = radii.z;
        local = size - coords;
        chamfered = corner_radius.z < 0.0;
    } else if (coords.x < radii.w && size.y - radii.w < coords.y) {
        radius = radii.w;
        local = vec2(coords.x, size.y - coords.y);
        chamfered = corner_radius.w < 0.0;
    } else {
        return 1.0;
    }

    float distance_to_edge = chamfered
        ? (radius - local.x - local.y) * 0.70710678
        : distance(local, vec2(radius)) - radius;
    float t = clamp(distance_to_edge * niri_scale + 0.5, 0.0, 1.0);
    return 1.0 - t * t * (3.0 - 2.0 * t);
}
