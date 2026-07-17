use sim_core::SpawnKind;
use winit::keyboard::KeyCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpawnMenuMode {
    Closed,
    Browsing,
    Placing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SpawnMenu {
    mode: SpawnMenuMode,
    selected: usize,
}

impl Default for SpawnMenu {
    fn default() -> Self {
        Self {
            mode: SpawnMenuMode::Closed,
            selected: 0,
        }
    }
}

impl SpawnMenu {
    pub(crate) const fn mode(self) -> SpawnMenuMode {
        self.mode
    }

    pub(crate) const fn selected(self) -> SpawnKind {
        SpawnKind::ALL[self.selected]
    }

    pub(crate) const fn is_placing(self) -> bool {
        matches!(self.mode, SpawnMenuMode::Placing)
    }

    pub(crate) fn handle_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Numpad5 => {
                self.mode = match self.mode {
                    SpawnMenuMode::Closed | SpawnMenuMode::Placing => SpawnMenuMode::Browsing,
                    SpawnMenuMode::Browsing => SpawnMenuMode::Placing,
                };
                true
            }
            KeyCode::Numpad0 if self.mode != SpawnMenuMode::Closed => {
                self.mode = SpawnMenuMode::Closed;
                true
            }
            KeyCode::Numpad8 | KeyCode::Numpad4 if self.mode != SpawnMenuMode::Closed => {
                self.selected = self
                    .selected
                    .checked_sub(1)
                    .unwrap_or(SpawnKind::ALL.len() - 1);
                true
            }
            KeyCode::Numpad2 | KeyCode::Numpad6 if self.mode != SpawnMenuMode::Closed => {
                self.selected = (self.selected + 1) % SpawnKind::ALL.len();
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numpad_navigation_opens_selects_places_and_exits() {
        let mut menu = SpawnMenu::default();
        assert!(menu.handle_key(KeyCode::Numpad5));
        assert_eq!(menu.mode(), SpawnMenuMode::Browsing);
        assert_eq!(menu.selected(), SpawnKind::Tree);
        assert!(menu.handle_key(KeyCode::Numpad2));
        assert_eq!(menu.selected(), SpawnKind::BerryBush);
        assert!(menu.handle_key(KeyCode::Numpad5));
        assert!(menu.is_placing());
        assert!(menu.handle_key(KeyCode::Numpad0));
        assert_eq!(menu.mode(), SpawnMenuMode::Closed);
    }

    #[test]
    fn navigation_wraps_and_placing_can_return_to_the_menu() {
        let mut menu = SpawnMenu::default();
        menu.handle_key(KeyCode::Numpad5);
        menu.handle_key(KeyCode::Numpad8);
        assert_eq!(menu.selected(), SpawnKind::Water);
        menu.handle_key(KeyCode::Numpad5);
        menu.handle_key(KeyCode::Numpad5);
        assert_eq!(menu.mode(), SpawnMenuMode::Browsing);
    }
}
