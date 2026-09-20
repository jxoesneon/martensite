//! Feedback & display category — one live card per assigned widget
//! module. Every construction is adapted from the widget's own `///`
//! doctest (the verified canonical example in its source file).

use std::time::Duration;

use martensite::core::Widget;
use martensite::widgets::activity_ring::ActivityRing;
use martensite::widgets::alarm_panel::{Alarm, AlarmPanel};
use martensite::widgets::analog_clock::AnalogClock;
use martensite::widgets::avatar::Avatar;
use martensite::widgets::avatar_group::AvatarGroup;
use martensite::widgets::badge::Badge;
use martensite::widgets::banner::{Banner, Severity};
use martensite::widgets::battery::Battery;
use martensite::widgets::chip::{Chip, ChipKind};
use martensite::widgets::chip_group::{ChipGroup, ChipSelection};
use martensite::widgets::compass::Compass;
use martensite::widgets::confetti::Confetti;
use martensite::widgets::countdown::Countdown;
use martensite::widgets::countdown_ring::CountdownRing;
use martensite::widgets::digital_clock::DigitalClock;
use martensite::widgets::empty_state::EmptyState;
use martensite::widgets::flashcard::Flashcard;
use martensite::widgets::gauge::Gauge;
use martensite::widgets::hover_card::HoverCard;
use martensite::widgets::kbd::Kbd;
use martensite::widgets::lcd_number::LcdNumber;
use martensite::widgets::led_matrix::LedMatrix;
use martensite::widgets::level_bar::LevelBar;
use martensite::widgets::odometer::Odometer;
use martensite::widgets::presence::{Presence, PresenceStatus};
use martensite::widgets::progress::ProgressBar;
use martensite::widgets::rating_summary::RatingSummary;
use martensite::widgets::result_page::{ResultPage, ResultStatus};
use martensite::widgets::scratch_card::ScratchCard;
use martensite::widgets::signal_strength::SignalStrength;
use martensite::widgets::skeleton::Skeleton;
use martensite::widgets::split_flap::SplitFlap;
use martensite::widgets::stack_light::{Lamp, StackLight};
use martensite::widgets::statistic::{Statistic, Trend};
use martensite::widgets::status_dot::{Status, StatusDot};
use martensite::widgets::stopwatch::Stopwatch;
use martensite::widgets::text::Text;
use martensite::widgets::thermometer::Thermometer;
use martensite::widgets::toast::{Toast, ToastHost};
use martensite::widgets::tooltip::Tooltip;
use martensite::widgets::typing_indicator::TypingIndicator;
use martensite::widgets::vu_meter::VuMeter;
use martensite::widgets::watermark::Watermark;
use martensite::widgets::weather::{Weather, WeatherCondition};
use martensite::widgets::world_clock::WorldClock;
use martensite::widgets::Time;

