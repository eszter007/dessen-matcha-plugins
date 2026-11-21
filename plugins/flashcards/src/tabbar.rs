//! matcha's bottom tab bar (components/HomeTabBar.cpp): Home, Library, Transfer, Insights,
//! Flashcards and Settings, the screen's own tab filled. Library, Transfer and Settings open the
//! firmware's screens; Insights and Flashcards switch plugin with open_app, which replaces this
//! one rather than stacking it. Dessen has no call for Home itself: that tab closes the plugin.
use crate::{dsn, gfx, tabicons as ic};
use alloc::{string::String, vec::Vec};

pub const HEIGHT: i32 = 72; // the Cover Grid theme's coverGridTabBarHeight
const SIDE: i32 = 18; // headerSidePadding: the outer icons line up with the header's corners
const ICON: i32 = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Home,
    Library,
    Transfer,
    Insights,
    Flashcards,
    Settings,
}

pub struct Bar {
    tabs: Vec<(Tab, String)>, // the app.bin for a plugin tab
    pub current: Tab,
}

/// The first of these plugin folders that holds an app.bin.
fn find_app(candidates: &[&str]) -> Option<String> {
    candidates.iter().find(|p| {
        let mut one = [0u8; 1];
        let n = dsn::fs_read(p, &mut one);
        n == dsn::ERR_TOO_BIG || n == 1
    }).map(|p| String::from(*p))
}

impl Bar {
    pub fn new(current: Tab) -> Bar {
        let mut tabs = Vec::new();
        tabs.push((Tab::Home, String::new()));
        tabs.push((Tab::Library, String::new()));
        tabs.push((Tab::Transfer, String::new()));
        if let Some(p) = find_app(&["/plugins/insights/app.bin", "/plugins/stats/app.bin"]) {
            tabs.push((Tab::Insights, p));
        }
        if let Some(p) = find_app(&["/plugins/flashcards/app.bin"]) {
            tabs.push((Tab::Flashcards, p));
        }
        tabs.push((Tab::Settings, String::new()));
        Bar { tabs, current }
    }
    pub fn count(&self) -> usize { self.tabs.len() }
    pub fn slot_of_current(&self) -> usize { self.tabs.iter().position(|t| t.0 == self.current).unwrap_or(0) }
    pub fn top() -> i32 { crate::ui::screen().1 - HEIGHT }
    fn step(&self) -> i32 { (crate::ui::screen().0 - 2 * SIDE) / self.tabs.len().max(1) as i32 }

    /// The band, with a focus ring on `focus` (a button board's cursor), as matcha draws it.
    pub fn draw(&self, focus: Option<usize>) {
        let top = Self::top();
        let step = self.step();
        dsn::fill(0, top, crate::ui::screen().0, HEIGHT, dsn::WHITE);
        for (i, (tab, _)) in self.tabs.iter().enumerate() {
            let x = SIDE + i as i32 * step;
            if focus == Some(i) {
                dsn::stroke(x, top, step, HEIGHT, dsn::BLACK, 4, 8);
            }
            let filled = *tab == self.current;
            let bits: &[u8; 128] = match tab {
                Tab::Home => if filled { &ic::HOME_FILLED_ICON } else { &ic::HOME_ICON },
                Tab::Library => if filled { &ic::LIBRARY_TAB_FILLED_ICON } else { &ic::LIBRARY_TAB_ICON },
                Tab::Transfer => if filled { &ic::TRANSFER_TAB_FILLED_ICON } else { &ic::TRANSFER_TAB_ICON },
                Tab::Insights => if filled { &ic::STATS_FILLED_ICON } else { &ic::STATS_ICON },
                Tab::Flashcards => if filled { &ic::FLASHCARDS_TAB_FILLED_ICON } else { &ic::FLASHCARDS_TAB_ICON },
                Tab::Settings => if filled { &ic::SETTINGS_TAB_FILLED_ICON } else { &ic::SETTINGS_TAB_ICON },
            };
            gfx::icon(x + (step - ICON) / 2, top + (HEIGHT - ICON) / 2, ICON, bits, dsn::BLACK);
        }
    }

    /// The slot under a tap, if it is in the band.
    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        if y < Self::top() || x < SIDE {
            return None;
        }
        let i = ((x - SIDE) / self.step().max(1)) as usize;
        (i < self.tabs.len()).then_some(i)
    }

    /// Opens the tab's screen; false when it is this screen's own tab. A successful switch ends
    /// the plugin, so callers save their state first.
    pub fn activate(&self, slot: usize) -> bool {
        let Some((tab, app)) = self.tabs.get(slot) else { return false };
        if *tab == self.current {
            return false;
        }
        match tab {
            Tab::Home => dsn::exit(0),
            Tab::Library => dsn::open_home(dsn::HOME_LIBRARY),
            Tab::Transfer => dsn::open_home(dsn::HOME_TRANSFER),
            Tab::Settings => dsn::open_home(dsn::HOME_SETTINGS),
            Tab::Insights | Tab::Flashcards => dsn::open_app(app),
        }
        true
    }
}
