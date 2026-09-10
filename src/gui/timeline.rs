use crate::domain::{Event, DECLINED_ROOM_MARKER, ONLINE_MEETING_MARKER};
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
const MINIMUM_RENDERED_OVERLAP_FOR_COLUMNS: i32 = 2;
const COMPACT_EVENT_HEIGHT: i32 = 48;

fn event_button_width(group_size: i32, spacing: i32) -> i32 {
    ((TIMELINE_MIN_WIDTH - (spacing * (group_size + 1))) / group_size).max(200)
}

fn event_vertical_geometry(
    start_minutes: i32,
    duration_minutes: i32,
    shares_boundary: bool,
) -> (i32, i32) {
    let y_position = (start_minutes * HOUR_HEIGHT) / 60 - if shares_boundary { 1 } else { 0 };
    let height =
        ((duration_minutes * HOUR_HEIGHT) / 60 + if shares_boundary { 1 } else { 0 }).max(30);

    (y_position, height)
}

fn event_start_and_duration_minutes(event: &Event, start_hour: i32) -> (i32, i32) {
    let event_start = event.start_timestamp.with_timezone(&Local);
    let event_end = event.end_timestamp.with_timezone(&Local);
    let start_minutes = (event_start.hour() as i32 - start_hour) * 60 + event_start.minute() as i32;
    let duration_minutes = event_end.signed_duration_since(event_start).num_minutes() as i32;

    (start_minutes, duration_minutes)
}

fn shares_timeline_boundary(event: &Event, events: &[Event]) -> bool {
    events
        .iter()
        .any(|other| other.end_timestamp == event.start_timestamp)
}

fn rendered_event_geometry(event: &Event, events: &[Event], start_hour: i32) -> (i32, i32) {
    let (start_minutes, duration_minutes) = event_start_and_duration_minutes(event, start_hour);
    event_vertical_geometry(
        start_minutes,
        duration_minutes,
        shares_timeline_boundary(event, events),
    )
}

fn rendered_events_overlap(a: &Event, b: &Event, events: &[Event], start_hour: i32) -> bool {
    let (a_y, a_height) = rendered_event_geometry(a, events, start_hour);
    let (b_y, b_height) = rendered_event_geometry(b, events, start_hour);

    let overlap = (a_y + a_height).min(b_y + b_height) - a_y.max(b_y);
    overlap >= MINIMUM_RENDERED_OVERLAP_FOR_COLUMNS
}

#[derive(Debug)]
struct PositionedEvent<'a> {
    event: &'a Event,
    lane_index: usize,
    lane_count: usize,
}

fn sorted_events(events: &[Event]) -> Vec<&Event> {
    let mut sorted_events: Vec<_> = events.iter().collect();
    sorted_events.sort_by(|a, b| {
        a.start_timestamp
            .cmp(&b.start_timestamp)
            .then_with(|| b.end_timestamp.cmp(&a.end_timestamp))
            .then_with(|| a.summary.cmp(&b.summary))
    });
    sorted_events
}

fn overlapping_event_groups<'a>(events: &'a [Event], start_hour: i32) -> Vec<Vec<&'a Event>> {
    let mut event_groups: Vec<Vec<&Event>> = Vec::new();

    for event in sorted_events(events) {
        let mut matching_group_indices: Vec<usize> = event_groups
            .iter()
            .enumerate()
            .filter_map(|(index, group)| {
                let overlaps_group = group
                    .iter()
                    .any(|existing| rendered_events_overlap(event, existing, events, start_hour));

                overlaps_group.then_some(index)
            })
            .collect();

        if matching_group_indices.is_empty() {
            event_groups.push(vec![event]);
            continue;
        }

        let first_group_index = matching_group_indices.remove(0);
        event_groups[first_group_index].push(event);

        for group_index in matching_group_indices.into_iter().rev() {
            let mut merged_group = event_groups.remove(group_index);
            event_groups[first_group_index].append(&mut merged_group);
        }
    }

    event_groups
}

