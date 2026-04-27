use crate::domain::Event;
use chrono::Local;
use gtk::prelude::*;

pub const TEXT_PRIMARY: &str = "#242a31";
pub const TEXT_SUBTLE: &str = "#75808c";
pub const TIMELINE_BACKGROUND: &str = "#fbfaf7";
pub const TIMELINE_GRID: &str = "rgba(74, 83, 94, 0.14)";
pub const TIMELINE_GRID_STRONG: &str = "rgba(74, 83, 94, 0.28)";
pub const TIMELINE_RAIL: &str = "rgba(74, 83, 94, 0.18)";
pub const CURRENT_TIME_MARKER: &str = "rgba(218, 55, 48, 0.72)";

pub struct EventPalette {
    pub background: &'static str,
    pub border: &'static str,
    pub text: &'static str,
}

pub fn event_palette(event: &Event) -> EventPalette {
    let now = Local::now();
    if now >= event.start_timestamp && now <= event.end_timestamp {
        EventPalette {
            background: "rgba(245, 184, 82, 0.92)",
            border: "#c17a16",
            text: "#2c2418",
        }
    } else if now < event.start_timestamp {
        EventPalette {
            background: "rgba(204, 217, 246, 0.90)",
            border: "#7f98c9",
            text: "#22304d",
        }
    } else {
        EventPalette {
            background: "rgba(226, 229, 232, 0.78)",
            border: "#c1c8cf",
            text: "#59636f",
        }
    }
}

pub fn load_css(style_context: &gtk::StyleContext, css: &str) {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(css.as_bytes()).unwrap();
    style_context.add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
}

pub fn style_label(label: &gtk::Label, color: &str) {
    style_label_with_css(label, color, "");
}

pub fn style_label_with_css(label: &gtk::Label, color: &str, extra_css: &str) {
    load_css(
        &label.style_context(),
        &format!(
            "label {{ color: {}; text-shadow: none; {} }}",
            color, extra_css
        ),
    );
}
