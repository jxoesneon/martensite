//! `weather` namespace — sky conditions, precipitation, temperature.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`weather.*`).
///
/// Every name constant in this module is prefixed `WEATHER_`
/// (`WEATHER_FOO` → `"weather.foo"`) so the flattened
/// `builtin::names` re-export cannot collide.
pub mod names {
    /// `"weather.cloud"` — cloud.
    pub const CLOUD: &str = "weather.cloud";
    /// `"weather.cloud-off"` — cloud struck through.
    pub const CLOUD_OFF: &str = "weather.cloud-off";
    /// `"weather.cloud-sun"` — sun behind cloud, partly cloudy.
    pub const CLOUD_SUN: &str = "weather.cloud-sun";
    /// `"weather.cloud-moon"` — moon behind cloud, partly cloudy night.
    pub const CLOUD_MOON: &str = "weather.cloud-moon";
    /// `"weather.cloud-rain"` — cloud with rain.
    pub const CLOUD_RAIN: &str = "weather.cloud-rain";
    /// `"weather.cloud-drizzle"` — cloud with light rain.
    pub const CLOUD_DRIZZLE: &str = "weather.cloud-drizzle";
    /// `"weather.cloud-snow"` — cloud with snow.
    pub const CLOUD_SNOW: &str = "weather.cloud-snow";
    /// `"weather.cloud-lightning"` — cloud with bolt, storm.
    pub const CLOUD_LIGHTNING: &str = "weather.cloud-lightning";
    /// `"weather.cloud-fog"` — cloud with fog lines.
    pub const CLOUD_FOG: &str = "weather.cloud-fog";
    /// `"weather.sun-dim"` — sun with short rays, low brightness.
    pub const SUN_DIM: &str = "weather.sun-dim";
    /// `"weather.sunrise"` — sun rising over horizon.
    pub const SUNRISE: &str = "weather.sunrise";
    /// `"weather.sunset"` — sun setting under horizon.
    pub const SUNSET: &str = "weather.sunset";
    /// `"weather.thermometer"` — thermometer.
    pub const THERMOMETER: &str = "weather.thermometer";
    /// `"weather.thermometer-snowflake"` — cold thermometer.
    pub const THERMOMETER_SNOWFLAKE: &str = "weather.thermometer-snowflake";
    /// `"weather.umbrella"` — umbrella.
    pub const UMBRELLA: &str = "weather.umbrella";
    /// `"weather.droplet"` — single droplet.
    pub const DROPLET: &str = "weather.droplet";
    /// `"weather.droplets"` — two droplets, humidity.
    pub const DROPLETS: &str = "weather.droplets";
    /// `"weather.snowflake"` — snowflake.
    pub const SNOWFLAKE: &str = "weather.snowflake";
    /// `"weather.wind"` — wind streaks.
    pub const WIND: &str = "weather.wind";
    /// `"weather.rainbow"` — rainbow arcs.
    pub const RAINBOW: &str = "weather.rainbow";
}

/// `"weather.cloud"` — two-bump cloud silhouette.
pub const WEATHER_CLOUD: &str = "M17.5 19H9a7 7 0 116.71-9h1.79a4.5 4.5 0 110 9z";
/// `"weather.cloud-off"` — cloud plus strike slash.
pub const WEATHER_CLOUD_OFF: &str = "M17.5 19H9a7 7 0 116.71-9h1.79a4.5 4.5 0 110 9zM2 2l20 20";
/// `"weather.cloud-sun"` — small sun peeking over a low cloud.
pub const WEATHER_CLOUD_SUN: &str =
    "M10 7a3 3 0 11-6 0 3 3 0 016 0zM7 2v1M2 7h1M3.6 3.6l.7.7M16.5 21H9a4.5 4.5 0 115.5-5.5h2a3 3 0 110 5.5z";
/// `"weather.cloud-moon"` — crescent peeking over a low cloud.
pub const WEATHER_CLOUD_MOON: &str =
    "M10.45 6.4A4.05 4.05 0 116.04 2a3.15 3.15 0 004.41 4.4zM16.5 21H9a4.5 4.5 0 115.5-5.5h2a3 3 0 110 5.5z";
/// `"weather.cloud-rain"` — open cloud with three rain strokes.
pub const WEATHER_CLOUD_RAIN: &str =
    "M4 14.9A7 7 0 1115.7 8h1.8a4.5 4.5 0 012.5 8.2M8 14v6M16 14v6M12 16v6";
/// `"weather.cloud-drizzle"` — open cloud with short rain stubs.
pub const WEATHER_CLOUD_DRIZZLE: &str =
    "M4 14.9A7 7 0 1115.7 8h1.8a4.5 4.5 0 012.5 8.2M8 15v1.5M16 15v1.5M12 16.5v1.5M8 20v1.5M16 20v1.5M12 21v1";
/// `"weather.cloud-snow"` — open cloud with snow dots.
pub const WEATHER_CLOUD_SNOW: &str =
    "M4 14.9A7 7 0 1115.7 8h1.8a4.5 4.5 0 012.5 8.2M8 15h.01M16 15h.01M12 17h.01M8 19h.01M16 19h.01M12 21h.01";
/// `"weather.cloud-lightning"` — open cloud with bolt.
pub const WEATHER_CLOUD_LIGHTNING: &str =
    "M4 14.9A7 7 0 1115.7 8h1.8a4.5 4.5 0 012.5 8.2M13 12L10 17H14L12 22";
