//! Shared widgets. Sliders here also respond to the mouse wheel while hovered.

use std::ops::RangeInclusive;

/// One wheel notch (egui reports ~50 points per notch) moves the value by this fraction
/// of the slider's range.
const NOTCH_FRACTION: f64 = 0.02;
const POINTS_PER_NOTCH: f64 = 50.0;

/// New value after scrolling by `scroll_y` points (positive = wheel up = increase).
pub fn scrolled_value(value: f64, range: &RangeInclusive<f64>, scroll_y: f64) -> f64 {
    let span = range.end() - range.start();
    (value + scroll_y / POINTS_PER_NOTCH * NOTCH_FRACTION * span).clamp(*range.start(), *range.end())
}

/// True when the pointer is anywhere on the slider's row (rail, value box or label).
/// Response::hovered alone only covers the part of the widget that senses the pointer.
fn over_row(ui: &egui::Ui, r: &egui::Response) -> bool {
    let row = egui::Rect::from_min_max(r.rect.min, egui::pos2(ui.max_rect().right().max(r.rect.max.x), r.rect.max.y));
    r.hovered() || ui.rect_contains_pointer(row)
}

/// Slider that also reacts to the mouse wheel. Returns true if the value changed.
pub fn slider(ui: &mut egui::Ui, value: &mut f64, range: RangeInclusive<f64>, text: &str) -> bool {
    let response = ui.add(egui::Slider::new(value, range.clone()).text(text));
    let mut changed = response.changed();
    if over_row(ui, &response) {
        // Take the scroll so the surrounding scroll area / canvas does not also react.
        let dy = ui.input_mut(|i| std::mem::take(&mut i.smooth_scroll_delta.y));
        if dy != 0.0 {
            let new = scrolled_value(*value, &range, f64::from(dy));
            if (new - *value).abs() > f64::EPSILON {
                *value = new;
                changed = true;
            }
        }
    }
    changed
}

/// Integer variant (rounds to the nearest whole number, at least one step per notch).
pub fn slider_u32(ui: &mut egui::Ui, value: &mut u32, range: RangeInclusive<u32>, text: &str) -> bool {
    let mut v = f64::from(*value);
    let fr = f64::from(*range.start())..=f64::from(*range.end());
    let response = ui.add(egui::Slider::new(value, range.clone()).text(text));
    let mut changed = response.changed();
    if over_row(ui, &response) {
        let dy = ui.input_mut(|i| std::mem::take(&mut i.smooth_scroll_delta.y));
        if dy != 0.0 {
            v = scrolled_value(v, &fr, f64::from(dy));
            *value = (v.round() as u32).clamp(*range.start(), *range.end());
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a few frames with the pointer over the slider and a wheel event; returns the value.
    fn wheel_over_slider(in_scroll_area: bool, wheel_y: f32, pointer_x: f32) -> f64 {
        let ctx = egui::Context::default();
        let mut v = 50.0_f64;
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 400.0));
        for frame in 0..6 {
            let mut raw = egui::RawInput { screen_rect: Some(screen), time: Some(f64::from(frame) * 0.016), ..Default::default() };
            raw.events.push(egui::Event::PointerMoved(egui::pos2(pointer_x, 14.0)));
            if frame == 3 {
                raw.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: egui::vec2(0.0, wheel_y),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let mut out = ctx.run_ui(raw, |ui| {
                if in_scroll_area {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        slider(ui, &mut v, 0.0..=100.0, "x");
                        ui.allocate_space(egui::vec2(10.0, 2000.0));
                    });
                } else {
                    slider(ui, &mut v, 0.0..=100.0, "x");
                }
            });
            out.textures_delta.clear();
        }
        v
    }

    #[test]
    fn wheel_changes_a_slider_even_inside_a_scroll_area() {
        assert!(wheel_over_slider(false, 1.0, 60.0) > 50.0, "plain");
        assert!(wheel_over_slider(true, 1.0, 60.0) > 50.0, "in scroll area");
        assert!(wheel_over_slider(true, -1.0, 60.0) < 50.0, "down");
    }

    #[test]
    fn wheel_works_over_the_value_box_and_label_too() {
        // Slider rail is on the left; 330 is over the value box / label area of a 400px row.
        for x in [200.0, 260.0, 330.0, 380.0] {
            assert!(wheel_over_slider(true, 1.0, x) > 50.0, "x = {x}");
        }
    }

    #[test]
    fn one_notch_is_two_percent_of_range() {
        let r = 0.0..=100.0;
        assert!((scrolled_value(50.0, &r, 50.0) - 52.0).abs() < 1e-9);
        assert!((scrolled_value(50.0, &r, -50.0) - 48.0).abs() < 1e-9);
    }

    #[test]
    fn clamps_to_range() {
        let r = -1.0..=1.0;
        assert_eq!(scrolled_value(0.99, &r, 5000.0), 1.0);
        assert_eq!(scrolled_value(-0.99, &r, -5000.0), -1.0);
    }
}

