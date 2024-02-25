use std::collections::VecDeque;

use getset::Getters;
use nanoid::nanoid;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

use crate::ring::Ring;
use crate::stackbar::Stackbar;
use crate::window::Window;
use crate::StackbarMode;
use crate::STACKBAR_MODE;

#[derive(Debug, Clone, Serialize, Deserialize, Getters, JsonSchema)]
pub struct Container {
    #[getset(get = "pub")]
    id: String,
    windows: Ring<Window>,
    #[getset(get = "pub", get_mut = "pub")]
    stackbar: Option<Stackbar>,
}

impl_ring_elements!(Container, Window);

impl Default for Container {
    fn default() -> Self {
        Self {
            id: nanoid!(),
            windows: Ring::default(),
            stackbar: match *STACKBAR_MODE.lock() {
                StackbarMode::Always => Stackbar::create().ok(),
                StackbarMode::Never | StackbarMode::OnStack => None,
            },
        }
    }
}

impl PartialEq for Container {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Container {
    pub fn hide(&self, omit: Option<isize>) {
        if let Some(stackbar) = self.stackbar() {
            stackbar.hide();
        }

        for window in self.windows().iter().rev() {
            let mut should_hide = omit.is_none();

            if !should_hide {
                if let Some(omit) = omit {
                    if omit != window.hwnd {
                        should_hide = true
                    }
                }
            }

            if should_hide {
                window.hide();
            }
        }
    }

    pub fn restore(&self) {
        if let Some(stackbar) = self.stackbar() {
            stackbar.restore();
        }

        if let Some(window) = self.focused_window() {
            window.restore();
        }
    }

    pub fn load_focused_window(&mut self) {
        let focused_idx = self.focused_window_idx();
        for (i, window) in self.windows_mut().iter_mut().enumerate() {
            if i == focused_idx {
                window.restore();
            } else {
                window.hide();
            }
        }
    }

    pub fn hwnd_from_exe(&self, exe: &str) -> Option<isize> {
        for window in self.windows() {
            if let Ok(window_exe) = window.exe() {
                if exe == window_exe {
                    return Option::from(window.hwnd);
                }
            }
        }

        None
    }

    pub fn contains_window(&self, hwnd: isize) -> bool {
        for window in self.windows() {
            if window.hwnd == hwnd {
                return true;
            }
        }

        false
    }

    pub fn idx_for_window(&self, hwnd: isize) -> Option<usize> {
        let mut idx = None;
        for (i, window) in self.windows().iter().enumerate() {
            if window.hwnd == hwnd {
                idx = Option::from(i);
            }
        }

        idx
    }

    pub fn remove_window_by_idx(&mut self, idx: usize) -> Option<Window> {
        let window = self.windows_mut().remove(idx);

        if matches!(*STACKBAR_MODE.lock(), StackbarMode::OnStack) && self.windows().len() <= 1 {
            self.stackbar = None;
        }

        if idx != 0 {
            self.focus_window(idx - 1);
        };

        window
    }

    pub fn remove_focused_window(&mut self) -> Option<Window> {
        let focused_idx = self.focused_window_idx();
        self.remove_window_by_idx(focused_idx)
    }

    pub fn add_window(&mut self, window: Window) {
        self.windows_mut().push_back(window);

        if matches!(*STACKBAR_MODE.lock(), StackbarMode::OnStack)
            && self.windows().len() > 1
            && self.stackbar.is_none()
        {
            self.stackbar = Stackbar::create().ok();
        }

        self.focus_window(self.windows().len() - 1);
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_window(&mut self, idx: usize) {
        tracing::info!("focusing window");
        self.windows.focus(idx);
    }
}
