//! Focus state machine for remote-driven navigation.
//!
//! This module deliberately knows nothing about Slint. It works entirely in
//! zones, tab indices and item indices, which makes the whole of the
//! navigation behaviour testable without a display — the state machine is the
//! part that has to be right, not the pixels.
//!
//! # Deliberate behaviours
//!
//! These are choices, not accidents, and the tests pin them down:
//!
//! - **Focus clamps, it does not wrap.** Holding right on the last tile leaves
//!   focus where it is. On a 10-foot interface a wrap looks like the list
//!   jumped, and the user has no cursor to reorient by.
//! - **Left/right stay on their row in a grid.** Moving right from the end of
//!   a row does not drop to the start of the next. Row changes are up/down
//!   only, so the two axes never surprise each other.
//! - **Down from a short last row lands on the last item.** Moving down into a
//!   partially filled row clamps to the final item rather than doing nothing,
//!   which would otherwise strand the user.
//! - **Each tab remembers where focus was.** Returning to a tab restores the
//!   previous position instead of resetting to the first tile.
//! - **Up from the first row reaches the tab bar.** The tab bar is part of the
//!   focus order, not mouse-only.

/// Which band of the interface currently holds focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// The row of tabs across the top.
    TabBar,
    /// The tiles belonging to the selected tab.
    Content,
}

/// How a tab arranges its items, which determines what up/down mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// A single horizontal row. Up leaves to the tab bar; down does nothing.
    Row,
    /// A wrapped grid. `columns` is clamped to at least 1 when used.
    Grid { columns: usize },
    /// Nothing focusable here, such as a screen that is not built yet.
    Empty,
}

/// A tab's shape and how many items it currently holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tab {
    pub layout: Layout,
    pub len: usize,
}

impl Tab {
    pub fn new(layout: Layout, len: usize) -> Self {
        Self { layout, len }
    }

    /// Effective column count: `Row` is one row of everything, `Grid` uses its
    /// own value with zero treated as one so division is always safe.
    fn columns(&self) -> usize {
        match self.layout {
            Layout::Row => self.len.max(1),
            Layout::Grid { columns } => columns.max(1),
            Layout::Empty => 1,
        }
    }

    fn is_focusable(&self) -> bool {
        self.len > 0 && self.layout != Layout::Empty
    }
}

/// A remote key press, already mapped from whatever produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Select,
    /// Back or Menu: step out one level.
    Back,
    /// Jump straight to the first tab's content from anywhere.
    Home,
    /// Previous tab, from anywhere.
    ///
    /// Reaching the tab bar by moving up out of the content is right for a
    /// d-pad remote and tedious on a controller, where shoulder buttons are
    /// the expected idiom. See ADR-0006 decision 2.
    TabPrev,
    /// Next tab, from anywhere.
    TabNext,
}

/// What the caller should do as a result of an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Focus changed. Redraw and scroll the focused item into view.
    Moved,
    /// Select was pressed on a content item.
    Activated { tab: usize, index: usize },
    /// The action had no effect, for example right at the end of a row.
    Ignored,
    /// Back pressed at the outermost level. The caller decides what this
    /// means; the launcher itself has nowhere further to go.
    ExitRequested,
}

/// Where focus is, and where it was in every tab previously visited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusModel {
    zone: Zone,
    tab: usize,
    /// Remembered content index per tab, so returning restores position.
    indices: Vec<usize>,
}

impl FocusModel {
    /// Start on the first tab's content with nothing else remembered.
    pub fn new(tab_count: usize) -> Self {
        Self {
            zone: Zone::Content,
            tab: 0,
            indices: vec![0; tab_count.max(1)],
        }
    }

    pub fn zone(&self) -> Zone {
        self.zone
    }

    pub fn tab(&self) -> usize {
        self.tab
    }

    /// Focused item within the current tab.
    pub fn index(&self) -> usize {
        self.indices.get(self.tab).copied().unwrap_or(0)
    }

