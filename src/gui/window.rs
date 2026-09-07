use crate::domain::{Event, RefreshState};
use crate::gui::styles::{style_label_with_css, TEXT_PRIMARY};
use crate::gui::timeline::{TimelineView, DAY_MIN_WIDTH, HOUR_HEIGHT};
use chrono::prelude::*;
use gtk::prelude::*;
use std::sync::{Arc, Mutex};

fn calculate_window_height(start_hour: i32, end_hour: i32) -> i32 {
    (end_hour - start_hour) * HOUR_HEIGHT + HOUR_HEIGHT + 90
}

pub struct WindowManager {
    pub current_window: Option<gtk::Window>,
    day_events: Vec<Vec<Event>>,
    application: gtk::Application,
    refresh_state: Arc<Mutex<RefreshState>>,
    start_hour: i32,
    end_hour: i32,
    future_days: i32,
}

impl WindowManager {
    pub fn new(
        application: &gtk::Application,
        start_hour: i32,
        end_hour: i32,
        future_days: i32,
        refresh_state: Arc<Mutex<RefreshState>>,
    ) -> Self {
        WindowManager {
            current_window: None,
            day_events: Vec::new(),
            application: application.clone(),
            refresh_state,
            start_hour,
            end_hour,
            future_days,
        }
    }

    pub fn toggle_window(&mut self) {
        if let Some(window) = &self.current_window {
            if window.is_visible() {
                window.hide();
            } else {
                window.present();
            }
        } else {
            self.show_window();
        }
    }

    fn day_label_text(day_index: usize) -> String {
        if day_index == 0 {
            "Today".to_string()
        } else if day_index == 1 {
            "Tomorrow".to_string()
        } else {
            let date = Local::now().date_naive() + chrono::Duration::days(day_index as i64);
            format!("{}", date.format("%A, %B %d"))
        }
    }

    fn build_day_box(&self, day_index: usize, events: &[Event]) -> gtk::Box {
        let day_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let label_text = Self::day_label_text(day_index);

        let day_label = gtk::Label::new(Some(&label_text));
        day_label.set_xalign(0.0);
        day_label.set_margin_bottom(4);
        day_label.set_markup(&format!("<b>{}</b>", label_text));
        style_label_with_css(&day_label, TEXT_PRIMARY, "font-size: 15px;");

        day_box.append(&day_label);

        let timeline = TimelineView::new(
            events.to_vec(),
            self.start_hour,
            self.end_hour,
            day_index == 0,
        );
        timeline.container.set_hexpand(true);
        timeline.container.set_vexpand(true);
        day_box.append(&timeline.container);

        day_box
    }

    fn build_days_view(&self, day_events: &[Vec<Event>]) -> gtk::ScrolledWindow {
        let scrolled_window = gtk::ScrolledWindow::new();
        scrolled_window.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);

        let days_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        for (day_index, events) in day_events.iter().enumerate() {
            let day_box = self.build_day_box(day_index, events);
            day_box.set_hexpand(true);
            day_box.set_vexpand(true);
            days_box.append(&day_box);
        }

        scrolled_window.set_child(Some(&days_box));
        scrolled_window
    }

    pub fn show_window(&mut self) {
        let day_events = &self.day_events;

        if let Some(window) = &self.current_window {
            window.present();
            return;
        }

        let window = gtk::Window::builder()
            .application(&self.application)
            .build();
        window.set_title(Some("Calendar View"));
        window.set_default_size(
            DAY_MIN_WIDTH * (self.future_days + 1),
            calculate_window_height(self.start_hour, self.end_hour),
        );

        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        main_box.set_margin_start(6);
        main_box.set_margin_end(6);
        main_box.set_margin_top(6);
        main_box.set_margin_bottom(6);

        let scrolled_window = self.build_days_view(day_events);
        scrolled_window.set_hexpand(true);
        scrolled_window.set_vexpand(true);
        main_box.append(&scrolled_window);
        window.set_child(Some(&main_box));

        window.connect_close_request(move |window| {
            window.hide();
            glib::Propagation::Stop
        });

        window.present();
        self.current_window = Some(window);
    }

    pub fn update_events(&mut self, new_events: Vec<Vec<Event>>) {
        self.day_events = new_events;
        if let Some(window) = &self.current_window {
            if let Some(main_box) = window.child().and_downcast::<gtk::Box>() {
                while let Some(child) = main_box.first_child() {
                    main_box.remove(&child);
                }
                let scrolled_window = self.build_days_view(&self.day_events);
                scrolled_window.set_vexpand(true);
                main_box.append(&scrolled_window);
            }
        }
    }

    pub fn today_events(&self) -> Vec<Event> {
        self.day_events.first().cloned().unwrap_or_default()
    }

    pub fn refresh_state_snapshot(&self) -> RefreshState {
        self.refresh_state.lock().unwrap().clone()
    }

    pub fn refresh_log_dialog_data(&self) -> (Option<gtk::Window>, RefreshState) {
        (self.current_window.clone(), self.refresh_state_snapshot())
    }
}
