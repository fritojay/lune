//! The main window's size and pane layout, restored from the last run and
//! saved, with the browse position (`session::browse`), when the app quits.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::pin::Pin;

use cxx_qt::CxxQtType;
use lune_core::WindowState;

use crate::{format::qml_int, session};

#[cxx_qt::bridge]
pub mod qobject {
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, width, READ, NOTIFY)]
        #[qproperty(i32, height, READ, NOTIFY)]
        #[qproperty(bool, maximized, READ, CONSTANT)]
        #[qproperty(i32, filter_pane_width, cxx_name = "filterPaneWidth", READ, NOTIFY)]
        #[qproperty(i32, queue_pane_width, cxx_name = "queuePaneWidth", READ, NOTIFY)]
        #[qproperty(bool, queue_shown, cxx_name = "queueShown", READ, CONSTANT)]
        type WindowLayout = super::WindowLayoutRust;
    }

    extern "RustQt" {
        /// Fits the saved layout to the screen (`WindowState::fitted`).
        #[qinvokable]
        fn fit(
            self: Pin<&mut WindowLayout>,
            available_width: i32,
            available_height: i32,
            main_pane_width: f64,
        );

        /// `width` and `height` are the size when not maximized.
        #[qinvokable]
        fn save(
            self: Pin<&mut WindowLayout>,
            width: f64,
            height: f64,
            maximized: bool,
            filter_pane_width: f64,
            queue_pane_width: f64,
            queue_shown: bool,
        );
    }
}

pub struct WindowLayoutRust {
    width: i32,
    height: i32,
    maximized: bool,
    filter_pane_width: i32,
    queue_pane_width: i32,
    queue_shown: bool,
}

impl From<&WindowState> for WindowLayoutRust {
    fn from(state: &WindowState) -> Self {
        Self {
            width: qml_int(state.width),
            height: qml_int(state.height),
            maximized: state.maximized,
            filter_pane_width: qml_int(state.filter_pane_width),
            queue_pane_width: qml_int(state.queue_pane_width),
            queue_shown: state.queue_shown,
        }
    }
}

impl Default for WindowLayoutRust {
    fn default() -> Self {
        Self::from(session::saved_window())
    }
}

impl qobject::WindowLayout {
    fn fit(
        mut self: Pin<&mut Self>,
        available_width: i32,
        available_height: i32,
        main_pane_width: f64,
    ) {
        let fitted = session::saved_window().clone().fitted(
            u32::try_from(available_width).unwrap_or(0),
            u32::try_from(available_height).unwrap_or(0),
            pixels(main_pane_width),
        );
        let layout = WindowLayoutRust::from(&fitted);
        self.as_mut().rust_mut().width = layout.width;
        self.as_mut().width_changed();
        self.as_mut().rust_mut().height = layout.height;
        self.as_mut().height_changed();
        self.as_mut().rust_mut().filter_pane_width = layout.filter_pane_width;
        self.as_mut().filter_pane_width_changed();
        self.as_mut().rust_mut().queue_pane_width = layout.queue_pane_width;
        self.queue_pane_width_changed();
    }

    fn save(
        self: Pin<&mut Self>,
        width: f64,
        height: f64,
        maximized: bool,
        filter_pane_width: f64,
        queue_pane_width: f64,
        queue_shown: bool,
    ) {
        let config = crate::config();
        if config.bench {
            return;
        }
        let state = WindowState {
            width: pixels(width),
            height: pixels(height),
            maximized,
            filter_pane_width: pixels(filter_pane_width),
            queue_pane_width: pixels(queue_pane_width),
            queue_shown,
            browse: session::browse(),
        };
        if let Err(err) = state.save(&config.window_state_path) {
            tracing::warn!(%err, "the window layout was not saved");
        }
    }
}

/// QML sizes are reals. The cast saturates, and NaN becomes 0.
fn pixels(value: f64) -> u32 {
    value.round() as u32
}

#[cfg(test)]
#[path = "tests/window_layout_test.rs"]
mod tests;
