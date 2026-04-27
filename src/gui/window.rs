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
    day_events: Arc<Mutex<Vec<Vec<Event>>>>,
    refresh_state: Arc<Mutex<RefreshState>>,
    start_hour: i32,
    end_hour: i32,
    future_days: i32,
}

impl WindowManager {
    pub fn new(
        start_hour: i32,
        end_hour: i32,
        future_days: i32,
        refresh_state: Arc<Mutex<RefreshState>>,
    ) -> Self {
        WindowManager {
            current_window: None,
            day_events: Arc::new(Mutex::new(Vec::new())),
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

        day_box.pack_start(&day_label, false, false, 0);

        let timeline = TimelineView::new(
            events.to_vec(),
            self.start_hour,
            self.end_hour,
            day_index == 0,
        );
        day_box.pack_start(&timeline.container, true, true, 0);

        day_box
    }

    fn build_days_view(&self, day_events: &[Vec<Event>]) -> gtk::ScrolledWindow {
        let scrolled_window =
            gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        scrolled_window.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);

        let days_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        for (day_index, events) in day_events.iter().enumerate() {
            let day_box = self.build_day_box(day_index, events);
            days_box.pack_start(&day_box, true, true, 0);
        }

        scrolled_window.add(&days_box);
        scrolled_window
    }

    pub fn show_window(&mut self) {
        let day_events = self.day_events.lock().unwrap();

        if let Some(window) = &self.current_window {
            if window.is_visible() {
                window.present();
                return;
            }
        }

        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title("Calendar View");
        window.set_default_size(
            DAY_MIN_WIDTH * (self.future_days + 1),
            calculate_window_height(self.start_hour, self.end_hour),
        );

        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        main_box.set_margin_start(6);
        main_box.set_margin_end(6);
        main_box.set_margin_top(6);
        main_box.set_margin_bottom(6);

        let scrolled_window = self.build_days_view(&day_events);
        main_box.pack_start(&scrolled_window, true, true, 0);
        window.add(&main_box);

        let window_clone = window.clone();
        window.connect_delete_event(move |_, _| {
            window_clone.hide();
            glib::Propagation::Stop
        });

        window.show_all();
        self.current_window = Some(window);
    }

    pub fn update_events(&mut self, new_events: Vec<Vec<Event>>) {
        let mut events = self.day_events.lock().unwrap();
        *events = new_events;

        if let Some(window) = &self.current_window {
            if let Some(main_box) = window.children().first() {
                let main_box = main_box.clone().downcast::<gtk::Box>().unwrap();
                main_box
                    .children()
                    .iter()
                    .for_each(|child| main_box.remove(child));

                let scrolled_window = self.build_days_view(&events);
                main_box.pack_start(&scrolled_window, true, true, 0);
                main_box.show_all();
            }
        }
    }

    pub fn today_events(&self) -> Vec<Event> {
        self.day_events
            .lock()
            .unwrap()
            .first()
            .cloned()
            .unwrap_or_default()
    }

    pub fn refresh_state_snapshot(&self) -> RefreshState {
        self.refresh_state.lock().unwrap().clone()
    }

    pub fn refresh_log_dialog_data(&self) -> (Option<gtk::Window>, RefreshState) {
        (self.current_window.clone(), self.refresh_state_snapshot())
    }
}
