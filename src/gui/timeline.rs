use crate::domain::{Event, ONLINE_MEETING_MARKER};
use crate::gui::actions::open_meeting;
use crate::gui::styles::{
    event_palette, load_css, style_label, style_label_with_css, CURRENT_TIME_MARKER, TEXT_SUBTLE,
    TIMELINE_BACKGROUND, TIMELINE_GRID, TIMELINE_GRID_STRONG, TIMELINE_RAIL,
};
use chrono::prelude::*;
use gtk::prelude::*;

pub const HOUR_HEIGHT: i32 = 80;
pub const TIMELINE_MIN_WIDTH: i32 = 600;
pub const DAY_MIN_WIDTH: i32 = 700;

fn event_button_width(group_size: i32, spacing: i32) -> i32 {
    ((TIMELINE_MIN_WIDTH - (spacing * (group_size + 1))) / group_size).max(200)
}

fn event_vertical_geometry(
    start_minutes: i32,
    duration_minutes: i32,
    touches_previous_event: bool,
) -> (i32, i32) {
    let y_position =
        (start_minutes * HOUR_HEIGHT) / 60 - if touches_previous_event { 1 } else { 0 };
    let height = ((duration_minutes * HOUR_HEIGHT) / 60
        + if touches_previous_event { 1 } else { 0 })
    .max(30);

    (y_position, height)
}

pub struct TimelineView {
    pub container: gtk::Box,
}

impl TimelineView {
    fn create_event_button(event: &Event, width: i32, height: i32, show_time: bool) -> gtk::Button {
        let button = gtk::Button::new();
        button.set_size_request(width, height.max(30));

        let trimmed_description = event.description.trim();
        if !trimmed_description.is_empty() {
            button.set_tooltip_text(Some(trimmed_description));
        }

        let palette = event_palette(event);

        load_css(
            &button.style_context(),
            &format!(
                "button {{ \
                    background: {}; \
                    border: 1px solid {}; \
                    border-radius: 5px; \
                    box-shadow: inset 0 1px rgba(255, 255, 255, 0.34); \
                    color: {}; \
                    text-shadow: none; \
                }} \
                button:hover {{ border-color: {}; }}",
                palette.background, palette.border, palette.text, palette.text
            ),
        );

        let text = if show_time {
            let event_start = event.start_timestamp.with_timezone(&Local);
            let event_end = event.end_timestamp.with_timezone(&Local);
            let time_str = format!(
                "{} - {}",
                event_start.format("%H:%M"),
                event_end.format("%H:%M")
            );
            format!(
                "{}  {}{}",
                time_str,
                event.summary,
                if event.meeturl.is_some() {
                    ONLINE_MEETING_MARKER
                } else {
                    ""
                }
            )
        } else {
            format!(
                "{}{}",
                event.summary,
                if event.meeturl.is_some() {
                    ONLINE_MEETING_MARKER
                } else {
                    ""
                }
            )
        };

        let label = gtk::Label::new(Some(&text));
        label.set_line_wrap(true);
        label.set_line_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_justify(gtk::Justification::Left);
        label.set_xalign(0.0);
        label.set_margin_start(8);
        label.set_margin_end(8);
        label.set_margin_top(4);
        label.set_margin_bottom(4);
        style_label(&label, palette.text);
        button.add(&label);

        if let Some(meet_url) = &event.meeturl {
            let url = meet_url.clone();
            button.connect_clicked(move |_| {
                open_meeting(&url);
            });
        }

        button
    }