fn positioned_events<'a>(events: &'a [Event], start_hour: i32) -> Vec<PositionedEvent<'a>> {
    let mut positioned_events = Vec::new();

    for mut group in overlapping_event_groups(events, start_hour) {
        group.sort_by(|a, b| {
            a.start_timestamp
                .cmp(&b.start_timestamp)
                .then_with(|| b.end_timestamp.cmp(&a.end_timestamp))
                .then_with(|| a.summary.cmp(&b.summary))
        });

        let mut lanes: Vec<Vec<&Event>> = Vec::new();
        let mut group_positions: Vec<(&Event, usize)> = Vec::new();

        for event in group {
            let maybe_lane_index = lanes.iter().position(|lane| {
                lane.iter()
                    .all(|existing| !rendered_events_overlap(event, existing, events, start_hour))
            });

            let lane_index = match maybe_lane_index {
                Some(index) => {
                    lanes[index].push(event);
                    index
                }
                None => {
                    lanes.push(vec![event]);
                    lanes.len() - 1
                }
            };

            group_positions.push((event, lane_index));
        }

        let lane_count = lanes.len();
        positioned_events.extend(group_positions.into_iter().map(|(event, lane_index)| {
            PositionedEvent {
                event,
                lane_index,
                lane_count,
            }
        }));
    }

    positioned_events
}

fn compact_room_label(event: &Event) -> Option<String> {
    let label = match event.metadata.rooms.as_slice() {
        [] => None,
        [room] => Some(room.name.clone()),
        [first, rest @ ..] => Some(format!("{} +{}", first.name.as_str(), rest.len())),
    }?;

    if event.metadata.has_declined_room() {
        Some(format!("{}{}", label, DECLINED_ROOM_MARKER))
    } else {
        Some(label)
    }
}

fn event_button_text(event: &Event, show_time: bool, compact: bool) -> String {
    let markers = if event.meeturl.is_some() {
        ONLINE_MEETING_MARKER
    } else {
        ""
    };
    let room_suffix = compact_room_label(event);

    let event_text = if show_time {
        format!("{}  {}{}", event_time_range(event), event.summary, markers)
    } else {
        format!("{}{}", event.summary, markers)
    };

    match room_suffix {
        Some(room) if compact => format!("{} - {}", event_text, room),
        Some(room) => format!("{}\n{}", event_text, room),
        None => event_text,
    }
}

fn event_time_range(event: &Event) -> String {
    let event_start = event.start_timestamp.with_timezone(&Local);
    let event_end = event.end_timestamp.with_timezone(&Local);

    format!(
        "{} - {}",
        event_start.format("%H:%M"),
        event_end.format("%H:%M")
    )
}