    /// Focused item within an arbitrary tab, for driving per-tab UI state.
    pub fn index_of(&self, tab: usize) -> usize {
        self.indices.get(tab).copied().unwrap_or(0)
    }

    /// Place focus directly, as a pointer click does.
    ///
    /// A click is an absolute move rather than a step, so it bypasses the
    /// directional rules entirely. Out-of-range requests are ignored rather
    /// than clamped: they mean the caller and the model disagree about what is
    /// on screen, and silently focusing a different tile would hide that.
    pub fn focus_on(&mut self, tab: usize, index: usize, tabs: &[Tab]) -> Outcome {
        let Some(spec) = tabs.get(tab) else {
            return Outcome::Ignored;
        };
        if index >= spec.len {
            return Outcome::Ignored;
        }
        if self.indices.len() < tabs.len() {
            self.indices.resize(tabs.len(), 0);
        }
        self.tab = tab;
        self.zone = Zone::Content;
        self.set_index(index);
        Outcome::Moved
    }

    /// Select a tab without entering it, as clicking the tab bar does.
    pub fn focus_tab(&mut self, tab: usize, tabs: &[Tab]) -> Outcome {
        if tab >= tabs.len() {
            return Outcome::Ignored;
        }
        if self.indices.len() < tabs.len() {
            self.indices.resize(tabs.len(), 0);
        }
        self.tab = tab;
        self.zone = if tabs[tab].is_focusable() {
            Zone::Content
        } else {
            Zone::TabBar
        };
        self.clamp_to_bounds(tabs);
        Outcome::Moved
    }

    fn set_index(&mut self, index: usize) {
        if let Some(slot) = self.indices.get_mut(self.tab) {
            *slot = index;
        }
    }

    /// Keep the remembered index inside the tab's current bounds.
    ///
    /// Item counts change underneath the model when apps are installed or
    /// removed, so a remembered index can outlive the item it pointed at.
    fn clamp_to_bounds(&mut self, tabs: &[Tab]) {
        let Some(tab) = tabs.get(self.tab) else {
            return;
        };
        let max = tab.len.saturating_sub(1);
        if self.index() > max {
            self.set_index(max);
        }
    }

    /// Re-settle focus after the item counts changed underneath the model.
    ///
    /// A catalogue arriving, or an install finishing, changes how many tiles a
    /// tab has. Two things can go wrong without this: a remembered index now
    /// points past the end, and a tab that has become empty cannot hold focus
    /// at all. Either way the model would report a focused tile the user cannot
    /// see, and the next key press would move relative to it.
    ///
    /// Takes the new shapes rather than deltas, so a caller cannot describe a
    /// change that did not happen.
    pub fn reconcile(&mut self, tabs: &[Tab]) {
        if tabs.is_empty() {
            return;
        }
        if self.indices.len() < tabs.len() {
            self.indices.resize(tabs.len(), 0);
        }
        self.clamp_to_bounds(tabs);
        if self.zone == Zone::Content && !tabs.get(self.tab).is_some_and(Tab::is_focusable) {
            self.zone = Zone::TabBar;
        }
    }

    pub fn handle(&mut self, action: Action, tabs: &[Tab]) -> Outcome {
        if tabs.is_empty() {
            return Outcome::Ignored;
        }
        if self.tab >= tabs.len() {
            self.tab = 0;
        }
        if self.indices.len() < tabs.len() {
            self.indices.resize(tabs.len(), 0);
        }
        self.clamp_to_bounds(tabs);

        match action {
            Action::Home => self.go_home(tabs),
            Action::Back => self.go_back(tabs),
            Action::Select => self.select(tabs),
            Action::TabPrev => self.switch_tab(-1, tabs),
            Action::TabNext => self.switch_tab(1, tabs),
            _ => match self.zone {
                Zone::TabBar => self.move_in_tab_bar(action, tabs),
                Zone::Content => self.move_in_content(action, tabs),
            },
        }
    }