/// `"weather.cloud-fog"` — open cloud with fog lines.
pub const WEATHER_CLOUD_FOG: &str =
    "M4 14.9A7 7 0 1115.7 8h1.8a4.5 4.5 0 012.5 8.2M5 18h14M7 21.5h10";
/// `"weather.sun-dim"` — sun disc with short rays.
pub const WEATHER_SUN_DIM: &str =
    "M17 12a5 5 0 11-10 0 5 5 0 0110 0zM12 3.5V5.5M12 18.5V20.5M3.5 12H5.5M18.5 12H20.5M5.4 5.4l1.4 1.4M18.6 5.4l-1.4 1.4M5.4 18.6l1.4-1.4M18.6 18.6l-1.4-1.4";
/// `"weather.sunrise"` — rising sun, up arrow, horizon.
pub const WEATHER_SUNRISE: &str =
    "M12 10V2M8 6l4-4 4 4M4.9 10.9l1.4 1.4M2 18h2M20 18h2M19.1 10.9l-1.4 1.4M16 18a4 4 0 00-8 0M2 22h20";
/// `"weather.sunset"` — setting sun, down arrow, horizon.
pub const WEATHER_SUNSET: &str =
    "M12 2v8M8 6l4 4 4-4M4.9 10.9l1.4 1.4M2 18h2M20 18h2M19.1 10.9l-1.4 1.4M16 18a4 4 0 00-8 0M2 22h20";
/// `"weather.thermometer"` — tube plus bulb.
pub const WEATHER_THERMOMETER: &str = "M14 4v10.5a4 4 0 11-4 0V4a2 2 0 014 0z";
/// `"weather.thermometer-snowflake"` — thermometer plus flake.
pub const WEATHER_THERMOMETER_SNOWFLAKE: &str =
    "M11 4v10.5a4 4 0 11-4 0V4a2 2 0 014 0zM17.5 8v8M14.5 10l6 4M20.5 10l-6 4";
/// `"weather.umbrella"` — dome plus hooked handle.
pub const WEATHER_UMBRELLA: &str = "M21.5 12a9.5 9.5 0 00-19 0zM12 12v8a2 2 0 004 0";
/// `"weather.droplet"` — teardrop.
pub const WEATHER_DROPLET: &str = "M12 2.5C8.5 7 5 10.5 5 15a7 7 0 0014 0C19 10.5 15.5 7 12 2.5z";
/// `"weather.droplets"` — two overlapping droplets.
pub const WEATHER_DROPLETS: &str =
    "M15 5C12.5 8.5 10 11 10 15a6 6 0 0012 0C22 11 19.5 8.5 15 5zM7 9.5C5.7 11 4.5 12.5 4.5 14.3a3.3 3.3 0 006.6 0C11.1 12.5 9.9 11 7 9.5z";
/// `"weather.snowflake"` — two axes plus four tick chevrons.
pub const WEATHER_SNOWFLAKE: &str =
    "M2 12h20M12 2v20M20 16l-4-4 4-4M4 8l4 4-4 4M16 4l-4 4-4-4M8 20l4-4 4 4";
/// `"weather.wind"` — three streaks with curls.
pub const WEATHER_WIND: &str =
    "M9.6 4.6A2 2 0 1111 8H2M17.7 7.7a2.5 2.5 0 111.8 4.3H2M12.6 19.4A2 2 0 1014 16H2";
/// `"weather.rainbow"` — three concentric arcs.
pub const WEATHER_RAINBOW: &str = "M22 17a10 10 0 00-20 0M6 17a6 6 0 0112 0M10 17a2 2 0 014 0";

/// `weather` entries — registered in the pack in this order.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::CLOUD, WEATHER_CLOUD),
    IconEntry::new(names::CLOUD_OFF, WEATHER_CLOUD_OFF),
    IconEntry::new(names::CLOUD_SUN, WEATHER_CLOUD_SUN),
    IconEntry::new(names::CLOUD_MOON, WEATHER_CLOUD_MOON),
    IconEntry::new(names::CLOUD_RAIN, WEATHER_CLOUD_RAIN),
    IconEntry::new(names::CLOUD_DRIZZLE, WEATHER_CLOUD_DRIZZLE),
    IconEntry::new(names::CLOUD_SNOW, WEATHER_CLOUD_SNOW),
    IconEntry::new(names::CLOUD_LIGHTNING, WEATHER_CLOUD_LIGHTNING),
    IconEntry::new(names::CLOUD_FOG, WEATHER_CLOUD_FOG),
    IconEntry::new(names::SUN_DIM, WEATHER_SUN_DIM),
    IconEntry::new(names::SUNRISE, WEATHER_SUNRISE),
    IconEntry::new(names::SUNSET, WEATHER_SUNSET),
    IconEntry::new(names::THERMOMETER, WEATHER_THERMOMETER),
    IconEntry::new(names::THERMOMETER_SNOWFLAKE, WEATHER_THERMOMETER_SNOWFLAKE),
    IconEntry::new(names::UMBRELLA, WEATHER_UMBRELLA),
    IconEntry::new(names::DROPLET, WEATHER_DROPLET),
    IconEntry::new(names::DROPLETS, WEATHER_DROPLETS),
    IconEntry::new(names::SNOWFLAKE, WEATHER_SNOWFLAKE),
    IconEntry::new(names::WIND, WEATHER_WIND),
    IconEntry::new(names::RAINBOW, WEATHER_RAINBOW),
];

/// `weather` morph pairs.
pub const PAIRS: &[IconPair] = &[
    IconPair::new(names::CLOUD_SUN, names::CLOUD_MOON),
    IconPair::new(names::SUNRISE, names::SUNSET),
];