fn participant_list(participants: &[crate::domain::Participant]) -> String {
    participants
        .iter()
        .map(|participant| participant.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn room_list(participants: &[crate::domain::Participant]) -> String {
    participants
        .iter()
        .map(|participant| participant.display_text())
        .collect::<Vec<_>>()
        .join(", ")
}

pub struct TimelineView {
    pub container: gtk::Box,
}

impl TimelineView {
    fn selectable_label(text: &str, color: &str, extra_css: &str) -> gtk::Label {
        let label = gtk::Label::new(Some(text));
        label.set_selectable(true);
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_max_width_chars(58);
        style_label_with_css(&label, color, extra_css);

        label
    }

    fn add_detail_row(container: &gtk::Box, label: &str, value: &str) {
        if value.trim().is_empty() {
            return;
        }

        let row = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let label_widget =
            Self::selectable_label(label, TEXT_SUBTLE, "font-size: 11px; font-weight: 700;");
        let value_widget = Self::selectable_label(value, "#242a31", "font-size: 13px;");

        row.append(&label_widget);
        row.append(&value_widget);
        container.append(&row);
    }

    fn add_description(container: &gtk::Box, description: &str) {
        let trimmed_description = description.trim();
        if trimmed_description.is_empty() {
            return;
        }

        let label = Self::selectable_label(
            "Description",
            TEXT_SUBTLE,
            "font-size: 11px; font-weight: 700;",
        );
        container.append(&label);

        let scrolled_window = gtk::ScrolledWindow::new();
        scrolled_window.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scrolled_window.set_min_content_width(420);
        scrolled_window.set_min_content_height(90);
        scrolled_window.set_max_content_height(220);

        let text_view = gtk::TextView::new();
        text_view.set_editable(false);
        text_view.set_cursor_visible(true);
        text_view.set_wrap_mode(gtk::WrapMode::WordChar);
        text_view.buffer().set_text(trimmed_description);
        load_css(
            &text_view.style_context(),
            "textview, textview text { \
                background-color: #ffffff; \
                color: #242a31; \
                font-size: 13px; \
            }",
        );

        scrolled_window.set_child(Some(&text_view));
        scrolled_window.set_vexpand(true);
        container.append(&scrolled_window);
    }

    fn create_event_popover(event: &Event, button: &gtk::Button) -> gtk::Popover {
        let popover = gtk::Popover::new();
        popover.set_parent(button);
        popover.set_position(gtk::PositionType::Bottom);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
        content.set_margin_start(12);
        content.set_margin_end(12);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_size_request(460, -1);

        let title = Self::selectable_label(
            &event.summary,
            "#242a31",
            "font-size: 15px; font-weight: 700;",
        );
        content.append(&title);

        Self::add_detail_row(&content, "Time", &event_time_range(event));

        if let Some(organizer) = &event.metadata.organizer {
            Self::add_detail_row(&content, "Organizer", organizer);
        }
        if !event.metadata.rooms.is_empty() {
            let label = if event.metadata.rooms.len() == 1 {
                "Room"
            } else {
                "Rooms"
            };
            Self::add_detail_row(&content, label, &room_list(&event.metadata.rooms));
        }
        if !event.metadata.required_attendees.is_empty() {
            Self::add_detail_row(
                &content,
                "Required",
                &participant_list(&event.metadata.required_attendees),
            );
        }
        if !event.metadata.optional_attendees.is_empty() {
            Self::add_detail_row(
                &content,
                "Optional",
                &participant_list(&event.metadata.optional_attendees),
            );
        }

        Self::add_description(&content, &event.description);

        if let Some(meet_url) = &event.meeturl {
            let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let join_button = gtk::Button::with_label("Join");
            let url = meet_url.clone();
            join_button.connect_clicked(move |_| {
                open_meeting(&url);
            });
            actions.append(&join_button);

            let link_button = gtk::LinkButton::with_label(meet_url, "Meeting link");
            actions.append(&link_button);
            content.append(&actions);
        }

        popover.set_child(Some(&content));
        popover
    }

    fn create_event_button(event: &Event, width: i32, height: i32, show_time: bool) -> gtk::Button {
        let button = gtk::Button::new();
        button.set_size_request(width, height.max(30));

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

        let compact = show_time && height < COMPACT_EVENT_HEIGHT;
        let text = event_button_text(event, show_time, compact);

        let label = gtk::Label::new(Some(&text));
        label.set_wrap(!compact);
        if compact {
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_lines(1);
        } else {
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        }
        label.set_justify(gtk::Justification::Left);
        label.set_xalign(0.0);
        label.set_margin_start(8);
        label.set_margin_end(8);
        label.set_margin_top(4);
        label.set_margin_bottom(4);
        style_label(&label, palette.text);
        button.set_child(Some(&label));

        let popover = Self::create_event_popover(event, &button);
        let popover_for_cleanup = popover.clone();
        button.connect_destroy(move |_| {
            popover_for_cleanup.unparent();
        });
        button.connect_clicked(move |_| {
            popover.popup();
        });

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
        all_day_container.append(&all_day_label);

        let all_day_events_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        all_day_events_box.set_size_request(-1, if all_day_events.is_empty() { 12 } else { 40 });

        if !all_day_events.is_empty() {
            let button_width = ((TIMELINE_MIN_WIDTH - (6 * (all_day_events.len() as i32 + 1)))
                / all_day_events.len() as i32)
                .max(150);

            for event in all_day_events {
                let button = Self::create_event_button(&event, button_width, 40, false);
                button.set_hexpand(true);
                all_day_events_box.append(&button);
            }
        }

        all_day_container.append(&all_day_events_box);
        container.append(&all_day_container);

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
        meeting_area.put(&background_box, 0.0, 0.0);

        let timeline_rail = gtk::Box::new(gtk::Orientation::Vertical, 0);
        timeline_rail.set_size_request(2, (end_hour - start_hour) * HOUR_HEIGHT);
        load_css(
            &timeline_rail.style_context(),
            &format!(
                "box {{ background-color: {}; margin: 0; padding: 0; }}",
                TIMELINE_RAIL
            ),
        );
        meeting_area.put(&timeline_rail, 0.0, 0.0);

        for hour in start_hour..=end_hour {
            let y_position = (hour - start_hour) * HOUR_HEIGHT;

            let label = gtk::Label::new(Some(&format!("{:02}:00", hour)));
            label.set_xalign(1.0);
            label.set_margin_end(5);
            style_label_with_css(&label, TEXT_SUBTLE, "font-size: 13px;");
            time_column.put(&label, 0.0, f64::from(y_position));

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
            meeting_area.put(&separator, 0.0, f64::from(y_position));
        }

        for positioned_event in positioned_events(&regular_events, start_hour) {
            let button_width = event_button_width(positioned_event.lane_count as i32, spacing);
            let (y_position, height) =
                rendered_event_geometry(positioned_event.event, &regular_events, start_hour);
            let x_position =
                spacing + (button_width + spacing) * positioned_event.lane_index as i32;

            let button =
                Self::create_event_button(positioned_event.event, button_width, height, true);
            meeting_area.put(&button, f64::from(x_position), f64::from(y_position));
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

                meeting_area.put(&current_time_marker, 0.0, f64::from(y_position));

                let current_time_cap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                current_time_cap.set_size_request(8, 8);
                load_css(
                    &current_time_cap.style_context(),
                    &format!(
                        "box {{ background-color: {}; border-radius: 4px; margin: 0; padding: 0; }}",
                        CURRENT_TIME_MARKER
                    ),
                );
                meeting_area.put(&current_time_cap, 0.0, f64::from(y_position - 3));
            }
        }

        let total_height = (end_hour - start_hour) * HOUR_HEIGHT;

        layout_box.append(&time_column);
        meeting_area.set_hexpand(true);
        meeting_area.set_vexpand(true);
        layout_box.append(&meeting_area);
        layout_box.set_size_request(-1, total_height);

        layout_box.set_hexpand(true);
        layout_box.set_vexpand(true);
        container.append(&layout_box);

        Self { container }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{EventMetadata, Participant, ResponseStatus};
    use chrono_tz::Tz;

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

    fn local_datetime(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Tz> {
        // Fixtures describe displayed wall-clock times, regardless of the host timezone.
        // Store them in UTC to exercise the formatter's conversion back to Local.
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .unwrap()
            .with_timezone(&chrono_tz::UTC)
    }

    fn event(summary: &str, start_hour: u32, start_minute: u32, end_minute: u32) -> Event {
        event_between(summary, start_hour, start_minute, start_hour, end_minute)
    }

    fn event_between(
        summary: &str,
        start_hour: u32,
        start_minute: u32,
        end_hour: u32,
        end_minute: u32,
    ) -> Event {
        Event {
            summary: summary.to_string(),
            description: String::new(),
            location: String::new(),
            meeturl: None,
            metadata: EventMetadata::empty(),
            all_day: false,
            start_timestamp: local_datetime(2026, 5, 19, start_hour, start_minute),
            end_timestamp: local_datetime(2026, 5, 19, end_hour, end_minute),
        }
    }

    fn event_with_rooms(
        summary: &str,
        start_hour: u32,
        start_minute: u32,
        end_minute: u32,
        rooms: Vec<Participant>,
    ) -> Event {
        Event {
            metadata: EventMetadata {
                rooms,
                ..EventMetadata::empty()
            },
            ..event(summary, start_hour, start_minute, end_minute)
        }
    }

    #[test]
    fn adjacent_short_events_overlap_after_minimum_height_expansion() {
        let first = event("first", 11, 0, 15);
        let second = event("second", 11, 15, 30);
        let events = vec![first.clone(), second.clone()];

        assert!(rendered_events_overlap(&first, &second, &events, 8));
    }

    #[test]
    fn adjacent_long_events_do_not_overlap_after_rendering() {
        let first = event("first", 11, 0, 30);
        let second = event("second", 11, 30, 59);
        let events = vec![first.clone(), second.clone()];

        assert!(!rendered_events_overlap(&first, &second, &events, 8));
    }

    #[test]
    fn positioned_events_reuse_lanes_for_non_overlapping_events_in_same_group() {
        let events = vec![
            event_between("blocker", 15, 0, 18, 0),
            event_between("development operations", 15, 0, 16, 0),
            event_between("room setup", 16, 0, 16, 30),
            event_between("room handoff", 16, 30, 17, 0),
        ];

        let positions = positioned_events(&events, 8);
        let event_position = |summary: &str| {
            positions
                .iter()
                .find(|position| position.event.summary == summary)
                .unwrap()
        };

        assert_eq!(event_position("blocker").lane_index, 0);
        assert_eq!(event_position("development operations").lane_index, 1);
        assert_eq!(event_position("room setup").lane_index, 1);
        assert_eq!(event_position("room handoff").lane_index, 1);
        assert!(positions.iter().all(|position| position.lane_count == 2));
    }

    #[test]
    fn positioned_events_keep_rendered_overlaps_in_separate_lanes() {
        let events = vec![event("first", 11, 0, 15), event("second", 11, 15, 30)];

        let positions = positioned_events(&events, 8);

        assert_eq!(positions[0].lane_count, 2);
        assert_eq!(positions[1].lane_count, 2);
        assert_ne!(positions[0].lane_index, positions[1].lane_index);
    }

    #[test]
    fn positioned_events_do_not_stagger_sequential_meetings() {
        let events = vec![
            event_between("first", 11, 0, 11, 30),
            event_between("second", 11, 30, 12, 0),
            event_between("third", 12, 0, 12, 30),
        ];

        let positions = positioned_events(&events, 8);

        assert_eq!(positions.len(), 3);
        assert!(positions.iter().all(|position| position.lane_index == 0));
        assert!(positions.iter().all(|position| position.lane_count == 1));
    }

    #[test]
    fn compact_event_text_keeps_room_inline_without_accepted_state() {
        let event = event_with_rooms(
            "Planning",
            16,
            0,
            30,
            vec![Participant {
                name: "Room 12".to_string(),
                response: Some(ResponseStatus::Accepted),
            }],
        );

        assert_eq!(
            event_button_text(&event, true, true),
            "16:00 - 16:30  Planning - Room 12"
        );
    }

    #[test]
    fn compact_event_text_marks_declined_room_inline() {
        let event = event_with_rooms(
            "Planning",
            16,
            0,
            30,
            vec![Participant {
                name: "Room 12".to_string(),
                response: Some(ResponseStatus::Declined),
            }],
        );

        assert_eq!(
            event_button_text(&event, true, true),
            "16:00 - 16:30  Planning - Room 12 !"
        );
    }

    #[test]
    fn regular_event_text_keeps_room_on_second_line() {
        let event = event_with_rooms(
            "Planning",
            16,
            0,
            30,
            vec![Participant {
                name: "Room 12".to_string(),
                response: Some(ResponseStatus::Accepted),
            }],
        );

        assert_eq!(
            event_button_text(&event, true, false),
            "16:00 - 16:30  Planning\nRoom 12"
        );
    }
}