    fn go_home(&mut self, tabs: &[Tab]) -> Outcome {
        let already_home = self.tab == 0 && self.zone == Zone::Content && self.index() == 0;
        if already_home {
            return Outcome::Ignored;
        }
        self.tab = 0;
        self.set_index(0);
        self.zone = if tabs[0].is_focusable() {
            Zone::Content
        } else {
            Zone::TabBar
        };
        Outcome::Moved
    }

    /// Content steps out to the tab bar; the tab bar steps back to the first
    /// tab; the first tab with focus already on the bar has nowhere to go.
    fn go_back(&mut self, tabs: &[Tab]) -> Outcome {
        match self.zone {
            Zone::Content => {
                self.zone = Zone::TabBar;
                Outcome::Moved
            }
            Zone::TabBar if self.tab != 0 => {
                self.tab = 0;
                self.clamp_to_bounds(tabs);
                Outcome::Moved
            }
            Zone::TabBar => Outcome::ExitRequested,
        }
    }

    fn select(&mut self, tabs: &[Tab]) -> Outcome {
        match self.zone {
            // Selecting a tab drops focus into its content.
            Zone::TabBar => {
                if tabs[self.tab].is_focusable() {
                    self.zone = Zone::Content;
                    Outcome::Moved
                } else {
                    Outcome::Ignored
                }
            }
            Zone::Content => {
                if tabs[self.tab].is_focusable() {
                    Outcome::Activated {
                        tab: self.tab,
                        index: self.index(),
                    }
                } else {
                    Outcome::Ignored
                }
            }
        }
    }

    /// Move between tabs without going through the tab bar.
    ///
    /// Clamps rather than wrapping, matching every other movement in this
    /// model — a controller held against the end of the row should stop, not
    /// cycle round and land somewhere unexpected.
    fn switch_tab(&mut self, delta: isize, tabs: &[Tab]) -> Outcome {
        let Some(target) = self.tab.checked_add_signed(delta) else {
            return Outcome::Ignored;
        };
        if target >= tabs.len() {
            return Outcome::Ignored;
        }
        self.tab = target;
        self.clamp_to_bounds(tabs);
        // Focus cannot stay in content on a tab that has none, which is the
        // same reasoning `reconcile` applies when a tab empties.
        if self.zone == Zone::Content && !tabs[self.tab].is_focusable() {
            self.zone = Zone::TabBar;
        }
        Outcome::Moved
    }

    fn move_in_tab_bar(&mut self, action: Action, tabs: &[Tab]) -> Outcome {
        match action {
            Action::Left if self.tab > 0 => {
                self.tab -= 1;
                self.clamp_to_bounds(tabs);
                Outcome::Moved
            }
            Action::Right if self.tab + 1 < tabs.len() => {
                self.tab += 1;
                self.clamp_to_bounds(tabs);
                Outcome::Moved
            }
            Action::Down if tabs[self.tab].is_focusable() => {
                self.zone = Zone::Content;
                Outcome::Moved
            }
            // Up from the tab bar is the top of the interface.
            _ => Outcome::Ignored,
        }
    }