/// Feedback & display showcase entries — `(display name, live widget)`.
pub fn entries() -> Vec<(&'static str, Box<dyn Widget>)> {
    vec![
        (
            "Activity Ring",
            Box::new(
                ActivityRing::new()
                    .label("Shift goals")
                    .ring("Output", 0.82, [92, 200, 120, 255])
                    .ring("Quality", 0.64, [90, 160, 240, 255])
                    .ring("Uptime", 0.91, [250, 190, 60, 255]),
            ),
        ),
        ("Alarm Panel", {
            let mut p = AlarmPanel::new().label("Line 2 alarms");
            p.push(Alarm::new(Severity::Error, "Bearing temp HI").source("P-201"));
            p.push(Alarm::new(Severity::Warning, "Low discharge flow").source("FT-104"));
            p.push(Alarm::new(Severity::Info, "Shift change logged"));
            Box::new(p)
        }),
        (
            "Analog Clock",
            Box::new(AnalogClock::new().time(14, 32, 45).show_seconds(true)),
        ),
        ("Avatar", Box::new(Avatar::new("Iris Chen").size(40.0))),
        (
            "Avatar Group",
            Box::new(
                AvatarGroup::new()
                    .member(Avatar::new("Iris Chen"))
                    .member(Avatar::new("Tom Alvarez"))
                    .member(Avatar::new("Sam Wu"))
                    .member(Avatar::new("Ana Ruiz"))
                    .max_count(3)
                    .label("On shift"),
            ),
        ),
        (
            "Badge",
            Box::new(Badge::wrap(Text::new("Alarms")).with_count(4)),
        ),
        (
            "Banner",
            Box::new(
                Banner::new(Severity::Warning, "Tank T-3 above level setpoint").dismissible(true),
            ),
        ),
        (
            "Battery",
            Box::new(
                Battery::new()
                    .level(0.62)
                    .charging(true)
                    .label("UPS backup"),
            ),
        ),
        (
            "Chip",
            Box::new(Chip::new("Auto").kind(ChipKind::Filter).selected(true)),
        ),
        (
            "Chip Group",
            Box::new(
                ChipGroup::new()
                    .chip(Chip::new("Run"))
                    .chip(Chip::new("Hold"))
                    .chip(Chip::new("Purge"))
                    .selection(ChipSelection::Single)
                    .label("Cycle state"),
            ),
        ),
        (
            "Compass",
            Box::new(Compass::new().heading(235.0).label("Wind direction")),
        ),
        ("Confetti", {
            let mut c = Confetti::new().count(120).label("Milestone");
            c.burst(0.5, 0.4);
            Box::new(c)
        }),
        (
            "Countdown",
            Box::new(
                Countdown::new(Duration::from_secs(90))
                    .warn_under(Duration::from_secs(15))
                    .label("Purge window"),
            ),
        ),
        (
            "Countdown Ring",
            Box::new(
                CountdownRing::new(Duration::from_secs(120))
                    .warn_under(Duration::from_secs(20))
                    .label("Cure time"),
            ),
        ),
        (
            "Digital Clock",
            Box::new(
                DigitalClock::new()
                    .time(Time {
                        hour: 14,
                        minute: 32,
                    })
                    .show_seconds(true)
                    .running(true),
            ),
        ),
        (
            "Empty State",
            Box::new(
                EmptyState::new("No active alarms")
                    .icon("✓")
                    .description("All channels within limits")
                    .action("View history"),
            ),
        ),
        (
            "Flashcard",
            Box::new(Flashcard::new("LOTO step 3", "Verify zero-energy state")),
        ),
        (
            "Gauge",
            Box::new(
                Gauge::new()
                    .range(0.0, 160.0)
                    .value(96.0)
                    .label("Discharge PSI")
                    .zones(110.0, 140.0)
                    .ticks(true),
            ),
        ),
        (
            "Hover Card",
            Box::new(
                HoverCard::new("PT-101", "Pressure transmitter · 4–20 mA · cal. 12 d ago")
                    .with_delay(Duration::from_millis(250)),
            ),
        ),
        ("Kbd", Box::new(Kbd::new("Ctrl+Shift+R"))),
        (
            "LCD Number",
            Box::new(LcdNumber::new().value(1240.5).digits(6).decimals(1)),
        ),
        ("LED Matrix", {
            let mut m = LedMatrix::new(9, 5).on_color([92, 200, 120, 255]);
            for c in 0..9 {
                m.set(c, c % 5, true);
                m.set(8 - c, c % 5, true);
            }
            Box::new(m)
        }),
        (
            "Level Bar",
            Box::new(
                LevelBar::new()
                    .value(0.62)
                    .zones(0.25, 0.6, 0.9)
                    .segments(10),
            ),
        ),
        (
            "Odometer",
            Box::new(Odometer::new().digits(6).value(128430).label("Total units")),
        ),
        (
            "Presence",
            Box::new(Presence::new("R. Okafor", PresenceStatus::Online).status_text("On shift")),
        ),
        ("Progress Bar", Box::new(ProgressBar::new().value(0.68))),
        (
            "Rating Summary",
            Box::new(
                RatingSummary::new()
                    .counts([38, 41, 22, 9, 4])
                    .label("Shift feedback"),
            ),
        ),
        (
            "Result Page",
            Box::new(
                ResultPage::new(ResultStatus::Success)
                    .title("Batch 0422 complete")
                    .subtitle("1,240 units · 0 rejects · 47 min cycle")
                    .action("Back to line view"),
            ),
        ),
        (
            "Scratch Card",
            Box::new(ScratchCard::new(Text::new("SOP-114 rev. C")).label("Reveal procedure")),
        ),
        (
            "Signal Strength",
            Box::new(SignalStrength::new().level(3).label("AGV uplink")),
        ),
        ("Skeleton", Box::new(Skeleton::lines(3))),
        (
            "Split Flap",
            Box::new(SplitFlap::new().cells(6).text("LINE A")),
        ),
        (
            "Stack Light",
            Box::new(
                StackLight::new()
                    .lamp(Lamp::new("Fault", [230, 70, 60, 255]))
                    .lamp(
                        Lamp::new("Warn", [250, 190, 60, 255])
                            .lit(true)
                            .flashing(true),
                    )
                    .lamp(Lamp::new("Run", [92, 200, 120, 255]).lit(true)),
            ),
        ),
        (
            "Statistic",
            Box::new(
                Statistic::new("Line OEE", "87.4")
                    .suffix("%")
                    .trend(Trend::Up, "+1.2% vs. last shift"),
            ),
        ),
        (
            "Status Dot",
            Box::new(StatusDot::new("Conveyor C-2").status(Status::Ok)),
        ),
        (
            "Stopwatch",
            Box::new(Stopwatch::new().running(true).label("Cycle time")),
        ),
        (
            "Text",
            Box::new(Text::new("PACKAGING LINE A — RUNNING").font_size(14.0)),
        ),
        (
            "Thermometer",
            Box::new(
                Thermometer::new()
                    .range(-20.0, 120.0)
                    .value(64.5)
                    .units("°C")
                    .warning(0.7)
                    .critical(0.9)
                    .ticks(5),
            ),
        ),
        ("Toast", {
            let mut host = ToastHost::new();
            host.push(Toast::new(Severity::Info, "Recipe R-88 loaded"));
            host.push(Toast::new(Severity::Warning, "Sensor drift on FT-104"));
            Box::new(host)
        }),
        (
            "Tooltip",
            Box::new(
                Tooltip::new(
                    Text::new("PT-101"),
                    "Pressure transmitter — hover for detail",
                )
                .delay_ms(200),
            ),
        ),
        (
            "Typing Indicator",
            Box::new(TypingIndicator::new().active(true).label("Remote operator")),
        ),
        (
            "VU Meter",
            Box::new(
                VuMeter::new()
                    .channels(4)
                    .levels([0.85, 0.6, 0.35, 0.15])
                    .peak_hold(1.5)
                    .label("Mixer bus"),
            ),
        ),
        ("Watermark", Box::new(Watermark::new("MARTENSITE DEMO"))),
        (
            "Weather",
            Box::new(
                Weather::new()
                    .location("Plant 2 · Pittsburgh")
                    .condition(WeatherCondition::PartlyCloudy)
                    .temperature(68.0)
                    .fahrenheit(true)
                    .hi_lo(74.0, 55.0),
            ),
        ),
        (
            "World Clock",
            Box::new(
                WorldClock::new()
                    .zone("Pittsburgh", -300)
                    .zone("Berlin", 60)
                    .zone("Shanghai", 480)
                    .with_utc(Time {
                        hour: 13,
                        minute: 7,
                    }),
            ),
        ),
    ]
}
