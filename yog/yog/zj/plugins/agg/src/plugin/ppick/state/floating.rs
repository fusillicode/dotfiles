use zellij_tile::prelude::FloatingPaneCoordinates;
use zellij_tile::prelude::TabInfo;

use crate::plugin::ppick::state::PpickState;

impl PpickState {
    pub fn set_floating_coordinates(&mut self, y: Option<String>, width: Option<String>, height: Option<String>) {
        self.floating_y = y;
        self.floating_width = width;
        self.floating_height = height;
        self.floating_size_applied = false;
    }

    pub fn take_floating_coordinates(&mut self) -> Option<FloatingPaneCoordinates> {
        if self.floating_size_applied {
            return None;
        }
        let coordinates = match (self.floating_display_rows, self.floating_display_columns) {
            (Some(display_rows), Some(display_columns)) => {
                self.centered_floating_coordinates(display_rows, display_columns)
            }
            _ => FloatingPaneCoordinates::new(
                None,
                self.floating_y.clone(),
                self.floating_width.clone(),
                self.floating_height.clone(),
                None,
                Some(false),
            ),
        }?;
        self.floating_size_applied = true;
        Some(coordinates)
    }

    pub(super) fn update_floating_display_area(&mut self, tabs: &[TabInfo]) -> bool {
        let Some(active_tab) = tabs.iter().find(|tab| tab.active) else {
            return false;
        };
        if active_tab.display_area_rows == 0 || active_tab.display_area_columns == 0 {
            return false;
        }

        let display_area_changed = self.floating_display_rows != Some(active_tab.display_area_rows)
            || self.floating_display_columns != Some(active_tab.display_area_columns);
        if display_area_changed {
            self.floating_display_rows = Some(active_tab.display_area_rows);
            self.floating_display_columns = Some(active_tab.display_area_columns);
            self.floating_size_applied = false;
        }
        display_area_changed
    }

    fn centered_floating_coordinates(
        &self,
        display_rows: usize,
        display_columns: usize,
    ) -> Option<FloatingPaneCoordinates> {
        let width = fixed_cells(self.floating_width.as_deref(), display_columns);
        let height = fixed_cells(self.floating_height.as_deref(), display_rows);
        let x = width.map(|width| display_columns.saturating_sub(width) / 2);
        let y = fixed_cells(self.floating_y.as_deref(), display_rows);
        FloatingPaneCoordinates::new(
            x.map(|x| x.to_string()),
            y.map(|y| y.to_string()),
            width.map(|width| width.to_string()),
            height.map(|height| height.to_string()),
            None,
            Some(false),
        )
    }
}

fn fixed_cells(value: Option<&str>, total: usize) -> Option<usize> {
    let value = value?;
    if let Some(percent) = value.strip_suffix('%') {
        let percent = percent.parse::<usize>().ok()?;
        if percent > 100 {
            return None;
        }
        Some(total.saturating_mul(percent) / 100)
    } else {
        value.parse::<usize>().ok()
    }
}