    fn move_in_content(&mut self, action: Action, tabs: &[Tab]) -> Outcome {
        let tab = tabs[self.tab];
        if !tab.is_focusable() {
            // An empty tab cannot hold focus; the tab bar is the only way out.
            return if action == Action::Up {
                self.zone = Zone::TabBar;
                Outcome::Moved
            } else {
                Outcome::Ignored
            };
        }

        let columns = tab.columns();
        let index = self.index();
        let row = index / columns;
        let column = index % columns;
        let last_row = (tab.len - 1) / columns;

        match action {
            Action::Left if column > 0 => {
                self.set_index(index - 1);
                Outcome::Moved
            }
            Action::Right if column + 1 < columns && index + 1 < tab.len => {
                self.set_index(index + 1);
                Outcome::Moved
            }
            Action::Up => {
                if row == 0 {
                    self.zone = Zone::TabBar;
                } else {
                    self.set_index(index - columns);
                }
                Outcome::Moved
            }
            Action::Down if row < last_row => {
                // Clamps into a partially filled final row rather than refusing.
                self.set_index((index + columns).min(tab.len - 1));
                Outcome::Moved
            }
            _ => Outcome::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Home is a single row of 5; Apps is a 3-column grid of 7; Store is a
    /// 3-column grid of 2; Settings has nothing focusable yet.
    fn tabs() -> Vec<Tab> {
        vec![
            Tab::new(Layout::Row, 5),
            Tab::new(Layout::Grid { columns: 3 }, 7),
            Tab::new(Layout::Grid { columns: 3 }, 2),
            Tab::new(Layout::Empty, 0),
        ]
    }

    fn model() -> FocusModel {
        FocusModel::new(4)
    }

    #[test]
    fn shoulder_buttons_switch_tabs_from_the_content_zone() {
        // The whole point of TabPrev/TabNext: no trip up to the tab bar.
        let mut m = model();
        assert_eq!(m.zone(), Zone::Content);
        assert_eq!(m.handle(Action::TabNext, &tabs()), Outcome::Moved);
        assert_eq!(m.tab(), 1);
        assert_eq!(m.zone(), Zone::Content, "should stay in the content zone");

        assert_eq!(m.handle(Action::TabPrev, &tabs()), Outcome::Moved);
        assert_eq!(m.tab(), 0);
    }

    #[test]
    fn switching_tabs_clamps_rather_than_wrapping() {
        let mut m = model();
        assert_eq!(m.handle(Action::TabPrev, &tabs()), Outcome::Ignored);
        assert_eq!(m.tab(), 0, "must not wrap round to the last tab");

        let last = tabs().len() - 1;
        for _ in 0..last {
            m.handle(Action::TabNext, &tabs());
        }
        assert_eq!(m.tab(), last);
        assert_eq!(m.handle(Action::TabNext, &tabs()), Outcome::Ignored);
        assert_eq!(m.tab(), last, "must not wrap round to the first tab");
    }

    #[test]
    fn switching_onto_an_empty_tab_leaves_focus_on_the_bar() {
        // Settings renders but holds nothing focusable, so content focus
        // there would point at a tile the user cannot see.
        let specs = tabs();
        let empty = specs
            .iter()
            .position(|t| !t.is_focusable())
            .expect("a tab with no focusable content");

        let mut m = model();
        for _ in 0..empty {
            m.handle(Action::TabNext, &specs);
        }
        assert_eq!(m.tab(), empty);
        assert_eq!(m.zone(), Zone::TabBar);
    }

    #[test]
    fn switching_tabs_from_the_tab_bar_stays_on_the_bar() {
        let mut m = model();
        m.handle(Action::Up, &tabs());
        assert_eq!(m.zone(), Zone::TabBar);
        m.handle(Action::TabNext, &tabs());
        assert_eq!(m.tab(), 1);
        assert_eq!(m.zone(), Zone::TabBar);
    }

    fn press(m: &mut FocusModel, actions: &[Action]) {
        for a in actions {
            m.handle(*a, &tabs());
        }
    }

    #[test]
    fn starts_on_the_first_tabs_first_item() {
        let m = model();
        assert_eq!(m.zone(), Zone::Content);
        assert_eq!(m.tab(), 0);
        assert_eq!(m.index(), 0);
    }

    // --- clamping -------------------------------------------------------

    #[test]
    fn right_clamps_at_the_end_of_a_row_rather_than_wrapping() {
        let mut m = model();
        press(&mut m, &[Action::Right; 4]);
        assert_eq!(m.index(), 4);

        assert_eq!(m.handle(Action::Right, &tabs()), Outcome::Ignored);
        assert_eq!(m.index(), 4, "must not wrap to the start");
    }

    #[test]
    fn left_clamps_at_the_start_of_a_row() {
        let mut m = model();
        assert_eq!(m.handle(Action::Left, &tabs()), Outcome::Ignored);
        assert_eq!(m.index(), 0);
    }

    #[test]
    fn right_stays_on_its_row_in_a_grid() {
        let mut m = model();
        m.handle(Action::Right, &tabs()); // tab bar reachable only via up
        let mut m = model();
        press(&mut m, &[Action::Up, Action::Right, Action::Down]); // to Apps grid
        assert_eq!(m.tab(), 1);
        assert_eq!(m.index(), 0);

        press(&mut m, &[Action::Right, Action::Right]);
        assert_eq!(m.index(), 2, "at the end of row 0");

        assert_eq!(m.handle(Action::Right, &tabs()), Outcome::Ignored);
        assert_eq!(m.index(), 2, "must not fall through to the next row");
    }

    // --- grid movement --------------------------------------------------

    #[test]
    fn down_moves_a_whole_row_in_a_grid() {
        let mut m = model();
        press(&mut m, &[Action::Up, Action::Right, Action::Down]);
        press(&mut m, &[Action::Down]);
        assert_eq!(m.index(), 3, "0 + 3 columns");
    }

    #[test]
    fn down_into_a_short_final_row_clamps_to_the_last_item() {
        // Apps has 7 items over 3 columns: rows are [0,1,2] [3,4,5] [6].
        let mut m = model();
        press(&mut m, &[Action::Up, Action::Right, Action::Down]);
        press(&mut m, &[Action::Right, Action::Right]); // index 2, row 0
        press(&mut m, &[Action::Down]); // index 5, row 1

        assert_eq!(m.index(), 5);
        assert_eq!(m.handle(Action::Down, &tabs()), Outcome::Moved);
        assert_eq!(m.index(), 6, "clamped to the only item in the last row");
    }

    #[test]
    fn down_on_the_last_row_does_nothing() {
        let mut m = model();
        press(&mut m, &[Action::Up, Action::Right, Action::Down]);
        press(&mut m, &[Action::Down, Action::Down]); // to index 6, last row
        assert_eq!(m.index(), 6);
        assert_eq!(m.handle(Action::Down, &tabs()), Outcome::Ignored);
    }

    #[test]
    fn up_moves_a_whole_row_before_leaving_to_the_tab_bar() {
        let mut m = model();
        press(&mut m, &[Action::Up, Action::Right, Action::Down]);
        press(&mut m, &[Action::Down]); // row 1
        assert_eq!(m.index(), 3);

        m.handle(Action::Up, &tabs());
        assert_eq!(m.index(), 0, "back to row 0, still in content");
        assert_eq!(m.zone(), Zone::Content);

        m.handle(Action::Up, &tabs());
        assert_eq!(m.zone(), Zone::TabBar, "only now does it leave");
    }

    // --- tab bar --------------------------------------------------------

    #[test]
    fn up_from_the_first_row_reaches_the_tab_bar() {
        let mut m = model();
        assert_eq!(m.handle(Action::Up, &tabs()), Outcome::Moved);
        assert_eq!(m.zone(), Zone::TabBar);
    }

    #[test]
    fn up_from_the_tab_bar_is_the_top() {
        let mut m = model();
        press(&mut m, &[Action::Up]);
        assert_eq!(m.handle(Action::Up, &tabs()), Outcome::Ignored);
        assert_eq!(m.zone(), Zone::TabBar);
    }

    #[test]
    fn left_right_change_tab_while_in_the_tab_bar() {
        let mut m = model();
        press(&mut m, &[Action::Up]);
        assert_eq!(m.tab(), 0);

        m.handle(Action::Right, &tabs());
        assert_eq!(m.tab(), 1);
        assert_eq!(m.zone(), Zone::TabBar, "changing tab does not enter it");

        m.handle(Action::Left, &tabs());
        assert_eq!(m.tab(), 0);
    }

    #[test]
    fn tab_selection_clamps_at_both_ends() {
        let mut m = model();
        press(&mut m, &[Action::Up]);
        assert_eq!(m.handle(Action::Left, &tabs()), Outcome::Ignored);
        assert_eq!(m.tab(), 0);

        press(&mut m, &[Action::Right; 3]);
        assert_eq!(m.tab(), 3);
        assert_eq!(m.handle(Action::Right, &tabs()), Outcome::Ignored);
        assert_eq!(m.tab(), 3);
    }

    #[test]
    fn down_from_the_tab_bar_enters_the_content() {
        let mut m = model();
        press(&mut m, &[Action::Up, Action::Right]);
        assert_eq!(m.handle(Action::Down, &tabs()), Outcome::Moved);
        assert_eq!(m.zone(), Zone::Content);
        assert_eq!(m.tab(), 1);
    }

    #[test]
    fn select_on_a_tab_enters_it_rather_than_launching() {
        let mut m = model();
        press(&mut m, &[Action::Up]);
        assert_eq!(m.handle(Action::Select, &tabs()), Outcome::Moved);
        assert_eq!(m.zone(), Zone::Content);
    }

    // --- empty tabs -----------------------------------------------------

    #[test]
    fn an_empty_tab_cannot_be_entered() {
        let mut m = model();
        press(
            &mut m,
            &[Action::Up, Action::Right, Action::Right, Action::Right],
        );
        assert_eq!(m.tab(), 3, "Settings");

        assert_eq!(m.handle(Action::Down, &tabs()), Outcome::Ignored);
        assert_eq!(m.zone(), Zone::TabBar);
        assert_eq!(m.handle(Action::Select, &tabs()), Outcome::Ignored);
    }

    #[test]
    fn focus_stranded_in_a_tab_that_became_empty_can_still_escape() {
        let mut m = model();
        let mut shrinking = tabs();
        shrinking[0] = Tab::new(Layout::Row, 0);

        assert_eq!(m.zone(), Zone::Content);
        assert_eq!(m.handle(Action::Up, &shrinking), Outcome::Moved);
        assert_eq!(m.zone(), Zone::TabBar);
    }

    // --- memory ---------------------------------------------------------

    #[test]
    fn each_tab_remembers_where_focus_was() {
        let mut m = model();
        press(&mut m, &[Action::Right, Action::Right]); // Home index 2
        assert_eq!(m.index(), 2);

        // Go to Apps and move down a row.
        press(
            &mut m,
            &[Action::Up, Action::Right, Action::Down, Action::Down],
        );
        assert_eq!(m.tab(), 1);
        assert_eq!(m.index(), 3);

        // Leave via Back rather than Up. Up would walk out through the grid,
        // which legitimately moves Apps' own focus back to row 0 on the way.
        press(&mut m, &[Action::Back, Action::Left, Action::Down]);
        assert_eq!(m.tab(), 0);
        assert_eq!(m.index(), 2, "Home restored its position");
        assert_eq!(m.index_of(1), 3, "Apps kept its own position");
    }

    #[test]
    fn walking_up_out_of_a_grid_moves_that_tabs_focus_on_the_way() {
        // The counterpart to the test above: leaving by Up is navigation, so
        // it is expected to change where that tab is focused.
        let mut m = model();
        press(
            &mut m,
            &[Action::Up, Action::Right, Action::Down, Action::Down],
        );
        assert_eq!(m.index_of(1), 3);

        press(&mut m, &[Action::Up]);
        assert_eq!(m.index_of(1), 0, "stepped up a row before leaving");
        assert_eq!(m.zone(), Zone::Content);

        press(&mut m, &[Action::Up]);
        assert_eq!(m.zone(), Zone::TabBar);
    }

    #[test]
    fn a_remembered_index_past_the_end_is_clamped() {
        // An app is uninstalled while focus sits on the last tile.
        let mut m = model();
        press(&mut m, &[Action::Right; 4]);
        assert_eq!(m.index(), 4);

        let shrunk = vec![
            Tab::new(Layout::Row, 2),
            Tab::new(Layout::Grid { columns: 3 }, 7),
            Tab::new(Layout::Grid { columns: 3 }, 2),
            Tab::new(Layout::Empty, 0),
        ];
        m.handle(Action::Left, &shrunk);
        assert!(m.index() <= 1, "index {} out of bounds", m.index());
    }

    // --- back and home --------------------------------------------------

    #[test]
    fn back_steps_out_of_content_to_the_tab_bar() {
        let mut m = model();
        assert_eq!(m.handle(Action::Back, &tabs()), Outcome::Moved);
        assert_eq!(m.zone(), Zone::TabBar);
    }

    #[test]
    fn back_from_another_tab_returns_to_the_first() {
        let mut m = model();
        press(&mut m, &[Action::Up, Action::Right, Action::Right]);
        assert_eq!(m.tab(), 2);

        assert_eq!(m.handle(Action::Back, &tabs()), Outcome::Moved);
        assert_eq!(m.tab(), 0);
    }

    #[test]
    fn back_at_the_outermost_level_requests_exit() {
        let mut m = model();
        press(&mut m, &[Action::Back]); // content -> tab bar
        assert_eq!(m.handle(Action::Back, &tabs()), Outcome::ExitRequested);
    }

    #[test]
    fn home_jumps_from_anywhere() {
        let mut m = model();
        press(
            &mut m,
            &[Action::Up, Action::Right, Action::Down, Action::Down],
        );
        assert_eq!(m.tab(), 1);

        assert_eq!(m.handle(Action::Home, &tabs()), Outcome::Moved);
        assert_eq!(m.tab(), 0);
        assert_eq!(m.index(), 0);
        assert_eq!(m.zone(), Zone::Content);
    }

    #[test]
    fn home_when_already_home_does_nothing() {
        let mut m = model();
        assert_eq!(m.handle(Action::Home, &tabs()), Outcome::Ignored);
    }

    // --- activation -----------------------------------------------------

    #[test]
    fn select_in_content_activates_the_focused_item() {
        let mut m = model();
        press(&mut m, &[Action::Right, Action::Right]);
        assert_eq!(
            m.handle(Action::Select, &tabs()),
            Outcome::Activated { tab: 0, index: 2 }
        );
    }

    // --- pointer input ---------------------------------------------------

    #[test]
    fn clicking_a_tile_moves_focus_there_directly() {
        let mut m = model();
        assert_eq!(m.focus_on(1, 5, &tabs()), Outcome::Moved);
        assert_eq!(m.tab(), 1);
        assert_eq!(m.index(), 5);
        assert_eq!(m.zone(), Zone::Content);
    }

    #[test]
    fn clicking_out_of_range_is_ignored_rather_than_clamped() {
        // A silent clamp would hide a genuine disagreement between the model
        // and what is actually on screen.
        let mut m = model();
        assert_eq!(m.focus_on(1, 99, &tabs()), Outcome::Ignored);
        assert_eq!(m.focus_on(9, 0, &tabs()), Outcome::Ignored);
        assert_eq!(m.tab(), 0);
        assert_eq!(m.index(), 0);
    }

    #[test]
    fn clicking_a_tab_enters_it_when_it_has_content() {
        let mut m = model();
        assert_eq!(m.focus_tab(2, &tabs()), Outcome::Moved);
        assert_eq!(m.tab(), 2);
        assert_eq!(m.zone(), Zone::Content);
    }

    #[test]
    fn clicking_an_empty_tab_leaves_focus_on_the_bar() {
        let mut m = model();
        assert_eq!(m.focus_tab(3, &tabs()), Outcome::Moved);
        assert_eq!(m.tab(), 3);
        assert_eq!(m.zone(), Zone::TabBar);
    }

    // --- degenerate input ------------------------------------------------

    #[test]
    fn no_tabs_at_all_is_survivable() {
        let mut m = FocusModel::new(0);
        for action in [Action::Up, Action::Down, Action::Left, Action::Select] {
            assert_eq!(m.handle(action, &[]), Outcome::Ignored);
        }
    }

    #[test]
    fn a_zero_column_grid_does_not_divide_by_zero() {
        let mut m = FocusModel::new(1);
        let broken = vec![Tab::new(Layout::Grid { columns: 0 }, 3)];
        // Reaching here at all is the assertion: columns() clamps to 1, so
        // the internal index / columns never divides by zero.
        m.handle(Action::Down, &broken);
        m.handle(Action::Right, &broken);
        assert!(m.index() < 3);
    }

    #[test]
    fn a_single_item_row_has_nowhere_to_go() {
        let mut m = FocusModel::new(1);
        let one = vec![Tab::new(Layout::Row, 1)];
        assert_eq!(m.handle(Action::Right, &one), Outcome::Ignored);
        assert_eq!(m.handle(Action::Left, &one), Outcome::Ignored);
        assert_eq!(m.handle(Action::Down, &one), Outcome::Ignored);
        assert_eq!(m.index(), 0);
    }

    #[test]
    fn tab_count_growing_at_runtime_is_absorbed() {
        // The model was built for one tab but is handed four.
        let mut m = FocusModel::new(1);
        assert_eq!(m.handle(Action::Up, &tabs()), Outcome::Moved);
        press(&mut m, &[Action::Right, Action::Down]);
        assert_eq!(m.tab(), 1);
        assert_eq!(m.index(), 0);
    }

    // --- lists changing underneath focus ----------------------------------

    #[test]
    fn reconcile_pulls_focus_back_inside_a_shrunken_tab() {
        // A catalogue refresh that returns fewer apps than before.
        let mut m = model();
        press(
            &mut m,
            &[Action::Up, Action::Right, Action::Right, Action::Down],
        );
        press(&mut m, &[Action::Right]);
        assert_eq!(m.tab(), 2);
        assert_eq!(m.index(), 1);

        let mut shrunk = tabs();
        shrunk[2] = Tab::new(Layout::Grid { columns: 3 }, 1);
        m.reconcile(&shrunk);

        assert_eq!(m.index(), 0, "focus was left past the end");
        assert_eq!(m.zone(), Zone::Content);
    }

    #[test]
    fn reconcile_lifts_focus_out_of_a_tab_that_became_empty() {
        // Otherwise the model reports a focused tile that is not on screen and
        // the next key press moves relative to it.
        let mut m = model();
        press(
            &mut m,
            &[Action::Up, Action::Right, Action::Right, Action::Down],
        );
        assert_eq!(m.zone(), Zone::Content);

        let mut emptied = tabs();
        emptied[2] = Tab::new(Layout::Empty, 0);
        m.reconcile(&emptied);

        assert_eq!(m.zone(), Zone::TabBar);
        assert_eq!(m.tab(), 2, "still on the tab, just not inside it");
    }

    #[test]
    fn reconcile_leaves_focus_alone_when_a_tab_grows() {
        // The catalogue arriving is the common case, and it must not move a
        // selection the user made while it was in flight.
        let mut m = model();
        press(&mut m, &[Action::Right, Action::Right]);
        assert_eq!(m.index(), 2);

        let mut grown = tabs();
        grown[0] = Tab::new(Layout::Row, 9);
        m.reconcile(&grown);

        assert_eq!(m.index(), 2);
        assert_eq!(m.zone(), Zone::Content);
    }

    #[test]
    fn reconcile_on_the_tab_bar_does_not_drag_focus_into_content() {
        let mut m = model();
        press(&mut m, &[Action::Up]);
        assert_eq!(m.zone(), Zone::TabBar);

        m.reconcile(&tabs());

        assert_eq!(m.zone(), Zone::TabBar);
    }

    #[test]
    fn reconcile_with_no_tabs_at_all_is_a_no_op() {
        let mut m = model();
        press(&mut m, &[Action::Right]);
        m.reconcile(&[]);
        assert_eq!(m.tab(), 0);
        assert_eq!(m.index(), 1);
    }
}
