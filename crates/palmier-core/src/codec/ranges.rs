//! The format's two distinct out-of-range policies. They are not interchangeable.

/// Anything outside `0..=1` becomes `0`. Reproduces `Clip.normalizedValue`, used only
/// by `edgeRounding` and `edgeSoftness`.
pub fn coerce_unit_interval(value: f64) -> f64 {
    if (0.0..=1.0).contains(&value) {
        value
    } else {
        0.0
    }
}

/// Clamped to the bounds. Used only by `Track.displayHeight`.
pub fn clamp_range(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}