    pub fn new(events: Vec<Event>, start_hour: i32, end_hour: i32, is_today: bool) -> Self {
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        container.set_margin_start(12);
        container.set_margin_end(12);
        container.set_margin_top(12);
        container.set_margin_bottom(12);

        let (all_day_events, regular_events): (Vec<_>, Vec<_>) = events
            .into_iter()
            .partition(|e| e.start_timestamp.time() == e.end_timestamp.time());

        let all_day_container = gtk::Box::new(gtk::Orientation::Vertical, 4);
        all_day_container.set_margin_bottom(if all_day_events.is_empty() { 6 } else { 12 });

        let all_day_label = gtk::Label::new(Some("All Day"));
        all_day_label.set_xalign(0.0);
        all_day_label.set_margin_bottom(2);
        all_day_label.set_markup("All Day");
        style_label_with_css(&all_day_label, TEXT_SUBTLE, "font-size: 13px;");
        all_day_container.pack_start(&all_day_label, false, false, 0);

        let all_day_events_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        all_day_events_box.set_size_request(-1, if all_day_events.is_empty() { 12 } else { 40 });

        if !all_day_events.is_empty() {
            let button_width = ((TIMELINE_MIN_WIDTH - (6 * (all_day_events.len() as i32 + 1)))
                / all_day_events.len() as i32)
                .max(150);

            for event in all_day_events {
                let button = Self::create_event_button(&event, button_width, 40, false);
                all_day_events_box.pack_start(&button, true, true, 0);
            }
        }

        all_day_container.pack_start(&all_day_events_box, false, false, 0);
        container.pack_start(&all_day_container, false, false, 0);

        let time_label_width: i32 = 50;
        let spacing: i32 = 10;

        let layout_box = gtk::Box::new(gtk::Orientation::Horizontal, spacing);
        layout_box.set_hexpand(true);

        let time_column = gtk::Fixed::new();
        time_column.set_size_request(time_label_width, -1);

        let meeting_area = gtk::Fixed::new();
        meeting_area.set_hexpand(true);
        meeting_area.set_size_request(TIMELINE_MIN_WIDTH, (end_hour - start_hour) * HOUR_HEIGHT);

        let background_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        background_box.set_size_request(TIMELINE_MIN_WIDTH, (end_hour - start_hour) * HOUR_HEIGHT);
        let css = format!(
            "box {{ \
                background-color: {}; \
                margin: 0; \
                padding: 0; \
            }}",
            TIMELINE_BACKGROUND
        );
        load_css(&background_box.style_context(), &css);
        meeting_area.put(&background_box, 0, 0);

        let timeline_rail = gtk::Box::new(gtk::Orientation::Vertical, 0);
        timeline_rail.set_size_request(2, (end_hour - start_hour) * HOUR_HEIGHT);
        load_css(
            &timeline_rail.style_context(),
            &format!(
                "box {{ background-color: {}; margin: 0; padding: 0; }}",
                TIMELINE_RAIL
            ),
        );
        meeting_area.put(&timeline_rail, 0, 0);

        for hour in start_hour..=end_hour {
            let y_position = (hour - start_hour) * HOUR_HEIGHT;

            let label = gtk::Label::new(Some(&format!("{:02}:00", hour)));
            label.set_xalign(1.0);
            label.set_margin_end(5);
            style_label_with_css(&label, TEXT_SUBTLE, "font-size: 13px;");
            time_column.put(&label, 0, y_position);

            let separator = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            separator.set_size_request(TIMELINE_MIN_WIDTH, -1);

            let css = if hour == start_hour || hour == end_hour {
                format!(
                    "box {{ background-color: {}; min-height: 2px; margin: 0; padding: 0; }}",
                    TIMELINE_GRID_STRONG
                )
            } else {
                format!(
                    "box {{ background-color: {}; min-height: 1px; margin: 0; padding: 0; }}",
                    TIMELINE_GRID
                )
            };

            load_css(&separator.style_context(), &css);
            meeting_area.put(&separator, 0, y_position);
        }

        let mut event_groups: Vec<Vec<&Event>> = Vec::new();
        for event in &regular_events {
            let mut found_group = false;
            for group in &mut event_groups {
                let overlaps = group.iter().any(|existing| {
                    !(event.end_timestamp <= existing.start_timestamp
                        || event.start_timestamp >= existing.end_timestamp)
                });

                if overlaps {
                    group.push(event);
                    found_group = true;
                    break;
                }
            }

            if !found_group {
                event_groups.push(vec![event]);
            }
        }

        for group in event_groups {
            let group_size = group.len() as i32;
            let button_width = event_button_width(group_size, spacing);

            for (index, event) in group.iter().enumerate() {
                let event_start = event.start_timestamp.with_timezone(&Local);
                let event_end = event.end_timestamp.with_timezone(&Local);

                let start_minutes =
                    (event_start.hour() as i32 - start_hour) * 60 + event_start.minute() as i32;
                let duration_minutes =
                    event_end.signed_duration_since(event_start).num_minutes() as i32;

                let touches_previous_event = regular_events
                    .iter()
                    .any(|other| other.end_timestamp == event.start_timestamp);
                let (y_position, height) = event_vertical_geometry(
                    start_minutes,
                    duration_minutes,
                    touches_previous_event,
                );
                let x_position = spacing + (button_width + spacing) * index as i32;

                let button = Self::create_event_button(event, button_width, height, true);
                meeting_area.put(&button, x_position, y_position);
            }
        }

        if is_today {
            let now = Local::now();
            let current_hour = now.hour() as i32;
            let current_minute = now.minute() as i32;
            if current_hour >= start_hour && current_hour <= end_hour {
                let minutes_from_start = (current_hour - start_hour) * 60 + current_minute;
                let y_position = (minutes_from_start * HOUR_HEIGHT) / 60;

                let current_time_marker = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                current_time_marker.set_size_request(TIMELINE_MIN_WIDTH, -1);
                load_css(
                    &current_time_marker.style_context(),
                    &format!(
                        "box {{ background-color: {}; min-height: 2px; margin: 0; padding: 0; }}",
                        CURRENT_TIME_MARKER
                    ),
                );

                meeting_area.put(&current_time_marker, 0, y_position);

                let current_time_cap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                current_time_cap.set_size_request(8, 8);
                load_css(
                    &current_time_cap.style_context(),
                    &format!(
                        "box {{ background-color: {}; border-radius: 4px; margin: 0; padding: 0; }}",
                        CURRENT_TIME_MARKER
                    ),
                );
                meeting_area.put(&current_time_cap, 0, y_position - 3);
            }
        }

        let total_height = (end_hour - start_hour) * HOUR_HEIGHT;

        layout_box.pack_start(&time_column, false, false, 0);
        layout_box.pack_start(&meeting_area, true, true, 0);
        layout_box.set_size_request(-1, total_height);

        container.pack_start(&layout_box, true, true, 0);

        Self { container }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_event_geometry_expands_into_shared_boundary() {
        let (y_position, height) = event_vertical_geometry(150, 30, true);

        assert_eq!(y_position, 199);
        assert_eq!(height, 41);
    }

    #[test]
    fn non_adjacent_event_geometry_uses_exact_timeline_position() {
        let (y_position, height) = event_vertical_geometry(150, 30, false);

        assert_eq!(y_position, 200);
        assert_eq!(height, 40);
    }

    #[test]
    fn short_event_geometry_keeps_minimum_height() {
        let (_y_position, height) = event_vertical_geometry(150, 10, false);

        assert_eq!(height, 30);
    }

    #[test]
    fn overlapping_event_width_splits_available_timeline_space() {
        assert_eq!(event_button_width(2, 10), 285);
    }

    #[test]
    fn overlapping_event_width_keeps_minimum_readable_width() {
        assert_eq!(event_button_width(4, 10), 200);
    }
}
